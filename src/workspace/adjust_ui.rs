use std::sync::Arc;

use gpui::{Context, Div, Window, prelude::*};
use image::RgbaImage;

use super::Workspace;
use crate::adjustments::{self, Adjustment};
use crate::commands::{
    BrightnessContrast, ColorBalance, Curves, Desaturate, HueSaturation, InvertColors, Levels,
};
use crate::document::LayerId;
use crate::selection::Selection;

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

impl AdjustKind {
    pub(super) fn of(adjustment: Adjustment) -> Self {
        match adjustment {
            Adjustment::BrightnessContrast { .. } => Self::BrightnessContrast,
            Adjustment::HueSaturation { .. } => Self::HueSaturation,
            Adjustment::Desaturate => Self::Desaturate,
            Adjustment::Invert => Self::Invert,
            Adjustment::ColorBalance(_) => Self::ColorBalance,
            Adjustment::Levels(_) => Self::Levels,
            Adjustment::Curves(_) => Self::Curves,
        }
    }

    pub(super) fn defaults(self) -> Adjustment {
        match self {
            Self::BrightnessContrast => Adjustment::BrightnessContrast {
                brightness: 0,
                contrast: 0,
            },
            Self::HueSaturation => Adjustment::HueSaturation {
                hue: 0,
                saturation: 0,
                lightness: 0,
            },
            Self::Desaturate => Adjustment::Desaturate,
            Self::Invert => Adjustment::Invert,
            Self::ColorBalance => Adjustment::ColorBalance(Default::default()),
            Self::Levels => Adjustment::Levels(Default::default()),
            Self::Curves => Adjustment::Curves(Default::default()),
        }
    }
}

pub(super) struct AdjustmentSession {
    reediting: bool,
    tab: usize,
    generation: u64,
    layer: LayerId,
    source: Arc<RgbaImage>,
    offset: (i32, i32),
    selection: Selection,
    canvas: (u32, u32),
    preview: Arc<crate::effect_preview::EffectPreview>,
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

    pub(super) fn begin_adjustment(
        &mut self,
        adjustment: Adjustment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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
        let edge = self.effect_preview_edge(true, window);
        let reediting = self.effect_reediting;
        let (content, selection) = if reediting {
            tab.document
                .content
                .retained_effect_input(layer.id)
                .unwrap_or_else(|| (tab.document.content.clone(), tab.selection.clone()))
        } else {
            (tab.document.content.clone(), tab.selection.clone())
        };
        let source = content.layer(layer.id).unwrap().pixels.clone();
        self.adjustment = Some(AdjustmentSession {
            reediting,
            tab: tab.id,
            generation: tab.history.generation(),
            layer: layer.id,
            source,
            offset: layer.offset,
            selection: selection.clone(),
            canvas: tab.document.content.size(),
            preview: Arc::new(crate::effect_preview::EffectPreview::new(
                content, layer.id, selection, edge,
            )),
            adjustment,
        });
        self.effect_before = false;
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
        let reediting = session.reediting;
        let layer = session.layer;
        let adjustment = session.adjustment;
        let target = (session.tab, session.generation);
        self.clear_adjustment(window, cx);
        if adjustment.is_identity() && !reediting {
            return;
        }
        self.edit_effect(
            super::effects::Effect::Adjustment(adjustment),
            target,
            layer,
            selection,
            reediting,
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
            super::drop_images(image.images(), window, cx);
        }
    }

    pub(super) fn resize_adjustment_preview(
        &mut self,
        edge: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.adjustment.as_mut() else {
            return;
        };
        let Some(preview) = session.preview.resized(edge) else {
            return;
        };
        session.preview = Arc::new(preview);
        self.adjustment_generation.request();
        self.adjustment_dirty = true;
        self.pump_adjustment_preview(window, cx);
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
        let preview = session.preview.clone();
        let adjustment = session.adjustment;
        let job = cx.background_executor().spawn(async move {
            let image = preview.adjustment(adjustment);
            super::EffectImages::new(&preview, &image)
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let image = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.adjustment_busy = false;
                if workspace.adjustment_generation.finish(generation) {
                    if let Some(previous) = workspace.adjustment_preview.replace(image) {
                        super::drop_images(previous.images(), window, cx);
                    }
                    cx.notify();
                } else {
                    super::drop_images(image.images(), window, cx);
                }
                if workspace.adjustment_dirty {
                    workspace.pump_adjustment_preview(window, cx);
                }
            });
        })
        .detach();
    }
}
