use std::sync::Arc;

use gpui::{Context, Div, Window, prelude::*};
use image::RgbaImage;

use super::Workspace;
use crate::adjustments::{self, Adjustment, PREVIEW_EDGE};
use crate::commands::{
    BrightnessContrast, ColorBalance, Curves, Desaturate, HueSaturation, InvertColors, Levels,
};
use crate::document::{self, LayerId};
use crate::selection::Selection;
use crate::transform::ColorMode;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AdjustKind {
    BrightnessContrast,
    HueSaturation,
    Desaturate,
    Invert,
    ColorBalance,
    Levels,
    Curves,
}

pub(super) struct AdjustmentSession {
    tab: usize,
    generation: u64,
    layer: LayerId,
    source: Arc<RgbaImage>,
    offset: (i32, i32),
    selection: Selection,
    canvas: (u32, u32),
    mode: ColorMode,
    adjustment: Adjustment,
}

impl Workspace {
    pub(super) fn bind_adjustment_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        if self.active_tab().is_none_or(|tab| {
            tab.busy()
                || tab
                    .document
                    .content
                    .layer(tab.active_layer)
                    .is_none_or(|layer| !layer.visible)
        }) {
            return root;
        }
        root.on_action(cx.listener(|this, _: &BrightnessContrast, window, cx| {
            this.adjustment_form(AdjustKind::BrightnessContrast, window, cx);
        }))
        .on_action(cx.listener(|this, _: &HueSaturation, window, cx| {
            this.adjustment_form(AdjustKind::HueSaturation, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Desaturate, window, cx| {
            this.adjustment_form(AdjustKind::Desaturate, window, cx);
        }))
        .on_action(cx.listener(|this, _: &InvertColors, window, cx| {
            this.adjustment_form(AdjustKind::Invert, window, cx);
        }))
        .on_action(cx.listener(|this, _: &ColorBalance, window, cx| {
            this.adjustment_form(AdjustKind::ColorBalance, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Levels, window, cx| {
            this.adjustment_form(AdjustKind::Levels, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Curves, window, cx| {
            this.adjustment_form(AdjustKind::Curves, window, cx);
        }))
    }

    pub(super) fn begin_adjustment(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.refuse_pending_text(cx) {
            return false;
        }
        if self.refuse_text_pixels(false, cx) {
            return false;
        }
        let Some(tab) = self.active_tab() else {
            return false;
        };
        if tab.busy() {
            return false;
        }
        let Some(layer) = tab.document.content.layer(tab.active_layer) else {
            return false;
        };
        if !layer.visible {
            self.notice("The active layer is hidden. Show it before adjusting.", cx);
            return false;
        }
        self.adjustment = Some(AdjustmentSession {
            tab: tab.id,
            generation: tab.history.generation(),
            layer: layer.id,
            source: layer.pixels.clone(),
            offset: layer.offset,
            selection: tab.selection.clone(),
            canvas: tab.document.content.size(),
            mode: tab.document.content.mode(),
            adjustment: Adjustment::BrightnessContrast {
                brightness: 0,
                contrast: 0,
            },
        });
        self.adjustment_generation.request();
        self.adjustment_dirty = true;
        self.pump_adjustment_preview(window, cx);
        true
    }

    pub(super) fn start_adjustment_histogram(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.histogram_token = self.histogram_token.wrapping_add(1);
        self.adjustment_histogram = None;
        let Some(session) = self.adjustment.as_ref() else {
            return;
        };
        let token = self.histogram_token;
        let source = session.source.clone();
        let offset = session.offset;
        let selection = session.selection.clone();
        let canvas = session.canvas;
        let job = cx.background_executor().spawn(async move {
            adjustments::histogram(&source, |x, y| {
                selection.coverage(
                    i64::from(offset.0) + i64::from(x),
                    i64::from(offset.1) + i64::from(y),
                    canvas.0,
                    canvas.1,
                )
            })
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let histogram = job.await;
            let _ = workspace.update_in(cx, |workspace, _, cx| {
                if workspace.histogram_token == token && workspace.adjustment.is_some() {
                    workspace.adjustment_histogram = Some(histogram);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn set_adjustment(
        &mut self,
        adjustment: Adjustment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.adjustment.as_mut() else {
            return;
        };
        if session.adjustment == adjustment {
            return;
        }
        session.adjustment = adjustment;
        self.adjustment_generation.request();
        self.adjustment_dirty = true;
        self.pump_adjustment_preview(window, cx);
    }

    pub(super) fn apply_adjustment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.adjustment.as_ref() else {
            return;
        };
        if self.active_tab().is_none_or(|tab| {
            tab.id != session.tab || tab.history.generation() != session.generation || tab.busy()
        }) {
            self.clear_adjustment(window, cx);
            self.notice("The image changed while the dialog was open.", cx);
            return;
        }
        let selection = session.selection.clone();
        let layer = session.layer;
        let adjustment = session.adjustment;
        self.clear_adjustment(window, cx);
        if adjustment.is_identity() {
            return;
        }
        self.edit_content(
            move |content, _, _| content.apply_adjustment(layer, adjustment, &selection),
            window,
            cx,
        );
    }

    pub(super) fn clear_adjustment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.adjustment = None;
        self.adjustment_dirty = false;
        self.adjustment_generation.request();
        self.histogram_token = self.histogram_token.wrapping_add(1);
        self.adjustment_histogram = None;
        if let Some(image) = self.adjustment_preview.take() {
            super::drop_images(std::iter::once(image), window, cx);
        }
    }

    fn pump_adjustment_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.adjustment_busy || !self.adjustment_dirty || self.adjustment.is_none() {
            return;
        }
        let Some(session) = self.adjustment.as_ref() else {
            return;
        };
        self.adjustment_dirty = false;
        self.adjustment_busy = true;
        let generation = self.adjustment_generation.latest();
        let source = session.source.clone();
        let offset = session.offset;
        let selection = session.selection.clone();
        let canvas = session.canvas;
        let mode = session.mode;
        let adjustment = session.adjustment;
        let job = cx.background_executor().spawn(async move {
            let image = adjustments::reduced_preview(
                &source,
                adjustment,
                |x, y| {
                    selection.coverage(
                        i64::from(offset.0) + i64::from(x),
                        i64::from(offset.1) + i64::from(y),
                        canvas.0,
                        canvas.1,
                    )
                },
                |pixel| mode.constrain(pixel),
                PREVIEW_EDGE,
            );
            document::preview_render(&image)
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let image = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.adjustment_busy = false;
                if workspace.adjustment_generation.finish(generation) {
                    if let Some(previous) = workspace.adjustment_preview.replace(image) {
                        super::drop_images(std::iter::once(previous), window, cx);
                    }
                    cx.notify();
                } else {
                    super::drop_images(std::iter::once(image), window, cx);
                }
                if workspace.adjustment_dirty {
                    workspace.pump_adjustment_preview(window, cx);
                }
            });
        })
        .detach();
    }
}
