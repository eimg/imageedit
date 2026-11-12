use gpui::{Context, CursorStyle, Pixels, Point, Window, point};

use super::Workspace;
use super::canvas_tools::PreviewGeometry;
use super::layers::MovePaint;
use crate::commands::Tool;
use crate::document::{self, LayerId, MAX_NEW_EDGE};
use crate::preview::{Dirty, PixelRect};

const SCALE_SLOP: f32 = 6.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScaleEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ScaleDrag {
    layer: LayerId,
    original: PixelRect,
    edges: ScaleEdges,
    start: (i64, i64),
    target: PixelRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScaleHit {
    Outside,
    Inside,
    Edge(ScaleEdges),
}

impl Workspace {
    pub(super) fn scale_frame(&self) -> Option<PixelRect> {
        if self.tool != Tool::ScaleLayer {
            return None;
        }
        if let Some(drag) = self.scale_drag {
            return Some(drag.target);
        }
        self.active_tab().and_then(|tab| {
            let layer = tab.document.content.layer(tab.active_layer)?;
            Some(PixelRect {
                x: i64::from(layer.offset.0),
                y: i64::from(layer.offset.1),
                width: layer.pixels.width(),
                height: layer.pixels.height(),
            })
        })
    }

    pub(super) fn scale_size(&self) -> Option<(u32, u32)> {
        if let Some(drag) = self.scale_drag {
            return Some((drag.target.width, drag.target.height));
        }
        let tab = self.active_tab()?;
        let layer = tab.document.content.layer(tab.active_layer)?;
        Some((layer.pixels.width(), layer.pixels.height()))
    }

    pub(super) fn begin_scale(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        if self.tool != Tool::ScaleLayer || self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        let layer = match tab.document.content.layer(tab.active_layer) {
            Some(layer) => layer,
            None => return,
        };
        if layer.text.is_some() {
            self.notice("Rasterize the text layer before scaling it.", cx);
            return;
        }
        let id = layer.id;
        let opacity = layer.opacity;
        let original = PixelRect {
            x: i64::from(layer.offset.0),
            y: i64::from(layer.offset.1),
            width: layer.pixels.width(),
            height: layer.pixels.height(),
        };
        let Some((left, top, right, bottom)) = self.scale_screen_rect(original, scale) else {
            return;
        };
        let ScaleHit::Edge(edges) = hit_scale(
            left,
            top,
            right,
            bottom,
            f32::from(position.x),
            f32::from(position.y),
            SCALE_SLOP,
        ) else {
            return;
        };
        let Some(start) = self.unclamped_pointer_pixel(position, scale) else {
            return;
        };
        let (below, above) = tab.document.content.composite_around(id);
        let sprite =
            document::preview_render(&super::layers::with_layer_opacity(&layer.pixels, opacity));
        self.scale_overlay = Some(MovePaint {
            below: below.as_ref().map(document::preview_render),
            above: above.as_ref().map(document::preview_render),
            sprite,
            offset: (original.x as i32, original.y as i32),
            size: (original.width, original.height),
        });
        self.scale_drag = Some(ScaleDrag {
            layer: id,
            original,
            edges,
            start,
            target: original,
        });
        cx.notify();
    }

    pub(super) fn extend_scale(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.scale_drag else {
            return;
        };
        let Some(pointer) = self.unclamped_pointer_pixel(position, scale) else {
            return;
        };
        let target = scale_target(drag.original, drag.edges, drag.start, pointer);
        if target == drag.target {
            return;
        }
        if let Some(drag) = self.scale_drag.as_mut() {
            drag.target = target;
        }
        if let Some(overlay) = self.scale_overlay.as_mut() {
            overlay.offset = (target.x as i32, target.y as i32);
            overlay.size = (target.width, target.height);
        }
        cx.notify();
    }

    pub(super) fn finish_scale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.scale_drag.take() else {
            return;
        };
        let overlay = self.scale_overlay.take();
        if drag.target == drag.original {
            if let Some(overlay) = overlay {
                super::drop_images(overlay.into_images(), window, cx);
            }
            cx.notify();
            return;
        }
        let id = drag.layer;
        let target = drag.target;
        let resample = self.warp_resample;
        self.edit_content(
            move |content, _, _| {
                if !content.scale_layer(id, target.width, target.height, resample) {
                    return Dirty::None;
                }
                let _ = content.set_offset(id, (target.x as i32, target.y as i32));
                Dirty::Canvas
            },
            window,
            cx,
        );
        if let Some(overlay) = overlay {
            super::drop_images(overlay.into_images(), window, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_scale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scale_drag = None;
        if let Some(overlay) = self.scale_overlay.take() {
            super::drop_images(overlay.into_images(), window, cx);
        }
        cx.notify();
    }

    pub(super) fn scale_cursor(&self, scale: f32) -> CursorStyle {
        if self.tool != Tool::ScaleLayer {
            return CursorStyle::Crosshair;
        }
        if let Some(drag) = self.scale_drag {
            return drag.edges.cursor();
        }
        let Some(frame) = self.scale_frame() else {
            return CursorStyle::Crosshair;
        };
        let Some(position) = self.canvas_cursor.position.get() else {
            return CursorStyle::Crosshair;
        };
        let Some((left, top, right, bottom)) = self.scale_screen_rect(frame, scale) else {
            return CursorStyle::Crosshair;
        };
        match hit_scale(
            left,
            top,
            right,
            bottom,
            f32::from(position.x),
            f32::from(position.y),
            SCALE_SLOP,
        ) {
            ScaleHit::Edge(edges) => edges.cursor(),
            ScaleHit::Inside | ScaleHit::Outside => CursorStyle::Crosshair,
        }
    }

    fn scale_screen_rect(&self, frame: PixelRect, scale: f32) -> Option<(f32, f32, f32, f32)> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        if width == 0 || height == 0 || frame.width == 0 || frame.height == 0 {
            return None;
        }
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let start = geometry.project(point(
            frame.x as f32 / width as f32,
            frame.y as f32 / height as f32,
        ));
        let end = geometry.project(point(
            (frame.x + i64::from(frame.width)) as f32 / width as f32,
            (frame.y + i64::from(frame.height)) as f32 / height as f32,
        ));
        Some((
            f32::from(start.x.min(end.x)),
            f32::from(start.y.min(end.y)),
            f32::from(start.x.max(end.x)),
            f32::from(start.y.max(end.y)),
        ))
    }
}

fn scale_target(
    original: PixelRect,
    edges: ScaleEdges,
    start: (i64, i64),
    pointer: (i64, i64),
) -> PixelRect {
    let dx = pointer.0.saturating_sub(start.0);
    let dy = pointer.1.saturating_sub(start.1);
    let mut left = original.x;
    let mut top = original.y;
    let mut right = original.x.saturating_add(i64::from(original.width));
    let mut bottom = original.y.saturating_add(i64::from(original.height));
    if edges.left {
        left = left.saturating_add(dx);
    }
    if edges.right {
        right = right.saturating_add(dx);
    }
    if edges.top {
        top = top.saturating_add(dy);
    }
    if edges.bottom {
        bottom = bottom.saturating_add(dy);
    }
    if left >= right {
        if edges.left {
            left = right - 1;
        } else {
            right = left + 1;
        }
    }
    if top >= bottom {
        if edges.top {
            top = bottom - 1;
        } else {
            bottom = top + 1;
        }
    }
    let width = (right - left).clamp(1, i64::from(MAX_NEW_EDGE)) as u32;
    let height = (bottom - top).clamp(1, i64::from(MAX_NEW_EDGE)) as u32;
    PixelRect::new(
        left.clamp(i64::from(i32::MIN), i64::from(i32::MAX)),
        top.clamp(i64::from(i32::MIN), i64::from(i32::MAX)),
        width,
        height,
    )
}

fn hit_scale(left: f32, top: f32, right: f32, bottom: f32, x: f32, y: f32, slop: f32) -> ScaleHit {
    if x < left - slop || x > right + slop || y < top - slop || y > bottom + slop {
        return ScaleHit::Outside;
    }
    let mut near_left = (x - left).abs() <= slop;
    let mut near_right = (x - right).abs() <= slop;
    let mut near_top = (y - top).abs() <= slop;
    let mut near_bottom = (y - bottom).abs() <= slop;
    if near_left && near_right {
        near_left = x - left <= right - x;
        near_right = !near_left;
    }
    if near_top && near_bottom {
        near_top = y - top <= bottom - y;
        near_bottom = !near_top;
    }
    if near_left || near_right || near_top || near_bottom {
        return ScaleHit::Edge(ScaleEdges {
            left: near_left,
            right: near_right,
            top: near_top,
            bottom: near_bottom,
        });
    }
    ScaleHit::Inside
}

impl ScaleEdges {
    fn cursor(self) -> CursorStyle {
        match (self.left || self.right, self.top || self.bottom) {
            (true, true) if (self.left && self.top) || (self.right && self.bottom) => {
                CursorStyle::ResizeUpLeftDownRight
            }
            (true, true) => CursorStyle::ResizeUpRightDownLeft,
            (true, false) => CursorStyle::ResizeLeftRight,
            (false, true) => CursorStyle::ResizeUpDown,
            (false, false) => CursorStyle::Crosshair,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_target_keeps_the_opposite_edge_fixed() {
        let original = PixelRect::new(10, 20, 30, 40);
        let right = ScaleEdges {
            left: false,
            right: true,
            top: false,
            bottom: false,
        };
        assert_eq!(
            scale_target(original, right, (0, 0), (10, 0)),
            PixelRect::new(10, 20, 40, 40)
        );
        let corner = ScaleEdges {
            left: true,
            right: false,
            top: true,
            bottom: false,
        };
        assert_eq!(
            scale_target(original, corner, (0, 0), (10, 10)),
            PixelRect::new(20, 30, 20, 30)
        );
        assert_eq!(
            scale_target(original, right, (0, 0), (-100, 0)),
            PixelRect::new(10, 20, 1, 40)
        );
    }

    #[test]
    fn scale_hit_favors_edges_and_detects_the_inside() {
        assert_eq!(
            hit_scale(0., 0., 100., 80., 1., 1., 6.),
            ScaleHit::Edge(ScaleEdges {
                left: true,
                right: false,
                top: true,
                bottom: false,
            })
        );
        assert_eq!(hit_scale(0., 0., 100., 80., 50., 40., 6.), ScaleHit::Inside);
        assert_eq!(
            hit_scale(0., 0., 100., 80., 120., 40., 6.),
            ScaleHit::Outside
        );
    }
}
