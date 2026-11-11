use std::sync::Arc;

use gpui::{Context, Pixels, Point, RenderImage, Window};
use image::RgbaImage;

use super::Workspace;
use super::layers::{MovePaint, with_layer_opacity};
use crate::document::{self, DocumentContent, FillMode, LayerId};
use crate::paint::{Gradient, GradientBlend, GradientShape};
use crate::selection::Selection;
use crate::transform::ColorMode;

pub(super) struct GradientFrame {
    pub start: (f64, f64),
    pub end: (f64, f64),
}

pub(super) struct GradientSession {
    layer: LayerId,
    source: Arc<RgbaImage>,
    offset: (i32, i32),
    opacity: u8,
    start: (f64, f64),
    end: (f64, f64),
    drag: Option<GradientDrag>,
}

impl GradientSession {
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

enum GradientDrag {
    New,
    Handle { index: usize, grab: (f64, f64) },
}

struct PreparedGradient {
    base: Option<DocumentContent>,
    source: Arc<RgbaImage>,
    offset: (i32, i32),
    opacity: u8,
    layer: LayerId,
    gradient: Gradient,
    selection: Selection,
    canvas: (u32, u32),
    mode: ColorMode,
}

struct GradientPreview {
    below: Option<Arc<RenderImage>>,
    above: Option<Arc<RenderImage>>,
    sprite: Arc<RenderImage>,
    offset: (i32, i32),
    size: (u32, u32),
}

impl Workspace {
    pub(super) fn begin_gradient(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let Some(point) = self.canvas_point(position, scale) else {
            return;
        };
        if let Some(index) = self.gradient_handle_at(position, scale) {
            if let Some(session) = self.gradient.as_mut() {
                let anchor = if index == 0 {
                    session.start
                } else {
                    session.end
                };
                session.drag = Some(GradientDrag::Handle {
                    index,
                    grab: (anchor.0 - point.0, anchor.1 - point.1),
                });
            }
            cx.notify();
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        let Some(layer) = tab.document.content.layer(tab.active_layer) else {
            return;
        };
        if !layer.visible {
            self.notice("The active layer is hidden. Show it before filling.", cx);
            return;
        }
        let session = GradientSession {
            layer: layer.id,
            source: layer.pixels.clone(),
            offset: layer.offset,
            opacity: layer.opacity,
            start: point,
            end: point,
            drag: Some(GradientDrag::New),
        };
        self.gradient = Some(session);
        self.request_gradient_preview(window, cx);
    }

    pub(super) fn extend_gradient(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .gradient
            .as_ref()
            .is_none_or(|session| session.drag.is_none())
        {
            return;
        }
        let Some(point) = self.canvas_point(position, scale) else {
            return;
        };
        let mut changed = false;
        if let Some(session) = self.gradient.as_mut() {
            match session.drag {
                Some(GradientDrag::Handle { index, grab }) => {
                    let point = (point.0 + grab.0, point.1 + grab.1);
                    let anchor = if index == 0 {
                        &mut session.start
                    } else {
                        &mut session.end
                    };
                    changed = *anchor != point;
                    *anchor = point;
                }
                Some(GradientDrag::New) => {
                    changed = session.end != point;
                    session.end = point;
                }
                None => {}
            }
        }
        if changed {
            self.request_gradient_preview(window, cx);
        }
    }

    pub(super) fn end_gradient_drag(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.gradient.as_mut() else {
            return;
        };
        if session.drag.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn cancel_gradient(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.gradient_token = self.gradient_token.wrapping_add(1);
        self.gradient_dirty = false;
        self.gradient = None;
        self.drop_gradient_overlay(window, cx);
    }

    pub(super) fn can_apply_gradient(&self) -> bool {
        self.active_tab().is_some_and(|tab| !tab.busy())
            && self
                .gradient
                .as_ref()
                .is_some_and(|session| session.drag.is_none())
    }

    pub(super) fn apply_gradient(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_apply_gradient() {
            return;
        }
        let Some(gradient) = self.gradient_spec() else {
            return;
        };
        let Some(layer) = self.gradient.as_ref().map(|session| session.layer) else {
            return;
        };
        self.edit_content(
            move |content, _, selection| content.apply_gradient(layer, gradient, selection),
            window,
            cx,
        );
    }

    pub(super) fn refresh_gradient(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gradient.is_some() {
            self.request_gradient_preview(window, cx);
        }
    }

    pub(super) fn set_gradient_shape(
        &mut self,
        shape: GradientShape,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.gradient_shape == shape {
            return;
        }
        self.gradient_shape = shape;
        self.refresh_gradient(window, cx);
        cx.notify();
    }

    pub(super) fn set_gradient_blend(
        &mut self,
        blend: GradientBlend,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.gradient_blend == blend {
            return;
        }
        self.gradient_blend = blend;
        self.refresh_gradient(window, cx);
        cx.notify();
    }

    pub(super) fn set_fill_mode(&mut self, mode: FillMode, cx: &mut Context<Self>) {
        if self.fill_mode != mode {
            self.fill_mode = mode;
            cx.notify();
        }
    }

    pub(super) fn set_sample_all(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.sample_all != enabled {
            self.sample_all = enabled;
            cx.notify();
        }
    }

    pub(super) fn gradient_frame(&self) -> Option<GradientFrame> {
        let session = self.gradient.as_ref()?;
        Some(GradientFrame {
            start: session.start,
            end: session.end,
        })
    }

    fn gradient_spec(&self) -> Option<Gradient> {
        let session = self.gradient.as_ref()?;
        Some(Gradient {
            shape: self.gradient_shape,
            blend: self.gradient_blend,
            start: session.start,
            end: session.end,
            foreground: document::solid_color(self.colors[0]),
            background: document::solid_color(self.colors[1]),
        })
    }

    fn prepare_gradient_preview(&self) -> Option<PreparedGradient> {
        let session = self.gradient.as_ref()?;
        let tab = self.active_tab()?;
        Some(PreparedGradient {
            base: self
                .gradient_overlay
                .is_none()
                .then(|| tab.document.content.clone()),
            source: session.source.clone(),
            offset: session.offset,
            opacity: session.opacity,
            layer: session.layer,
            gradient: Gradient {
                shape: self.gradient_shape,
                blend: self.gradient_blend,
                start: session.start,
                end: session.end,
                foreground: document::solid_color(self.colors[0]),
                background: document::solid_color(self.colors[1]),
            },
            selection: tab.selection.clone(),
            canvas: tab.document.content.size(),
            mode: tab.document.content.mode(),
        })
    }

    fn request_gradient_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gradient.is_none() {
            return;
        }
        self.gradient_token = self.gradient_token.wrapping_add(1);
        self.gradient_dirty = true;
        self.pump_gradient_preview(window, cx);
        cx.notify();
    }

    fn pump_gradient_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gradient_busy || !self.gradient_dirty || self.gradient.is_none() {
            return;
        }
        let Some(prepared) = self.prepare_gradient_preview() else {
            return;
        };
        self.gradient_dirty = false;
        self.gradient_busy = true;
        let token = self.gradient_token;
        let job = cx
            .background_executor()
            .spawn(async move { preview_gradient(prepared) });
        cx.spawn_in(window, async move |workspace, cx| {
            let preview = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.gradient_busy = false;
                let current = workspace.gradient.is_some() && workspace.gradient_token == token;
                if current {
                    workspace.publish_gradient_preview(preview, window, cx);
                } else {
                    drop_gradient_preview(preview, window, cx);
                }
                if workspace.gradient_dirty {
                    workspace.pump_gradient_preview(window, cx);
                }
            });
        })
        .detach();
    }

    fn publish_gradient_preview(
        &mut self,
        preview: GradientPreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let GradientPreview {
            below,
            above,
            sprite,
            offset,
            size,
        } = preview;
        if let Some(overlay) = self.gradient_overlay.as_mut() {
            let previous = std::mem::replace(&mut overlay.sprite, sprite);
            overlay.offset = offset;
            overlay.size = size;
            super::drop_images(std::iter::once(previous), window, cx);
            super::drop_images(below.into_iter().chain(above), window, cx);
        } else {
            self.gradient_overlay = Some(MovePaint {
                below,
                sprite,
                above,
                offset,
                size,
            });
        }
        cx.notify();
    }

    fn drop_gradient_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(overlay) = self.gradient_overlay.take() {
            super::drop_images(overlay.into_images(), window, cx);
        }
    }

    fn gradient_handle_at(&self, position: Point<Pixels>, scale: f32) -> Option<usize> {
        let tab = self.active_tab()?;
        let geometry =
            super::canvas_tools::PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let frame = self.gradient_frame()?;
        let width = f64::from(tab.document.width());
        let height = f64::from(tab.document.height());
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let project = |grip: (f64, f64)| {
            geometry.project(gpui::point(
                (grip.0 / width) as f32,
                (grip.1 / height) as f32,
            ))
        };
        nearest_gradient_handle([project(frame.start), project(frame.end)], position)
    }
}

fn nearest_gradient_handle(grips: [Point<Pixels>; 2], position: Point<Pixels>) -> Option<usize> {
    grips
        .into_iter()
        .enumerate()
        .filter_map(|(index, grip)| {
            let delta = grip - position;
            let distance = f32::from(delta.x).powi(2) + f32::from(delta.y).powi(2);
            (distance <= 64.0).then_some((index, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(index, _)| index)
}

fn constrain(mut gradient: Gradient, mode: ColorMode) -> Gradient {
    gradient.foreground = mode.constrain(gradient.foreground);
    gradient.background = mode.constrain(gradient.background);
    gradient
}

fn preview_gradient(prepared: PreparedGradient) -> GradientPreview {
    let PreparedGradient {
        base,
        source,
        offset,
        opacity,
        layer,
        gradient,
        selection,
        canvas,
        mode,
    } = prepared;
    let gradient = constrain(gradient, mode);
    let rendered = document::gradient_preview(
        &source,
        offset,
        gradient,
        &selection,
        canvas,
        crate::adjustments::PREVIEW_EDGE,
    );
    let shown = with_layer_opacity(&rendered, opacity);
    let (below, above) = base
        .as_ref()
        .map(|content| content.composite_around(layer))
        .unwrap_or((None, None));
    GradientPreview {
        below: below.as_ref().map(document::preview_render),
        above: above.as_ref().map(document::preview_render),
        sprite: document::preview_render(&shown),
        offset,
        size: source.dimensions(),
    }
}

fn drop_gradient_preview(
    preview: GradientPreview,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    super::drop_images(
        preview
            .below
            .into_iter()
            .chain(preview.above)
            .chain(std::iter::once(preview.sprite)),
        window,
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_gradient_handles_can_extend_a_zero_length_gradient() {
        let start = gpui::point(gpui::px(10.), gpui::px(10.));
        assert_eq!(nearest_gradient_handle([start, start], start), Some(1));
        let end = gpui::point(gpui::px(20.), gpui::px(10.));
        assert_eq!(nearest_gradient_handle([start, end], start), Some(0));
        assert_eq!(nearest_gradient_handle([start, end], end), Some(1));
    }
}
