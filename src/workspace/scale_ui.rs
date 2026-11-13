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
    pointer: bool,
    anchor: crate::transform::Anchor,
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
            if layer.locks.position {
                return None;
            }
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tool != Tool::ScaleLayer
            || self.scale_drag.is_some()
            || self.active_tab().is_none_or(|tab| tab.busy())
        {
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        let layer = match tab.document.content.layer(tab.active_layer) {
            Some(layer) => layer,
            None => return,
        };
        if layer.locks.position {
            self.notice("Unlock the layer's position before scaling it.", cx);
            return;
        }
        if !layer.visible {
            self.notice("The active layer is hidden. Show it before scaling.", cx);
            return;
        }
        if layer.text.is_some() {
            self.notice("Rasterize the text layer before scaling it.", cx);
            return;
        }
        let id = layer.id;
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
        self.start_scale_preview(
            ScaleDrag {
                layer: id,
                original,
                edges,
                start,
                target: original,
                pointer: true,
                anchor: self.scale_anchor,
            },
            window,
            cx,
        );
    }

    pub(super) fn begin_scale_numeric(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.tool != Tool::ScaleLayer || self.scale_drag.is_some() {
            return false;
        }
        let Some(tab) = self.active_tab().filter(|tab| !tab.busy()) else {
            return false;
        };
        let Some(layer) = tab.document.content.layer(tab.active_layer) else {
            return false;
        };
        if layer.locks.position {
            self.notice("Unlock the layer's position before scaling it.", cx);
            return false;
        }
        if !layer.visible || layer.text.is_some() {
            self.notice(
                "Show the layer and rasterize editable text before scaling.",
                cx,
            );
            return false;
        }
        let original = PixelRect::new(
            i64::from(layer.offset.0),
            i64::from(layer.offset.1),
            layer.pixels.width(),
            layer.pixels.height(),
        );
        let drag = ScaleDrag {
            layer: layer.id,
            original,
            target: original,
            start: (0, 0),
            edges: ScaleEdges {
                left: false,
                right: false,
                top: false,
                bottom: false,
            },
            pointer: false,
            anchor: self.scale_anchor,
        };
        self.start_scale_preview(drag, window, cx);
        true
    }

    fn start_scale_preview(
        &mut self,
        drag: ScaleDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let Some(layer) = tab.document.content.layer(drag.layer) else {
            return;
        };
        let id = drag.layer;
        let original = drag.original;
        let content = tab.document.content.clone();
        let pixels = layer.pixels.clone();
        let opacity = layer.opacity;
        self.scale_token = self.scale_token.wrapping_add(1);
        let token = self.scale_token;
        self.scale_drag = Some(drag);
        let job = cx.background_executor().spawn(async move {
            let (below, above) = content.preview_around(id);
            MovePaint {
                below: below.as_ref().map(document::preview_render),
                above: above.as_ref().map(document::preview_render),
                sprite: document::preview_render(&super::layers::with_layer_opacity(
                    &pixels, opacity,
                )),
                offset: (original.x as i32, original.y as i32),
                size: (original.width, original.height),
            }
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let mut overlay = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                if workspace.scale_token != token || workspace.scale_drag.is_none() {
                    super::drop_images(overlay.into_images(), window, cx);
                    return;
                }
                let target = workspace.scale_drag.unwrap().target;
                overlay.offset = (target.x as i32, target.y as i32);
                overlay.size = (target.width, target.height);
                workspace.scale_overlay = Some(overlay);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn extend_scale(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.scale_drag.filter(|drag| drag.pointer) else {
            return;
        };
        let Some(pointer) = self.unclamped_pointer_pixel(position, scale) else {
            return;
        };
        let target = scale_target(
            drag.original,
            drag.edges,
            drag.start,
            pointer,
            self.scale_lock ^ shift,
        );
        self.update_scale_target(target, cx);
    }

    pub(super) fn scale_pointer_dragging(&self) -> bool {
        self.scale_drag.is_some_and(|drag| drag.pointer)
    }

    pub(super) fn preview_scale_size(&mut self, width: bool, value: u32, cx: &mut Context<Self>) {
        let Some(drag) = self.scale_drag.filter(|drag| !drag.pointer) else {
            return;
        };
        let Some((w, h)) = scale_dimensions(
            (drag.original.width, drag.original.height),
            width,
            value,
            self.scale_lock,
        ) else {
            self.restore_scale_size(cx);
            self.notice(
                "Enter a size within the layer dimension and memory limits.",
                cx,
            );
            return;
        };
        let Some(offset) = crate::transform::anchored_offset(
            (drag.original.x as i32, drag.original.y as i32),
            (drag.original.width, drag.original.height),
            (w, h),
            drag.anchor,
        ) else {
            self.restore_scale_size(cx);
            self.notice(
                "That anchored position exceeds the layer coordinate limits.",
                cx,
            );
            return;
        };
        if !self.update_scale_target(
            PixelRect::new(i64::from(offset.0), i64::from(offset.1), w, h),
            cx,
        ) {
            self.restore_scale_size(cx);
        }
    }

    pub(super) fn set_scale_anchor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_numeric_edit(window, cx);
        let Some(anchor) = crate::transform::Anchor::from_grid(index) else {
            return;
        };
        if self.scale_drag.is_none() && self.scale_anchor != anchor {
            self.scale_anchor = anchor;
            cx.notify();
        }
    }

    pub(super) fn restore_scale_size(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.scale_drag {
            self.update_scale_target(drag.original, cx);
        }
    }

    fn update_scale_target(&mut self, target: PixelRect, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.scale_drag else {
            return false;
        };
        if target == drag.target {
            return true;
        }
        if self.active_tab().is_none_or(|tab| {
            !crate::transform::canvas_allowed(target.width, target.height)
                || !tab
                    .document
                    .content
                    .accepts_replacement(drag.layer, (target.width, target.height))
        }) {
            self.notice("That size exceeds the layer or image memory limit.", cx);
            return false;
        }
        if let Some(drag) = self.scale_drag.as_mut() {
            drag.target = target;
        }
        if let Some(overlay) = self.scale_overlay.as_mut() {
            overlay.offset = (target.x as i32, target.y as i32);
            overlay.size = (target.width, target.height);
        }
        cx.notify();
        true
    }

    pub(super) fn finish_scale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.scale_drag.take() else {
            return;
        };
        self.scale_token = self.scale_token.wrapping_add(1);
        let overlay = self.scale_overlay.take();
        if drag.target == drag.original {
            if let Some(overlay) = overlay {
                super::drop_images(overlay.into_images(), window, cx);
            }
            cx.notify();
            return;
        }
        let resample = self.warp_resample;
        self.edit_content(
            move |content, _, _| commit_scale(content, drag, resample),
            window,
            cx,
        );
        if let Some(overlay) = overlay {
            super::drop_images(overlay.into_images(), window, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_scale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .numeric_edit
            .as_ref()
            .is_some_and(|edit| edit.field.is_scale())
        {
            self.numeric_edit = None;
        }
        self.scale_token = self.scale_token.wrapping_add(1);
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
        if let Some(drag) = self.scale_drag.filter(|drag| drag.pointer) {
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

fn commit_scale(
    content: &mut crate::document::DocumentContent,
    drag: ScaleDrag,
    resample: crate::transform::Resample,
) -> Dirty {
    if drag.target == drag.original
        || i32::try_from(drag.target.x).is_err()
        || i32::try_from(drag.target.y).is_err()
        || !content.scale_layer(drag.layer, drag.target.width, drag.target.height, resample)
    {
        return Dirty::None;
    }
    let _ = content.set_offset(drag.layer, (drag.target.x as i32, drag.target.y as i32));
    Dirty::Canvas
}

fn scale_target(
    original: PixelRect,
    edges: ScaleEdges,
    start: (i64, i64),
    pointer: (i64, i64),
    locked: bool,
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
    let mut width = right.saturating_sub(left).clamp(1, i64::from(MAX_NEW_EDGE)) as u32;
    let mut height = bottom.saturating_sub(top).clamp(1, i64::from(MAX_NEW_EDGE)) as u32;
    if locked {
        let horizontal = edges.left || edges.right;
        let vertical = edges.top || edges.bottom;
        let width_axis = horizontal
            && (!vertical
                || (width as f64 / original.width as f64 - 1.).abs()
                    >= (height as f64 / original.height as f64 - 1.).abs());
        // Clamp the common scale, rather than either dimension independently.
        let factor = if width_axis {
            width as f64 / original.width as f64
        } else {
            height as f64 / original.height as f64
        };
        let factor = factor.min(MAX_NEW_EDGE as f64 / original.width.max(original.height) as f64);
        width = (original.width as f64 * factor).round().max(1.) as u32;
        height = (original.height as f64 * factor).round().max(1.) as u32;
        if !horizontal {
            left = original.x + (i64::from(original.width) - i64::from(width)) / 2;
        }
        if !vertical {
            top = original.y + (i64::from(original.height) - i64::from(height)) / 2;
        }
    }
    if edges.left {
        left = right - i64::from(width);
    }
    if edges.top {
        top = bottom - i64::from(height);
    }
    PixelRect::new(
        left.clamp(i64::from(i32::MIN), i64::from(i32::MAX)),
        top.clamp(i64::from(i32::MIN), i64::from(i32::MAX)),
        width,
        height,
    )
}

fn scale_dimensions(
    original: (u32, u32),
    width_axis: bool,
    value: u32,
    locked: bool,
) -> Option<(u32, u32)> {
    if value == 0 || value > MAX_NEW_EDGE {
        return None;
    }
    let (width, height) = if locked {
        let factor = value as f64 / if width_axis { original.0 } else { original.1 } as f64;
        (
            (original.0 as f64 * factor).round().max(1.) as u32,
            (original.1 as f64 * factor).round().max(1.) as u32,
        )
    } else if width_axis {
        (value, original.1)
    } else {
        (original.0, value)
    };
    (width <= MAX_NEW_EDGE
        && height <= MAX_NEW_EDGE
        && crate::transform::canvas_allowed(width, height))
    .then_some((width, height))
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
    fn fit_layer_from_off_canvas_is_one_undo_step_with_stationary_selection() {
        use super::super::{Change, Tab, history::State};
        fn publish(tab: &mut Tab, state: State, dirty: Dirty, change: Change) {
            let patch = crate::document::RenderedCanvas::prepare_patch(
                &tab.document.canvas.pixels,
                tab.document.canvas.preview.has_transparency,
                tab.document.canvas.level_caches(),
                &state.content,
                dirty,
            )
            .unwrap();
            assert!(
                tab.publish_change(
                    tab.history.generation(),
                    change,
                    Some((state.content, state.active_layer, state.selection, patch)),
                    1
                )
                .0
            );
        }
        let mut tab = super::super::tests::opacity_tab();
        let id = tab.active_layer;
        let mut content = tab.document.content.clone();
        content.place_layer(
            id,
            image::RgbaImage::from_pixel(4, 2, image::Rgba([30, 80, 160, 128])),
            (-100, -100),
        );
        tab.document = crate::document::Document::from_content("Fit".into(), content);
        tab.selection = crate::selection::Selection::Rect(PixelRect::new(0, 0, 1, 2));
        let original = tab.document.content.layer(id).unwrap().pixels.clone();
        let selection = tab.selection.clone();
        let mut state = tab.state();
        let dirty = state
            .content
            .fit_layer_to_canvas(id, crate::transform::Resample::Bilinear);
        assert_ne!(dirty, Dirty::None);
        publish(&mut tab, state, dirty, Change::Edit);
        assert_eq!(tab.selection, selection);
        assert!(tab.history.is_dirty());
        assert_eq!(
            tab.document.content.layer(id).unwrap().pixels.dimensions(),
            (2, 1)
        );
        assert_eq!(tab.document.content.layer(id).unwrap().offset, (0, 0));
        assert_eq!(
            tab.document
                .content
                .fit_layer_to_canvas(id, crate::transform::Resample::Nearest),
            Dirty::None
        );
        let undo = tab.history.undo_target().unwrap().0.clone();
        publish(&mut tab, undo, Dirty::Canvas, Change::Undo);
        assert!(!tab.history.is_dirty());
        assert!(tab.history.undo_target().is_none());
        assert_eq!(tab.selection, selection);
        assert_eq!(tab.document.content.layer(id).unwrap().offset, (-100, -100));
        assert!(std::sync::Arc::ptr_eq(
            &original,
            &tab.document.content.layer(id).unwrap().pixels
        ));
        let redo = tab.history.redo_target().unwrap().0.clone();
        publish(&mut tab, redo, Dirty::Canvas, Change::Redo);
        assert!(tab.history.is_dirty());
        assert_eq!(tab.selection, selection);
        assert_eq!(
            tab.document.content.layer(id).unwrap().pixels.dimensions(),
            (2, 1)
        );
    }

    #[test]
    fn numeric_anchor_commit_rejects_unplaceable_targets_before_resampling() {
        let tab = super::super::tests::opacity_tab();
        let mut state = tab.state();
        let before = state
            .content
            .layer(state.active_layer)
            .unwrap()
            .pixels
            .clone();
        let mut drag = ScaleDrag {
            layer: state.active_layer,
            original: PixelRect::new(0, 0, 2, 2),
            target: PixelRect::new(i64::from(i32::MAX) + 1, 0, 4, 4),
            edges: ScaleEdges {
                left: false,
                right: false,
                top: false,
                bottom: false,
            },
            start: (0, 0),
            pointer: false,
            anchor: crate::transform::Anchor::SouthEast,
        };
        assert!(matches!(
            commit_scale(
                &mut state.content,
                drag,
                crate::transform::Resample::Bicubic
            ),
            Dirty::None
        ));
        let layer = state.content.layer(state.active_layer).unwrap();
        assert!(std::sync::Arc::ptr_eq(&before, &layer.pixels));
        assert_eq!(layer.offset, (0, 0));
        let offset =
            crate::transform::anchored_offset((0, 0), (2, 2), (4, 4), drag.anchor).unwrap();
        drag.target = PixelRect::new(i64::from(offset.0), i64::from(offset.1), 4, 4);
        assert!(matches!(
            commit_scale(
                &mut state.content,
                drag,
                crate::transform::Resample::Nearest
            ),
            Dirty::Canvas
        ));
        let layer = state.content.layer(state.active_layer).unwrap();
        assert_eq!(layer.offset, (-2, -2));
        assert_eq!(layer.pixels.dimensions(), (4, 4));
    }

    #[test]
    fn numeric_scale_commits_pixels_and_placement_as_one_undo_step() {
        let mut tab = super::super::tests::opacity_tab();
        let generation = tab.history.generation();
        let mut state = tab.state();
        let original = PixelRect::new(0, 0, 2, 2);
        let mut drag = ScaleDrag {
            layer: state.active_layer,
            original,
            target: original,
            edges: ScaleEdges {
                left: false,
                right: false,
                top: false,
                bottom: false,
            },
            start: (0, 0),
            pointer: false,
            anchor: crate::transform::Anchor::Center,
        };
        assert!(matches!(
            commit_scale(
                &mut state.content,
                drag,
                crate::transform::Resample::Nearest
            ),
            Dirty::None
        ));
        drag.target = PixelRect::new(-1, -1, 4, 4);
        let dirty = commit_scale(
            &mut state.content,
            drag,
            crate::transform::Resample::Nearest,
        );
        assert!(matches!(dirty, Dirty::Canvas));
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            dirty,
        )
        .unwrap();
        tab.editing = true;
        assert!(
            tab.publish_change(
                generation,
                super::super::Change::Edit,
                Some((state.content, state.active_layer, state.selection, patch)),
                1
            )
            .0
        );
        let layer = tab.document.content.layer(tab.active_layer).unwrap();
        assert_eq!(layer.pixels.dimensions(), (4, 4));
        assert_eq!(layer.offset, (-1, -1));
        assert_eq!(layer.opacity, 255);
        assert_eq!(tab.selection, crate::selection::Selection::Unrestricted);
        assert!(tab.history.is_dirty());
        assert_eq!(tab.history.generation(), generation + 1);
        let (undo, _) = tab.history.undo_target().unwrap();
        let layer = undo.content.layer(tab.active_layer).unwrap();
        assert_eq!(layer.pixels.dimensions(), (2, 2));
        assert_eq!(layer.offset, (0, 0));
    }

    #[test]
    fn locked_scale_anchors_corners_and_centers_the_other_axis_for_edges() {
        let original = PixelRect::new(-10, 20, 100, 50);
        let right = ScaleEdges {
            left: false,
            right: true,
            top: false,
            bottom: false,
        };
        assert_eq!(
            scale_target(original, right, (0, 0), (100, 0), true),
            PixelRect::new(-10, -5, 200, 100)
        );
        let corner = ScaleEdges {
            left: true,
            right: false,
            top: true,
            bottom: false,
        };
        let target = scale_target(original, corner, (0, 0), (-100, -10), true);
        assert_eq!(target, PixelRect::new(-110, -30, 200, 100));
        assert_eq!(target.right(), original.right());
        assert_eq!(target.bottom(), original.bottom());
        let limited = scale_target(original, corner, (0, 0), (-100_000, -100_000), true);
        assert_eq!(
            (limited.width, limited.height),
            (MAX_NEW_EDGE, MAX_NEW_EDGE / 2)
        );
        assert_eq!(limited.right(), original.right());
        assert_eq!(limited.bottom(), original.bottom());
    }

    #[test]
    fn numeric_scale_preserves_ratio_rounding_and_size_limits() {
        assert_eq!(
            scale_dimensions((100, 50), true, 200, true),
            Some((200, 100))
        );
        assert_eq!(
            scale_dimensions((100, 50), false, 200, true),
            Some((400, 200))
        );
        assert_eq!(
            scale_dimensions((100, 50), true, 200, false),
            Some((200, 50))
        );
        assert_eq!(scale_dimensions((3, 2), true, 10, true), Some((10, 7)));
        assert_eq!(scale_dimensions((100, 1), false, MAX_NEW_EDGE, true), None);
        assert_eq!(scale_dimensions((1, 1), true, MAX_NEW_EDGE, true), None);
        assert_eq!(scale_dimensions((100, 50), true, 0, true), None);
        assert_eq!(
            scale_dimensions((100, 50), true, MAX_NEW_EDGE + 1, false),
            None
        );
    }

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
            scale_target(original, right, (0, 0), (10, 0), false),
            PixelRect::new(10, 20, 40, 40)
        );
        let corner = ScaleEdges {
            left: true,
            right: false,
            top: true,
            bottom: false,
        };
        assert_eq!(
            scale_target(original, corner, (0, 0), (10, 10), false),
            PixelRect::new(20, 30, 20, 30)
        );
        assert_eq!(
            scale_target(original, right, (0, 0), (-100, 0), false),
            PixelRect::new(10, 20, 1, 40)
        );
        let limited = scale_target(original, corner, (0, 0), (-100_000, -100_000), false);
        assert_eq!(limited.width, MAX_NEW_EDGE);
        assert_eq!(limited.height, MAX_NEW_EDGE);
        assert_eq!(limited.right(), original.right());
        assert_eq!(limited.bottom(), original.bottom());
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
