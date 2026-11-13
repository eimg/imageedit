use std::sync::Arc;

use gpui::{Context, Div, Window, prelude::*};

use super::Workspace;
use crate::adjustments::PREVIEW_EDGE;
use crate::commands::{DropShadow, GaussianBlur, Sharpen};
use crate::document::LayerId;
use crate::filters::FilterSpec;
use crate::selection::Selection;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FilterKind {
    Blur,
    Sharpen,
    Shadow,
}

impl FilterKind {
    pub(super) fn of(spec: FilterSpec) -> Self {
        match spec {
            FilterSpec::Blur { .. } => Self::Blur,
            FilterSpec::Sharpen { .. } => Self::Sharpen,
            FilterSpec::Shadow { .. } => Self::Shadow,
        }
    }
}

pub(super) struct FilterSession {
    reediting: bool,
    tab: usize,
    generation: u64,
    layer: LayerId,
    selection: Selection,
    preview: Arc<crate::effect_preview::EffectPreview>,
    spec: FilterSpec,
}

impl Workspace {
    pub(super) fn bind_filter_actions(&self, root: Div, cx: &Context<Self>) -> Div {
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
        root.on_action(cx.listener(|this, _: &GaussianBlur, window, cx| {
            this.filter_form(FilterKind::Blur, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Sharpen, window, cx| {
            this.filter_form(FilterKind::Sharpen, window, cx);
        }))
        .on_action(cx.listener(|this, _: &DropShadow, window, cx| {
            this.filter_form(FilterKind::Shadow, window, cx);
        }))
    }

    pub(super) fn begin_filter(
        &mut self,
        spec: FilterSpec,
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
            self.notice("The active layer is hidden. Show it before filtering.", cx);
            return false;
        }
        let reediting = self.effect_reediting;
        let (content, selection) = if reediting {
            tab.document
                .content
                .retained_effect_input(layer.id)
                .unwrap_or_else(|| (tab.document.content.clone(), tab.selection.clone()))
        } else {
            (tab.document.content.clone(), tab.selection.clone())
        };
        self.filter = Some(FilterSession {
            reediting,
            tab: tab.id,
            generation: tab.history.generation(),
            layer: layer.id,
            selection: selection.clone(),
            preview: Arc::new(crate::effect_preview::EffectPreview::new(
                content,
                layer.id,
                selection,
                PREVIEW_EDGE,
            )),
            spec,
        });
        self.effect_before = false;
        self.filter_generation.request();
        self.filter_dirty = true;
        self.pump_filter_preview(window, cx);
        true
    }

    pub(super) fn set_filter(
        &mut self,
        spec: FilterSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.filter.as_mut() else {
            return;
        };
        if session.spec == spec {
            return;
        }
        session.spec = spec;
        self.filter_generation.request();
        self.filter_dirty = true;
        self.pump_filter_preview(window, cx);
    }

    pub(super) fn apply_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(session) = self.filter.as_ref() else {
            return true;
        };
        if self.active_tab().is_none_or(|tab| {
            tab.id != session.tab || tab.history.generation() != session.generation || tab.busy()
        }) {
            self.clear_filter(window, cx);
            self.notice("The image changed while the dialog was open.", cx);
            return true;
        }
        let selection = session.selection.clone();
        let reediting = session.reediting;
        let layer = session.layer;
        let spec = session.spec;
        let target = (session.tab, session.generation);
        if let FilterSpec::Shadow {
            radius,
            opacity,
            offset,
            ..
        } = spec
            && opacity != 0
            && !self
                .active_tab()
                .is_some_and(|tab| tab.document.content.drop_shadow_fits(layer, radius, offset))
        {
            if let Some(form) = self.form.as_mut() {
                form.error = Some(
                    "The drop shadow does not fit in the layer stack or its offset is too large."
                        .into(),
                );
            }
            cx.notify();
            return false;
        }
        self.clear_filter(window, cx);
        if spec.is_identity() && !reediting {
            return true;
        }
        self.edit_effect(
            super::effects::Effect::Filter(spec),
            target,
            layer,
            selection,
            reediting,
            window,
            cx,
        );
        true
    }

    pub(super) fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = None;
        self.filter_dirty = false;
        self.filter_generation.request();
        if let Some(image) = self.filter_preview.take() {
            super::drop_images(image.images(), window, cx);
        }
    }

    fn pump_filter_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter_busy || !self.filter_dirty || self.filter.is_none() {
            return;
        }
        let Some(session) = self.filter.as_ref() else {
            return;
        };
        self.filter_dirty = false;
        self.filter_busy = true;
        let generation = self.filter_generation.latest();
        let preview = session.preview.clone();
        let spec = session.spec;
        let job = cx.background_executor().spawn(async move {
            let image = preview.filter(spec);
            super::EffectImages::new(&preview, &image)
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let image = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.filter_busy = false;
                if workspace.filter_generation.finish(generation) {
                    if let Some(previous) = workspace.filter_preview.replace(image) {
                        super::drop_images(previous.images(), window, cx);
                    }
                    cx.notify();
                } else {
                    super::drop_images(image.images(), window, cx);
                }
                if workspace.filter_dirty {
                    workspace.pump_filter_preview(window, cx);
                }
            });
        })
        .detach();
    }
}
