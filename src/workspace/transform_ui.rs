use std::sync::Arc;

use gpui::{Context, Div, Pixels, Point, Window, div, point, prelude::*, px, rgb};
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
        .when(!self.active_position_locked(), |root| {
            root.on_action(cx.listener(|this, action: &LayerAdjust, window, cx| {
                this.adjust_layer(*action, window, cx);
            }))
        })
        .on_action(cx.listener(|this, _: &ScaleImage, window, cx| {
            this.scale_image_form(window, cx);
        }))
        .when(!self.active_position_locked(), |root| {
            root.on_action(cx.listener(|this, _: &ScaleLayer, window, cx| {
                this.scale_layer_form(window, cx);
            }))
        })
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
                    let antialias = previous.is_antialiased();
                    *selection = match previous {
                        Selection::Rect(rect) | Selection::Inverse(rect) => previous.moved_box(
                            transform::scale_rect(rect, old, content.size()),
                            content.size().0,
                            content.size().1,
                        ),
                        Selection::Ellipse(ellipse) => Selection::ellipse_rect_quality(
                            transform::scale_rect(ellipse.rect(), old, content.size()),
                            content.size().0,
                            content.size().1,
                            antialias,
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
        return Selection::ellipse_rect_quality(
            map_image_rect(ellipse.rect(), old, action),
            new.0,
            new.1,
            selection.is_antialiased(),
        );
    }
    if let Selection::RoundedRect(shape) = &selection {
        return Selection::rounded_rect_quality(
            map_image_rect(shape.rect(), old, action),
            shape.radius(),
            new.0,
            new.1,
            selection.is_antialiased(),
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
pub(super) const MAX_TRANSFORM_COORD: f64 = i32::MAX as f64 + document::MAX_NEW_EDGE as f64;

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
    Pivot {
        grab: (f64, f64),
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
    mask: Option<document::LayerMask>,
    offset: (i32, i32),
    opacity: u8,
    kind: WarpKind,
    degrees: f64,
    pivot: (f64, f64),
    mirror: Mirror,
    corners: [(f64, f64); 4],
    selected_corner: usize,
    drag: Option<WarpDrag>,
}

#[derive(Clone, Copy)]
pub(super) struct WarpFrame {
    pub outline: [(f64, f64); 4],
    pub axis: Option<[(f64, f64); 2]>,
    pub grips: [(f64, f64); 4],
    pub grip_count: usize,
    pub invalid: bool,
    pub pivot: Option<(f64, f64)>,
    pub active_grip: Option<usize>,
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
    pub(super) fn begin_layer_transform_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preferences.tool == Tool::ScaleLayer {
            self.cancel_scale(window, cx);
            self.begin_scale_numeric(window, cx);
        } else {
            self.begin_warp(window, cx);
        }
    }

    pub(super) fn begin_warp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = warp_kind(self.preferences.tool) else {
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
        if layer.locks.position {
            self.notice("Unlock the layer's position before transforming it.", cx);
            return;
        }
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
            mask: layer.mask.clone(),
            offset,
            opacity: layer.opacity,
            kind,
            degrees: 0.0,
            pivot: (
                f64::from(offset.0) + f64::from(size.0) / 2.,
                f64::from(offset.1) + f64::from(size.1) / 2.,
            ),
            mirror: Mirror::Horizontal,
            corners: transform::layer_corners(offset, size),
            selected_corner: 0,
            drag: None,
        });
        self.warp_error = None;
        self.request_warp_preview(window, cx);
    }

    pub(super) fn cancel_warp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .numeric_edit
            .as_ref()
            .is_some_and(|edit| edit.field.is_warp())
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
        self.commit_numeric_edit(window, cx);
        if !self.can_apply_warp() {
            return;
        }
        let Some(session) = self.warp.as_ref() else {
            return;
        };
        let warp = session.spec();
        let source = session.source.clone();
        let mask = session.mask.clone();
        let offset = session.offset;
        let layer = session.layer;
        let resample = self.preferences.warp_resample;
        self.warp_dirty = false;
        self.warp_applying = true;
        cx.notify();
        self.warp_token = self.warp_token.wrapping_add(1);
        let token = self.warp_token;
        let job = cx.background_executor().spawn(async move {
            let placed = transform::warp_layer(&source, offset, warp, resample, None)?;
            let mask = mask
                .map(|mask| {
                    transform::warp_layer(&mask.pixels, offset, warp, resample, None).map(
                        |placed| document::LayerMask {
                            pixels: Arc::new(placed.pixels),
                            enabled: mask.enabled,
                        },
                    )
                })
                .transpose()?;
            Ok::<_, WarpError>((placed, mask))
        });
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
                    Ok((placed, mask)) => workspace.commit_warp(layer, placed, mask, window, cx),
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

    pub(super) fn rotation_pivot(&self) -> Option<(f64, f64)> {
        self.warp
            .as_ref()
            .filter(|session| session.kind == WarpKind::Rotate)
            .map(|session| session.pivot)
    }

    pub(super) fn set_rotation_pivot(
        &mut self,
        pivot: (f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.warp_applying {
            return;
        }
        let Some(session) = self
            .warp
            .as_mut()
            .filter(|session| session.kind == WarpKind::Rotate && !session.dragging())
        else {
            return;
        };
        if !session.set_pivot(pivot) {
            return;
        }
        if session.changed() {
            self.request_warp_preview(window, cx);
        } else {
            cx.notify();
        }
    }

    pub(super) fn set_rotation_anchor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_numeric_edit(window, cx);
        let Some(pivot) = self
            .warp
            .as_ref()
            .and_then(|session| session.pivot_anchor(index))
        else {
            return;
        };
        self.set_rotation_pivot(pivot, window, cx);
    }

    pub(super) fn rotation_pivot_options(&self, cx: &Context<Self>) -> Div {
        use super::numeric::NumericField;
        use super::tooltip::tooltip;
        use super::view::{MUTED, TransformPointControl, transform_point_grid};
        let selected = self.warp.as_ref().and_then(|session| {
            (0..9).find(|index| session.pivot_anchor(*index) == Some(session.pivot))
        });
        let grid = transform_point_grid(TransformPointControl::Rotation, selected, cx);
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child("Pivot"),
            )
            .child(
                div().flex().gap_2().child(grid).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            self.numeric_chip(NumericField::RotatePivotX, Some("X"), cx)
                                .tooltip(tooltip("Pivot X in canvas pixels")),
                        )
                        .child(
                            self.numeric_chip(NumericField::RotatePivotY, Some("Y"), cx)
                                .tooltip(tooltip("Pivot Y in canvas pixels")),
                        ),
                ),
            )
    }

    pub(super) fn perspective_point(&self, index: u8) -> Option<(f64, f64)> {
        self.warp
            .as_ref()
            .filter(|session| session.kind == WarpKind::Perspective)
            .and_then(|session| session.corners.get(usize::from(index)).copied())
    }

    pub(super) fn set_perspective_point(
        &mut self,
        index: u8,
        point: (f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.warp_applying {
            return;
        }
        let Some(session) = self
            .warp
            .as_mut()
            .filter(|session| session.kind == WarpKind::Perspective && !session.dragging())
        else {
            return;
        };
        if session.set_corner(usize::from(index), point) {
            self.request_warp_preview(window, cx);
        }
    }

    pub(super) fn select_perspective_corner(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_numeric_edit(window, cx);
        if self.warp_applying || index >= 4 {
            return;
        }
        if let Some(session) = self
            .warp
            .as_mut()
            .filter(|session| session.kind == WarpKind::Perspective && !session.dragging())
        {
            session.selected_corner = index;
            cx.notify();
        }
    }

    fn reset_perspective(&mut self, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_numeric_edit(window, cx);
        if self.warp_applying {
            return;
        }
        let Some(session) = self
            .warp
            .as_mut()
            .filter(|session| session.kind == WarpKind::Perspective && !session.dragging())
        else {
            return;
        };
        if session.reset_corners(all) {
            self.request_warp_preview(window, cx);
        }
    }

    pub(super) fn perspective_corner_options(&self, cx: &Context<Self>) -> Div {
        use super::numeric::NumericField;
        use super::tooltip::tooltip;
        use super::view::{
            MUTED, TransformPointControl, action_row, choice_button, transform_point_grid,
        };
        let index = self
            .warp
            .as_ref()
            .map_or(0, |session| session.selected_corner);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child("Corner"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(transform_point_grid(
                        TransformPointControl::Perspective,
                        Some(index),
                        cx,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                self.numeric_chip(
                                    NumericField::PerspectiveX(index as u8),
                                    Some("X"),
                                    cx,
                                )
                                .tooltip(tooltip("Corner X in canvas pixels")),
                            )
                            .child(
                                self.numeric_chip(
                                    NumericField::PerspectiveY(index as u8),
                                    Some("Y"),
                                    cx,
                                )
                                .tooltip(tooltip("Corner Y in canvas pixels")),
                            ),
                    ),
            )
            .child(action_row([
                choice_button("Reset Corner", false).on_click(
                    cx.listener(|this, _, window, cx| this.reset_perspective(false, window, cx)),
                ),
                choice_button("Reset All", false).on_click(
                    cx.listener(|this, _, window, cx| this.reset_perspective(true, window, cx)),
                ),
            ]))
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
        if self.warp_applying || self.preferences.warp_resample == resample {
            return;
        }
        self.preferences.warp_resample = resample;
        if self.warp.is_some() {
            self.request_warp_preview(window, cx);
        }
        cx.notify();
    }

    pub(super) fn begin_warp_drag(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_numeric_edit(window, cx);
        if self.warp_applying {
            return;
        }
        if self.warp.is_none() {
            self.begin_warp(window, cx);
        }
        let Some(canvas) = self.canvas_point(position, scale) else {
            return;
        };
        let pivot_hit = self.rotation_pivot_hit(position, scale);
        let grip = self.grip_at(position, scale);
        let Some(session) = self.warp.as_mut() else {
            return;
        };
        if session.kind == WarpKind::Perspective
            && let Some(index) = grip
        {
            session.selected_corner = index;
        }
        session.drag = match session.kind {
            WarpKind::Rotate if pivot_hit => Some(WarpDrag::Pivot {
                grab: (session.pivot.0 - canvas.0, session.pivot.1 - canvas.1),
            }),
            WarpKind::Rotate => Some(WarpDrag::Rotate {
                start_degrees: session.degrees,
                start_angle: pointer_angle(session.pivot, canvas),
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
        shift: bool,
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
                    let degrees = drag_degrees(
                        start_degrees,
                        start_angle,
                        pointer_angle(session.pivot, canvas),
                        shift,
                    );
                    changed = degrees != session.degrees;
                    session.degrees = degrees;
                }
                Some(WarpDrag::Pivot { grab }) => {
                    changed = session.set_pivot((canvas.0 + grab.0, canvas.1 + grab.1))
                        && session.changed();
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
                    changed = session.set_corner(index, (canvas.0 + grab.0, canvas.1 + grab.1));
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
                let outline = session.rotation_outline();
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
                    pivot: Some(session.pivot),
                    active_grip: None,
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
                    pivot: None,
                    active_grip: None,
                }
            }
            WarpKind::Perspective => WarpFrame {
                outline: session.corners,
                axis: None,
                grips: session.corners,
                grip_count: 4,
                invalid: self.warp_error.is_some(),
                pivot: None,
                active_grip: Some(session.selected_corner),
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
            resample: self.preferences.warp_resample,
        })
    }

    fn commit_warp(
        &mut self,
        layer: LayerId,
        placed: transform::Placed,
        mask: Option<document::LayerMask>,
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
            move |content, _, _| content.place_layer_with_mask(layer, pixels, offset, mask),
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
        let mask = session.mask.clone().filter(|mask| mask.enabled);
        let offset = session.offset;
        let opacity = session.opacity;
        let layer = session.layer;
        let resample = self.preferences.warp_resample;
        let job = cx.background_executor().spawn(async move {
            let placed = transform::warp_layer(&source, offset, warp, resample, Some(PREVIEW_EDGE));
            let (below, above, sprite) = match placed {
                Ok(placed) => {
                    let mut shown = layers::with_layer_opacity(&placed.pixels, opacity);
                    if let Some(mask) = &mask {
                        match transform::warp_layer(
                            &mask.pixels,
                            offset,
                            warp,
                            resample,
                            Some(PREVIEW_EDGE),
                        ) {
                            Ok(transformed) => {
                                for (pixel, mask) in
                                    shown.pixels_mut().zip(transformed.pixels.pixels())
                                {
                                    pixel[3] = ((u16::from(pixel[3]) * u16::from(mask[3]) + 127)
                                        / 255) as u8;
                                }
                            }
                            Err(error) => {
                                return WarpPreview {
                                    below: None,
                                    above: None,
                                    sprite: Err(error),
                                };
                            }
                        }
                    }
                    let sprite = Ok(DrawnSprite {
                        image: document::preview_render(&shown),
                        offset: placed.offset,
                        size: placed.bounds,
                    });
                    let (below, above) = base
                        .as_ref()
                        .map(|content| content.preview_around(layer))
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

    fn rotation_pivot_hit(&self, position: Point<Pixels>, scale: f32) -> bool {
        let Some(pivot) = self.rotation_pivot() else {
            return false;
        };
        let Some(tab) = self.active_tab() else {
            return false;
        };
        let Some(geometry) = PreviewGeometry::for_tab(tab, self.viewport.get(), scale) else {
            return false;
        };
        let projected = geometry.project(point(
            (pivot.0 / f64::from(tab.document.width())) as f32,
            (pivot.1 / f64::from(tab.document.height())) as f32,
        ));
        f32::from(projected.x - position.x).hypot(f32::from(projected.y - position.y)) <= 8.
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
            WarpKind::Rotate if self.pivot == self.center() => Warp::Rotate(self.degrees),
            WarpKind::Rotate => Warp::RotateAround {
                degrees: self.degrees,
                pivot: self.pivot,
            },
            WarpKind::Flip => Warp::Flip(self.mirror),
            WarpKind::Perspective => Warp::Perspective(self.corners),
        }
    }

    fn set_pivot(&mut self, pivot: (f64, f64)) -> bool {
        if !pivot.0.is_finite()
            || !pivot.1.is_finite()
            || pivot.0.abs() > MAX_TRANSFORM_COORD
            || pivot.1.abs() > MAX_TRANSFORM_COORD
        {
            return false;
        }
        let pivot = ((pivot.0 * 10.).round() / 10., (pivot.1 * 10.).round() / 10.);
        if self.pivot == pivot {
            return false;
        }
        self.pivot = pivot;
        true
    }

    fn set_corner(&mut self, index: usize, point: (f64, f64)) -> bool {
        if !point.0.is_finite()
            || !point.1.is_finite()
            || point.0.abs() > MAX_TRANSFORM_COORD
            || point.1.abs() > MAX_TRANSFORM_COORD
        {
            return false;
        }
        let Some(corner) = self.corners.get_mut(index) else {
            return false;
        };
        let point = ((point.0 * 10.).round() / 10., (point.1 * 10.).round() / 10.);
        if *corner == point {
            return false;
        }
        *corner = point;
        true
    }

    fn reset_corners(&mut self, all: bool) -> bool {
        let original = transform::layer_corners(self.offset, self.source.dimensions());
        let before = self.corners;
        if all {
            self.corners = original;
        } else {
            self.corners[self.selected_corner] = original[self.selected_corner];
        }
        self.corners != before
    }

    fn pivot_anchor(&self, index: usize) -> Option<(f64, f64)> {
        (index < 9).then(|| {
            let (width, height) = self.source.dimensions();
            (
                f64::from(self.offset.0) + f64::from(width) * (index % 3) as f64 / 2.,
                f64::from(self.offset.1) + f64::from(height) * (index / 3) as f64 / 2.,
            )
        })
    }

    fn rotation_outline(&self) -> [(f64, f64); 4] {
        if self.pivot == self.center() {
            transform::placed_rotation_outline(self.offset, self.source.dimensions(), self.degrees)
        } else {
            transform::rotate_corners_about(
                transform::layer_corners(self.offset, self.source.dimensions()),
                self.degrees,
                self.pivot,
            )
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

fn drag_degrees(start: f64, pointer_start: f64, pointer: f64, constrained: bool) -> f64 {
    let degrees = wrap_degrees(start + pointer - pointer_start);
    let step = if constrained { 15.0 } else { 0.1 };
    wrap_degrees((degrees / step).round() * step)
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
    fn perspective_corners_share_exact_tenths_and_recover_invalid_geometry() {
        let mut session = WarpSession {
            layer: LayerId(7),
            mask: None,
            source: Arc::new(RgbaImage::new(5, 2)),
            offset: (-3, 4),
            opacity: 180,
            kind: WarpKind::Perspective,
            degrees: 0.,
            pivot: (-0.5, 5.),
            mirror: Mirror::Horizontal,
            corners: transform::layer_corners((-3, 4), (5, 2)),
            selected_corner: 2,
            drag: None,
        };
        let source = session.source.clone();
        assert!(!session.changed()); // Corner selection alone is not an image edit.
        assert!(session.set_corner(0, (-3.56, 4.34)));
        assert_eq!(session.corners[0], (-3.6, 4.3));
        assert!(!session.set_corner(0, (-3.6, 4.3)));
        let before = session.corners;
        for point in [
            (f64::NAN, 0.),
            (0., f64::INFINITY),
            (MAX_TRANSFORM_COORD + 1., 0.),
        ] {
            assert!(!session.set_corner(0, point));
            assert_eq!(session.corners, before);
        }
        assert!(!session.set_corner(4, (0., 0.)));
        assert!(session.set_corner(2, (-20., 30.)));
        assert!(
            transform::warp_bounds(session.offset, session.source.dimensions(), session.spec())
                .is_err()
        );
        assert!(session.reset_corners(false));
        assert_eq!(session.corners[0], before[0]);
        assert!(session.changed());
        assert!(
            transform::warp_bounds(session.offset, session.source.dimensions(), session.spec())
                .is_ok()
        );
        assert!(session.reset_corners(true));
        assert!(!session.changed());
        assert!(!session.reset_corners(true));
        assert!(Arc::ptr_eq(&source, &session.source));
    }

    #[test]
    fn pivot_anchors_precision_and_default_center_leave_source_and_pending_state_intact() {
        let mut session = WarpSession {
            layer: LayerId(7),
            mask: None,
            source: Arc::new(RgbaImage::new(5, 2)),
            offset: (-3, 4),
            opacity: 180,
            kind: WarpKind::Rotate,
            degrees: 0.,
            pivot: (-0.5, 5.),
            mirror: Mirror::Horizontal,
            corners: transform::layer_corners((-3, 4), (5, 2)),
            selected_corner: 0,
            drag: None,
        };
        let pixels = session.source.clone();
        assert_eq!(session.spec(), Warp::Rotate(0.));
        let expected = [
            (-3., 4.),
            (-0.5, 4.),
            (2., 4.),
            (-3., 5.),
            (-0.5, 5.),
            (2., 5.),
            (-3., 6.),
            (-0.5, 6.),
            (2., 6.),
        ];
        for (index, point) in expected.into_iter().enumerate() {
            assert_eq!(session.pivot_anchor(index), Some(point));
            session.set_pivot(point);
            assert!(!session.changed());
            assert!(Arc::ptr_eq(&pixels, &session.source));
            assert_eq!(session.offset, (-3, 4));
        }
        assert_eq!(session.pivot_anchor(9), None);
        assert!(session.set_pivot((1.234, -4.567)));
        assert_eq!(session.pivot, (1.2, -4.6));
        assert!(!session.set_pivot((1.2, -4.6)));
        for bad in [
            (f64::NAN, 0.),
            (0., f64::INFINITY),
            (MAX_TRANSFORM_COORD + 1., 0.),
        ] {
            assert!(!session.set_pivot(bad));
            assert_eq!(session.pivot, (1.2, -4.6));
        }
        session.set_pivot(session.center());
        session.degrees = 90.;
        assert_eq!(session.spec(), Warp::Rotate(90.));
        let placed =
            transform::warp_bounds(session.offset, session.source.dimensions(), session.spec())
                .unwrap();
        let outline = session.rotation_outline();
        let left = outline
            .iter()
            .map(|point| point.0)
            .fold(f64::INFINITY, f64::min);
        let top = outline
            .iter()
            .map(|point| point.1)
            .fold(f64::INFINITY, f64::min);
        assert!((left - f64::from(placed.offset.0)).abs() < 1e-9);
        assert!((top - f64::from(placed.offset.1)).abs() < 1e-9);
        session.set_pivot(expected[0]);
        assert_eq!(
            session.spec(),
            Warp::RotateAround {
                degrees: 90.,
                pivot: expected[0]
            }
        );
        assert_eq!(session.rotation_outline()[0], expected[0]);
    }

    #[test]
    fn applying_precise_transforms_is_one_undo_step_and_keeps_canvas_selection() {
        use super::super::{Change, Tab, history::State};
        fn publish(tab: &mut Tab, state: State, change: Change) {
            let patch = document::RenderedCanvas::prepare_patch(
                &tab.document.canvas.pixels,
                tab.document.canvas.preview.has_transparency,
                tab.document.canvas.level_caches(),
                &state.content,
                Dirty::Canvas,
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
        for (spec, expected_offset) in [
            (
                Warp::RotateAround {
                    degrees: 90.,
                    pivot: (0., 0.),
                },
                (-2, 0),
            ),
            (
                Warp::Perspective([(-1., -1.), (2., 0.), (2., 2.), (0., 2.)]),
                (-1, -1),
            ),
        ] {
            let mut tab = super::super::tests::opacity_tab();
            tab.selection = Selection::Rect(PixelRect::new(0, 0, 1, 2));
            let selection = tab.selection.clone();
            let layer = tab.active_layer;
            let before = tab.document.content.layer(layer).unwrap().pixels.clone();
            let placed =
                transform::warp_layer(&before, (0, 0), spec, Resample::Bicubic, None).unwrap();
            assert_eq!(placed.offset, expected_offset);
            assert!(!tab.history.is_dirty());
            let mut state = tab.state();
            assert_ne!(
                state
                    .content
                    .place_layer(layer, placed.pixels, placed.offset),
                Dirty::None
            );
            publish(&mut tab, state, Change::Edit);
            assert!(tab.history.is_dirty());
            assert_eq!(tab.selection, selection);
            assert_eq!(
                tab.document.content.layer(layer).unwrap().offset,
                expected_offset
            );
            let previous = tab.history.undo_target().unwrap().0.clone();
            publish(&mut tab, previous, Change::Undo);
            assert!(!tab.history.is_dirty());
            assert!(tab.history.undo_target().is_none());
            assert_eq!(tab.document.content.layer(layer).unwrap().offset, (0, 0));
            assert!(Arc::ptr_eq(
                &before,
                &tab.document.content.layer(layer).unwrap().pixels
            ));
            let next = tab.history.redo_target().unwrap().0.clone();
            publish(&mut tab, next, Change::Redo);
            assert_eq!(
                tab.document.content.layer(layer).unwrap().offset,
                expected_offset
            );
            assert_eq!(tab.selection, selection);
        }
    }

    #[test]
    fn image_turns_and_flips_keep_antialiased_selection_coverage() {
        for selection in [
            Selection::ellipse_rect_quality(
                crate::preview::PixelRect::new(1, 1, 6, 4),
                10,
                8,
                true,
            ),
            Selection::rounded_rect_quality(
                crate::preview::PixelRect::new(1, 1, 6, 4),
                2,
                10,
                8,
                true,
            ),
        ] {
            for action in [
                ImageAdjust::RotateRight,
                ImageAdjust::FlipHorizontal,
                ImageAdjust::FlipVertical,
            ] {
                let new = if action == ImageAdjust::RotateRight {
                    (8, 10)
                } else {
                    (10, 8)
                };
                let transformed = map_image_selection(selection.clone(), (10, 8), new, action);
                assert!(transformed.is_antialiased());
                for y in 0..8 {
                    for x in 0..10 {
                        let point = match action {
                            ImageAdjust::RotateRight => (7 - y, x),
                            ImageAdjust::FlipHorizontal => (9 - x, y),
                            _ => (x, 7 - y),
                        };
                        assert_eq!(
                            transformed.coverage(point.0, point.1, new.0, new.1),
                            selection.coverage(x, y, 10, 8)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn constrained_rotation_snaps_absolute_angles_and_survives_the_angle_seam() {
        assert_eq!(drag_degrees(0., 0., 22., true), 15.);
        assert_eq!(drag_degrees(0., 0., -24., true), -30.);
        assert_eq!(drag_degrees(13., 350., 5., true), 30.);
        assert_eq!(drag_degrees(178., 0., 4., true).abs(), 180.);
        assert!((drag_degrees(0., 0., 22.34, false) - 22.3).abs() < 1e-9);
        // Releasing Shift recomputes from the original grab, without accumulating snaps.
        assert_eq!(drag_degrees(13., 350., 5., false), 28.);
    }

    #[test]
    fn untouched_rotate_and_perspective_are_not_pending_but_a_flip_is() {
        let mut session = WarpSession {
            layer: LayerId(0),
            mask: None,
            source: Arc::new(RgbaImage::new(3, 2)),
            offset: (1, 2),
            opacity: 255,
            kind: WarpKind::Rotate,
            degrees: 0.0,
            pivot: (2.5, 3.),
            mirror: Mirror::Horizontal,
            corners: transform::layer_corners((1, 2), (3, 2)),
            selected_corner: 0,
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
