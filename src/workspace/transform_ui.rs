use gpui::{Context, Div, Window, prelude::*};

use super::Workspace;
use crate::commands::{
    CanvasSize, ImageAdjust, LayerAdjust, ScaleImage, ScaleLayer, SetGrayscale, SetRgb,
};
use crate::preview::Dirty;
use crate::selection::Selection;
use crate::transform::{self, ColorMode, Mirror, Turn};

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
        if !self.transform_enabled() {
            return;
        }
        self.edit_content(
            move |content, _, selection| {
                let old = content.size();
                let previous = *selection;
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
        let Some(id) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        if !self.transform_enabled() {
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
        if !self.transform_enabled() {
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
                    let previous = *selection;
                    if !content.scale_image(width, height, resample) {
                        return Dirty::None;
                    }
                    *selection = match previous {
                        Selection::Rect(rect) | Selection::Inverse(rect) => previous.moved_box(
                            transform::scale_rect(rect, old, content.size()),
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
                let previous = *selection;
                let Some(delta) = content.resize_canvas(width, height, anchor) else {
                    return Dirty::None;
                };
                let source = match previous {
                    Selection::Full => crate::preview::PixelRect::new(0, 0, old.0, old.1),
                    Selection::Rect(rect) | Selection::Inverse(rect) => rect,
                    other => {
                        *selection = other;
                        return Dirty::Canvas;
                    }
                };
                let mapped = transform::translate_rect(source, delta);
                let kind = if matches!(previous, Selection::Inverse(_)) {
                    previous
                } else {
                    Selection::Rect(mapped)
                };
                *selection = kind.moved_box(mapped, content.size().0, content.size().1);
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
    let (Selection::Rect(rect) | Selection::Inverse(rect)) = selection else {
        return selection;
    };
    let mapped = match action {
        ImageAdjust::RotateLeft => transform::turn_rect(rect, old, Turn::Left),
        ImageAdjust::RotateRight => transform::turn_rect(rect, old, Turn::Right),
        ImageAdjust::RotateHalf => transform::turn_rect(rect, old, Turn::Half),
        ImageAdjust::FlipHorizontal => transform::mirror_rect(rect, old, Mirror::Horizontal),
        ImageAdjust::FlipVertical => transform::mirror_rect(rect, old, Mirror::Vertical),
    };
    selection.moved_box(mapped, new.0, new.1)
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
