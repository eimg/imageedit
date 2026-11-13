use std::sync::Arc;

use gpui::{Context, Div, Pixels, Point, Window, point, prelude::*};
use image::RgbaImage;

use super::Workspace;
use super::canvas_tools::PreviewGeometry;
use super::layers::{self, MovePaint};
use crate::commands::{
    CanvasSize, ImageAdjust, LayerAdjust, ScaleImage, ScaleLayer, SetGrayscale, SetRgb, Tool,
};
use crate::document::{self, LayerId};
use crate::preview::{Dirty, PixelRect};
use crate::selection::Selection;
use crate::transform::{self, ColorMode, Mirror, Resample, Turn, Warp, WarpError};

impl Workspace {
    pub(super) fn bind_transform_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        if !self.transform_enabled() {
            return root;
        }
        root.on_action(cx.listener(|this, action: &ImageAdjust, window, cx| {
            this.adjust_image(*action, window, cx);
        }))
        .on_action(cx.listener(|this, action: &LayerAdjust, window, cx| {
            this.adjust_layer(*action, window, cx);
        }))
        .on_action(cx.listener(|this, _: &ScaleImage, window, cx| {
            this.scale_image_form(window, cx);
        }))
        .on_action(cx.listener(|this, _: &ScaleLayer, window, cx| {
            this.scale_layer_form(window, cx);
        }))
        .on_action(
            cx.listener(|this, _: &CanvasSize, window, cx| this.canvas_size_form(window, cx)),
        )
        .on_action(cx.listener(|this, _: &SetRgb, window, cx| {
            this.set_color_mode(ColorMode::Rgb, window, cx);
        }))
        .on_action(cx.listener(|this, _: &SetGrayscale, window, cx| {
            this.set_color_mode(ColorMode::Grayscale, window, cx);
        }))
    }

    fn transform_enabled(&self) -> bool {
        self.move_drag.is_none() && self.active_tab().is_some_and(|tab| !tab.busy())
    }

    fn adjust_image(&mut self, action: ImageAdjust, window: &mut Window, cx: &mut Context<Self>) {
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| this.adjust_image(action, window, cx));
            return;
        }
        if !self.transform_enabled() || self.refuse_text_pixels(true, cx) {
            return;
        }
        self.edit_content(
            move |content, _, selection| {
                let old = content.size();
                let previous = selection.clone();
                let turned = matches!(
                    action,
                    ImageAdjust::RotateLeft | ImageAdjust::RotateRight | ImageAdjust::RotateHalf
                );
                let ok = if turned {
                    content.turn_image(image_turn(action))
                } else {
                    content.mirror_image(image_mirror(action))
                };
                if !ok {
                    return Dirty::None;
                }
                *selection = map_image_selection(previous, old, content.size(), action);
                Dirty::Canvas
            },
            window,
            cx,
        );
    }

    fn adjust_layer(&mut self, action: LayerAdjust, window: &mut Window, cx: &mut Context<Self>) {
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| this.adjust_layer(action, window, cx));
            return;
        }
        let Some(id) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        if !self.transform_enabled() || self.refuse_text_pixels(false, cx) {
            return;
        }
        self.edit_content(
            move |content, _, _| {
                let ok = if matches!(
                    action,
                    LayerAdjust::RotateLeft | LayerAdjust::RotateRight | LayerAdjust::RotateHalf
                ) {
                    content.turn_layer(id, layer_turn(action))
                } else {
                    content.mirror_layer(id, layer_mirror(action))
                };
                if ok { Dirty::Canvas } else { Dirty::None }
            },
            window,
            cx,
        );
    }

    pub(super) fn apply_scale(
        &mut self,
        width: u32,
        height: u32,
        resample: transform::Resample,
        layer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        if !self.transform_enabled() || self.refuse_text_pixels(!layer, cx) {
            return;
        }
        self.edit_content(
            move |content, _, selection| {
                if layer {
                    if content.scale_layer(id, width, height, resample) {
                        Dirty::Canvas
                    } else {
                        Dirty::None
                    }
                } else {
                    let old = content.size();
                    let previous = selection.clone();
                    if !content.scale_image(width, height, resample) {
                        return Dirty::None;
                    }
                    if matches!(previous, Selection::Mask(_) | Selection::RoundedRect(_)) {
                        *selection = previous.scale_mask(old, content.size());
                        return Dirty::Canvas;
                    }
                    *selection = match previous {
                        Selection::Rect(rect) | Selection::Inverse(rect) => previous.moved_box(
                            transform::scale_rect(rect, old, content.size()),
                            content.size().0,
                            content.size().1,
                        ),
                        Selection::Ellipse(ellipse) => Selection::ellipse_rect(
                            transform::scale_rect(ellipse.rect(), old, content.size()),
                            content.size().0,
                            content.size().1,
                        ),
                        other => other,
                    };
                    Dirty::Canvas
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn apply_canvas_size(
        &mut self,
        width: u32,
        height: u32,
        anchor: transform::Anchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.transform_enabled() {
            return;
        }
        self.edit_content(
            move |content, _, selection| {
                let old = content.size();
                let Some(delta) = content.resize_canvas(width, height, anchor) else {
                    return Dirty::None;
                };
                *selection = selection.clone().resize_canvas(old, content.size(), delta);
                Dirty::Canvas
            },
            window,
            cx,
        );
    }

    fn set_color_mode(&mut self, mode: ColorMode, window: &mut Window, cx: &mut Context<Self>) {
        if !self.transform_enabled() {
            return;
        }
        self.edit_content(
            move |content, _, _| content.set_color_mode(mode),
            window,
            cx,
        );
    }
}

fn map_image_selection(
    selection: Selection,
    old: (u32, u32),
    new: (u32, u32),
    action: ImageAdjust,
) -> Selection {
    if matches!(selection, Selection::Mask(_)) {
        let (width, height) = (old.0 as i32, old.1 as i32);
        return match action {
            ImageAdjust::RotateLeft => {
                selection.map_mask(new.0, new.1, move |x, y| (y, width - 1 - x))
            }
            ImageAdjust::RotateRight => {
                selection.map_mask(new.0, new.1, move |x, y| (height - 1 - y, x))
            }
            ImageAdjust::RotateHalf => {
                selection.map_mask(new.0, new.1, move |x, y| (width - 1 - x, height - 1 - y))
            }
            ImageAdjust::FlipHorizontal => {
                selection.map_mask(new.0, new.1, move |x, y| (width - 1 - x, y))
            }
            ImageAdjust::FlipVertical => {
                selection.map_mask(new.0, new.1, move |x, y| (x, height - 1 - y))
            }
        };
    }
    if let Selection::Ellipse(ellipse) = &selection {
        return Selection::ellipse_rect(map_image_rect(ellipse.rect(), old, action), new.0, new.1);
    }
    if let Selection::RoundedRect(shape) = &selection {
        return Selection::rounded_rect_rect(
            map_image_rect(shape.rect(), old, action),
            shape.radius(),
            new.0,
            new.1,
        );
    }
    let (Selection::Rect(rect) | Selection::Inverse(rect)) = selection else {
        return selection;
    };
    selection.moved_box(map_image_rect(rect, old, action), new.0, new.1)
}

fn map_image_rect(rect: PixelRect, old: (u32, u32), action: ImageAdjust) -> PixelRect {
    match action {
        ImageAdjust::RotateLeft => transform::turn_rect(rect, old, Turn::Left),
        ImageAdjust::RotateRight => transform::turn_rect(rect, old, Turn::Right),
        ImageAdjust::RotateHalf => transform::turn_rect(rect, old, Turn::Half),
        ImageAdjust::FlipHorizontal => transform::mirror_rect(rect, old, Mirror::Horizontal),
        ImageAdjust::FlipVertical => transform::mirror_rect(rect, old, Mirror::Vertical),
    }
}

fn image_turn(action: ImageAdjust) -> Turn {
    match action {
        ImageAdjust::RotateLeft => Turn::Left,
        ImageAdjust::RotateRight => Turn::Right,
        _ => Turn::Half,
    }
}

fn image_mirror(action: ImageAdjust) -> Mirror {
    match action {
        ImageAdjust::FlipVertical => Mirror::Vertical,
        _ => Mirror::Horizontal,
    }
}

fn layer_turn(action: LayerAdjust) -> Turn {
    match action {
        LayerAdjust::RotateLeft => Turn::Left,
        LayerAdjust::RotateRight => Turn::Right,
        _ => Turn::Half,
    }
}

fn layer_mirror(action: LayerAdjust) -> Mirror {
    match action {
        LayerAdjust::FlipVertical => Mirror::Vertical,
        _ => Mirror::Horizontal,
    }
}

const PREVIEW_EDGE: u32 = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WarpKind {
    Rotate,
    Flip,
    Perspective,
}

enum WarpDrag {
    Rotate {
        start_degrees: f64,
        start_angle: f64,
    },
    Flip {
        origin: Point<Pixels>,
    },
    Corner {
        index: usize,
        grab: (f64, f64),
    },
}

pub(super) struct WarpSession {
    layer: LayerId,
    source: Arc<RgbaImage>,
    offset: (i32, i32),
    opacity: u8,
    kind: WarpKind,
    degrees: f64,
    mirror: Mirror,
    corners: [(f64, f64); 4],
    drag: Option<WarpDrag>,
}

#[derive(Clone, Copy)]
pub(super) struct WarpFrame {
    pub outline: [(f64, f64); 4],
    pub axis: Option<[(f64, f64); 2]>,
    pub grips: [(f64, f64); 4],
    pub grip_count: usize,
    pub invalid: bool,
}

pub(super) struct WarpStatus {
    pub degrees: Option<f64>,
    pub error: Option<&'static str>,
    pub horizontal: bool,
    pub applying: bool,
    pub resample: Resample,
}

struct DrawnSprite {
    image: Arc<gpui::RenderImage>,
    offset: (i32, i32),
    size: (u32, u32),
}

struct WarpPreview {
    below: Option<Arc<gpui::RenderImage>>,
    above: Option<Arc<gpui::RenderImage>>,
    sprite: Result<DrawnSprite, WarpError>,
}

impl Workspace {
    pub(super) fn begin_warp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = warp_kind(self.tool) else {
            return;
        };
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        self.cancel_warp(window, cx);
        let Some(tab) = self.active_tab() else {
            return;
        };
        let Some(layer) = tab.document.content.layer(tab.active_layer) else {
            return;
        };
        if layer.text.is_some() {
            self.notice("Rasterize the text layer before using pixel tools.", cx);
            return;
        }
        if !layer.visible {
            self.notice(
                "The active layer is hidden. Show it before transforming.",
                cx,
            );
            return;
        }
        let offset = layer.offset;
        let size = layer.pixels.dimensions();
        self.warp = Some(WarpSession {
            layer: layer.id,
            source: layer.pixels.clone(),
            offset,
            opacity: layer.opacity,
            kind,
            degrees: 0.0,
            mirror: Mirror::Horizontal,
            corners: transform::layer_corners(offset, size),
            drag: None,
        });
        self.warp_error = None;
        self.request_warp_preview(window, cx);
    }

    pub(super) fn cancel_warp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .numeric_edit
            .as_ref()
            .is_some_and(|edit| edit.field == super::numeric::NumericField::RotateAngle)
        {
            self.numeric_edit = None;
        }
        self.warp_token = self.warp_token.wrapping_add(1);
        self.warp_dirty = false;
        self.warp_applying = false;
        self.warp = None;
        self.warp_error = None;
        self.drop_warp_overlay(window, cx);
    }

    pub(super) fn can_apply_warp(&self) -> bool {
        !self.warp_applying
            && self.warp_error.is_none()
            && self.active_tab().is_some_and(|tab| !tab.busy())
            && self
                .warp
                .as_ref()
                .is_some_and(|session| !session.dragging())
    }

    pub(super) fn apply_warp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_apply_warp() {
            return;
        }
        let Some(session) = self.warp.as_ref() else {
            return;
        };
        let warp = session.spec();
        let source = session.source.clone();
        let offset = session.offset;
        let layer = session.layer;
        let resample = self.warp_resample;
        self.warp_dirty = false;
        self.warp_applying = true;
        cx.notify();
        self.warp_token = self.warp_token.wrapping_add(1);
        let token = self.warp_token;
        let job = cx
            .background_executor()
            .spawn(async move { transform::warp_layer(&source, offset, warp, resample, None) });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                if workspace.warp_token != token {
                    return;
                }
                workspace.warp_applying = false;
                match result {
                    Err(error) => {
                        workspace.warp_error = Some(error.message());
                        cx.notify();
                    }
                    Ok(placed) => workspace.commit_warp(layer, placed, window, cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn set_warp_angle(
        &mut self,
        degrees: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.warp_applying || !degrees.is_finite() {
            return;
        }
        let Some(session) = self.warp.as_mut() else {
            return;
        };
        if session.kind != WarpKind::Rotate {
            return;
        }
        let degrees = wrap_degrees(degrees);
        if session.degrees == degrees {
            return;
        }
        session.degrees = degrees;
        self.warp_error = None;
        self.request_warp_preview(window, cx);
    }

    pub(super) fn set_warp_mirror(
        &mut self,
        mirror: Mirror,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.warp_applying {
            return;
        }
        let Some(session) = self.warp.as_mut() else {
            return;
        };
        if session.kind != WarpKind::Flip || session.mirror == mirror {
            return;
        }
        session.mirror = mirror;
        self.warp_error = None;
        self.request_warp_preview(window, cx);
    }

    pub(super) fn set_warp_resample(
        &mut self,
        resample: Resample,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.warp_applying || self.warp_resample == resample {
            return;
        }
        self.warp_resample = resample;
        if self.warp.is_some() {
            self.request_warp_preview(window, cx);
        }
    }

    pub(super) fn begin_warp_drag(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.warp_applying {
            return;
        }
        if self.warp.is_none() {
            self.begin_warp(window, cx);
        }
        let Some(canvas) = self.canvas_point(position, scale) else {
            return;
        };
        let grip = self.grip_at(position, scale);
        let Some(session) = self.warp.as_mut() else {
            return;
        };
        session.drag = match session.kind {
            WarpKind::Rotate => Some(WarpDrag::Rotate {
                start_degrees: session.degrees,
                start_angle: pointer_angle(session.center(), canvas),
            }),
            WarpKind::Flip => Some(WarpDrag::Flip { origin: position }),
            WarpKind::Perspective => grip.map(|index| WarpDrag::Corner {
                index,
                grab: (
                    session.corners[index].0 - canvas.0,
                    session.corners[index].1 - canvas.1,
                ),
            }),
        };
        cx.notify();
    }

    pub(super) fn extend_warp_drag(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .warp
            .as_ref()
            .is_none_or(|session| session.drag.is_none())
        {
            return;
        }
        let Some(canvas) = self.canvas_point(position, scale) else {
            return;
        };
        let mut changed = false;
        if let Some(session) = self.warp.as_mut() {
            match session.drag {
                Some(WarpDrag::Rotate {
                    start_degrees,
                    start_angle,
                }) => {
                    let degrees = wrap_degrees(
                        start_degrees + pointer_angle(session.center(), canvas) - start_angle,
                    );
                    let degrees = (degrees * 10.0).round() / 10.0;
                    changed = degrees != session.degrees;
                    session.degrees = degrees;
                }
                Some(WarpDrag::Flip { origin }) => {
                    let dx = f32::from(position.x - origin.x).abs();
                    let dy = f32::from(position.y - origin.y).abs();
                    if dx.max(dy) >= 3.0 {
                        let mirror = if dx >= dy {
                            Mirror::Horizontal
                        } else {
                            Mirror::Vertical
                        };
                        changed = session.mirror != mirror;
                        session.mirror = mirror;
                    }
                }
                Some(WarpDrag::Corner { index, grab }) => {
                    let corner = (canvas.0 + grab.0, canvas.1 + grab.1);
                    changed = session.corners[index] != corner;
                    session.corners[index] = corner;
                }
                None => {}
            }
        }
        if changed {
            self.warp_error = None;
            self.request_warp_preview(window, cx);
        }
        cx.notify();
    }

    pub(super) fn end_warp_drag(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.warp.as_mut() else {
            return;
        };
        if session.drag.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn warp_frame(&self) -> Option<WarpFrame> {
        let session = self.warp.as_ref()?;
        let size = session.source.dimensions();
        let source = transform::layer_corners(session.offset, size);
        let center = session.center();
        Some(match session.kind {
            WarpKind::Rotate => {
                let outline =
                    transform::placed_rotation_outline(session.offset, size, session.degrees);
                let mid = (
                    (outline[0].0 + outline[1].0) / 2.0,
                    (outline[0].1 + outline[1].1) / 2.0,
                );
                let grip = mid;
                WarpFrame {
                    outline,
                    axis: None,
                    grips: [grip, grip, grip, grip],
                    grip_count: 1,
                    invalid: self.warp_error.is_some(),
                }
            }
            WarpKind::Flip => {
                let axis = match session.mirror {
                    Mirror::Horizontal => [(center.0, source[0].1), (center.0, source[2].1)],
                    Mirror::Vertical => [(source[0].0, center.1), (source[1].0, center.1)],
                };
                WarpFrame {
                    outline: source,
                    axis: Some(axis),
                    grips: source,
                    grip_count: 4,
                    invalid: self.warp_error.is_some(),
                }
            }
            WarpKind::Perspective => WarpFrame {
                outline: session.corners,
                axis: None,
                grips: session.corners,
                grip_count: 4,
                invalid: self.warp_error.is_some(),
            },
        })
    }

    pub(super) fn warp_status(&self) -> Option<WarpStatus> {
        let session = self.warp.as_ref()?;
        Some(WarpStatus {
            degrees: (session.kind == WarpKind::Rotate).then_some(session.degrees),
            error: self.warp_error,
            horizontal: session.mirror == Mirror::Horizontal,
            applying: self.warp_applying,
            resample: self.warp_resample,
        })
    }

    fn commit_warp(
        &mut self,
        layer: LayerId,
        placed: transform::Placed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            self.notice(
                "Finish the current operation before applying the transform.",
                cx,
            );
            return;
        }
        let pixels = placed.pixels;
        let offset = placed.offset;
        self.edit_content(
            move |content, _, _| content.place_layer(layer, pixels, offset),
            window,
            cx,
        );
    }

    fn request_warp_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.warp.as_ref() else {
            return;
        };
        let result =
            transform::warp_bounds(session.offset, session.source.dimensions(), session.spec())
                .and_then(|bounds| {
                    self.active_document()
                        .is_some_and(|doc| {
                            doc.content.accepts_replacement(session.layer, bounds.size)
                        })
                        .then_some(())
                        .ok_or(WarpError::TooLarge)
                });
        self.warp_error = result.err().map(WarpError::message);
        self.warp_dirty = result.is_ok();
        if self.warp_error.is_some() {
            self.warp_token = self.warp_token.wrapping_add(1);
        }
        self.pump_warp_preview(window, cx);
        cx.notify();
    }

    fn pump_warp_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.warp_busy || self.warp_applying || !self.warp_dirty || self.warp.is_none() {
            return;
        }
        let base = if self.warp_overlay.is_none() {
            self.active_tab().map(|tab| tab.document.content.clone())
        } else {
            None
        };
        let Some(session) = self.warp.as_ref() else {
            return;
        };
        self.warp_dirty = false;
        self.warp_busy = true;
        self.warp_token = self.warp_token.wrapping_add(1);
        let token = self.warp_token;
        let warp = session.spec();
        let source = session.source.clone();
        let offset = session.offset;
        let opacity = session.opacity;
        let layer = session.layer;
        let resample = self.warp_resample;
        let job = cx.background_executor().spawn(async move {
            let placed = transform::warp_layer(&source, offset, warp, resample, Some(PREVIEW_EDGE));
            let (below, above, sprite) = match placed {
                Ok(placed) => {
                    let shown = layers::with_layer_opacity(&placed.pixels, opacity);
                    let sprite = Ok(DrawnSprite {
                        image: document::preview_render(&shown),
                        offset: placed.offset,
                        size: placed.bounds,
                    });
                    let (below, above) = base
                        .as_ref()
                        .map(|content| content.composite_around(layer))
                        .unwrap_or((None, None));
                    (
                        below.as_ref().map(document::preview_render),
                        above.as_ref().map(document::preview_render),
                        sprite,
                    )
                }
                Err(error) => (None, None, Err(error)),
            };
            WarpPreview {
                below,
                above,
                sprite,
            }
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let preview = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.warp_busy = false;
                let current = workspace.warp.is_some()
                    && workspace.warp_token == token
                    && (!workspace.warp_dirty || workspace.warp_overlay.is_none());
                if !current {
                    drop_preview(preview, window, cx);
                } else {
                    workspace.publish_warp_preview(preview, window, cx);
                }
                if workspace.warp_dirty {
                    workspace.pump_warp_preview(window, cx);
                }
            });
        })
        .detach();
    }

    fn publish_warp_preview(
        &mut self,
        preview: WarpPreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let WarpPreview {
            below,
            above,
            sprite,
        } = preview;
        match sprite {
            Ok(DrawnSprite {
                image: sprite,
                offset,
                size,
            }) => {
                self.warp_error = None;
                if let Some(overlay) = self.warp_overlay.as_mut() {
                    let previous = std::mem::replace(&mut overlay.sprite, sprite);
                    overlay.offset = offset;
                    overlay.size = size;
                    super::drop_images(std::iter::once(previous), window, cx);
                    super::drop_images(below.into_iter().chain(above), window, cx);
                } else {
                    self.warp_overlay = Some(MovePaint {
                        below,
                        sprite,
                        above,
                        offset,
                        size,
                    });
                }
            }
            Err(error) => {
                self.warp_error = Some(error.message());
                super::drop_images(below.into_iter().chain(above), window, cx);
            }
        }
        cx.notify();
    }

    fn drop_warp_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(overlay) = self.warp_overlay.take() {
            super::drop_images(overlay.into_images(), window, cx);
        }
    }

    pub(super) fn canvas_point(&self, position: Point<Pixels>, scale: f32) -> Option<(f64, f64)> {
        let tab = self.active_tab()?;
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let fraction = geometry.fraction(position);
        Some((
            f64::from(fraction.x) * f64::from(tab.document.width()),
            f64::from(fraction.y) * f64::from(tab.document.height()),
        ))
    }

    fn grip_at(&self, position: Point<Pixels>, scale: f32) -> Option<usize> {
        let tab = self.active_tab()?;
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let frame = self.warp_frame()?;
        let width = f64::from(tab.document.width());
        let height = f64::from(tab.document.height());
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        frame.grips.iter().take(frame.grip_count).position(|grip| {
            let projected =
                geometry.project(point((grip.0 / width) as f32, (grip.1 / height) as f32));
            let dx = f32::from(projected.x - position.x);
            let dy = f32::from(projected.y - position.y);
            dx * dx + dy * dy <= 64.0
        })
    }
}

impl WarpSession {
    pub(super) fn changed(&self) -> bool {
        match self.kind {
            WarpKind::Rotate => self.degrees.rem_euclid(360.0).abs() > 1e-9,
            WarpKind::Flip => true,
            WarpKind::Perspective => {
                self.corners != transform::layer_corners(self.offset, self.source.dimensions())
            }
        }
    }

    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn spec(&self) -> Warp {
        match self.kind {
            WarpKind::Rotate => Warp::Rotate(self.degrees),
            WarpKind::Flip => Warp::Flip(self.mirror),
            WarpKind::Perspective => Warp::Perspective(self.corners),
        }
    }

    fn center(&self) -> (f64, f64) {
        let (width, height) = self.source.dimensions();
        (
            f64::from(self.offset.0) + f64::from(width) / 2.0,
            f64::from(self.offset.1) + f64::from(height) / 2.0,
        )
    }
}

fn warp_kind(tool: Tool) -> Option<WarpKind> {
    match tool {
        Tool::Rotate => Some(WarpKind::Rotate),
        Tool::Flip => Some(WarpKind::Flip),
        Tool::Perspective => Some(WarpKind::Perspective),
        _ => None,
    }
}

fn pointer_angle(center: (f64, f64), point: (f64, f64)) -> f64 {
    (point.1 - center.1).atan2(point.0 - center.0).to_degrees()
}

fn wrap_degrees(mut value: f64) -> f64 {
    value %= 360.0;
    if value > 180.0 {
        value -= 360.0;
    }
    if value <= -180.0 {
        value += 360.0;
    }
    value
}

fn drop_preview(preview: WarpPreview, window: &mut Window, cx: &mut Context<Workspace>) {
    let mut images = Vec::new();
    images.extend(preview.below);
    images.extend(preview.above);
    if let Ok(sprite) = preview.sprite {
        images.push(sprite.image);
    }
    super::drop_images(images, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_rotate_and_perspective_are_not_pending_but_a_flip_is() {
        let mut session = WarpSession {
            layer: LayerId(0),
            source: Arc::new(RgbaImage::new(3, 2)),
            offset: (1, 2),
            opacity: 255,
            kind: WarpKind::Rotate,
            degrees: 0.0,
            mirror: Mirror::Horizontal,
            corners: transform::layer_corners((1, 2), (3, 2)),
            drag: None,
        };
        assert!(!session.changed());
        session.degrees = 360.0;
        assert!(!session.changed());
        session.degrees = 10.0;
        assert!(session.changed());
        session.kind = WarpKind::Perspective;
        assert!(!session.changed());
        session.corners[0].0 += 0.5;
        assert!(session.changed());
        session.kind = WarpKind::Flip;
        assert!(session.changed());
    }
}
