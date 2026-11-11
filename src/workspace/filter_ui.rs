use std::sync::Arc;

use gpui::{Context, Div, Window, prelude::*};
use image::RgbaImage;

use super::Workspace;
use crate::adjustments::PREVIEW_EDGE;
use crate::commands::{DropShadow, GaussianBlur, Sharpen};
use crate::document::{self, LayerId};
use crate::filters::{self, FilterSpec};
use crate::preview::Dirty;
use crate::selection::Selection;
use crate::transform::ColorMode;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FilterKind {
    Blur,
    Sharpen,
    Shadow,
}

pub(super) struct FilterSession {
    tab: usize,
    generation: u64,
    layer: LayerId,
    source: Arc<RgbaImage>,
    offset: (i32, i32),
    selection: Selection,
    canvas: (u32, u32),
    mode: ColorMode,
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
        self.filter = Some(FilterSession {
            tab: tab.id,
            generation: tab.history.generation(),
            layer: layer.id,
            source: layer.pixels.clone(),
            offset: layer.offset,
            selection: tab.selection.clone(),
            canvas: tab.document.content.size(),
            mode: tab.document.content.mode(),
            spec,
        });
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
        let layer = session.layer;
        let spec = session.spec;
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
        if spec.is_identity() {
            return true;
        }
        self.edit_content(
            move |content, _, _| match spec {
                FilterSpec::Shadow { .. } => content
                    .place_drop_shadow(layer, spec)
                    .map_or(Dirty::None, Dirty::Region),
                _ => content.apply_filter(layer, spec, &selection),
            },
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
            super::drop_images(std::iter::once(image), window, cx);
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
        let source = session.source.clone();
        let offset = session.offset;
        let selection = session.selection.clone();
        let canvas = session.canvas;
        let mode = session.mode;
        let spec = session.spec;
        let job = cx.background_executor().spawn(async move {
            let image = filters::preview(
                &source,
                spec,
                PREVIEW_EDGE,
                |x, y| {
                    selection.coverage(
                        i64::from(offset.0) + i64::from(x),
                        i64::from(offset.1) + i64::from(y),
                        canvas.0,
                        canvas.1,
                    )
                },
                |pixel| mode.constrain(pixel),
            );
            document::preview_render(&image)
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let image = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.filter_busy = false;
                if workspace.filter_generation.finish(generation) {
                    if let Some(previous) = workspace.filter_preview.replace(image) {
                        super::drop_images(std::iter::once(previous), window, cx);
                    }
                    cx.notify();
                } else {
                    super::drop_images(std::iter::once(image), window, cx);
                }
                if workspace.filter_dirty {
                    workspace.pump_filter_preview(window, cx);
                }
            });
        })
        .detach();
    }
}
