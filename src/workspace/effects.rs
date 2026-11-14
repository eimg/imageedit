use gpui::{Context, Div, Window, prelude::*};

use super::{Workspace, adjust_ui::AdjustKind, filter_ui::FilterKind};
use crate::adjustments::Adjustment;
use crate::commands::{ReopenLastEffect, RepeatLastEffect};
use crate::document::{DocumentContent, LayerId};
use crate::filters::FilterSpec;
use crate::preview::Dirty;
use crate::selection::Selection;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum Effect {
    Adjustment(Adjustment),
    Filter(FilterSpec),
}

impl Effect {
    fn valid(self) -> bool {
        match self {
            Self::Adjustment(value) => value.valid(),
            Self::Filter(FilterSpec::Blur { radius }) => radius <= crate::filters::MAX_RADIUS,
            Self::Filter(FilterSpec::Sharpen { radius, amount }) => {
                radius <= crate::filters::MAX_RADIUS && amount <= 100
            }
            Self::Filter(FilterSpec::Shadow {
                radius,
                offset,
                opacity,
                ..
            }) => {
                radius <= crate::filters::MAX_RADIUS
                    && opacity <= 100
                    && offset.0.unsigned_abs() <= 16384
                    && offset.1.unsigned_abs() <= 16384
            }
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::Adjustment(value) => AdjustKind::of(value) as usize,
            Self::Filter(value) => 7 + FilterKind::of(value) as usize,
        }
    }

    fn is_identity(self) -> bool {
        match self {
            Self::Adjustment(value) => value.is_identity(),
            Self::Filter(value) => value.is_identity(),
        }
    }

    fn fits(self, content: &DocumentContent, layer: LayerId) -> bool {
        match self {
            Self::Filter(FilterSpec::Shadow {
                radius,
                offset,
                opacity,
                ..
            }) if opacity != 0 => content.drop_shadow_fits(layer, radius, offset),
            _ => true,
        }
    }

    fn retained_spec(self) -> Option<crate::document::layer_effect::EffectSpec> {
        use crate::document::layer_effect::EffectSpec;
        match self {
            Self::Adjustment(value) => Some(EffectSpec::Adjustment(value)),
            Self::Filter(FilterSpec::Blur { radius }) => Some(EffectSpec::Blur { radius }),
            Self::Filter(FilterSpec::Sharpen { radius, amount }) => {
                Some(EffectSpec::Sharpen { radius, amount })
            }
            Self::Filter(FilterSpec::Shadow { .. }) => None,
        }
    }

    fn apply(self, content: &mut DocumentContent, layer: LayerId, selection: &Selection) -> Dirty {
        match self {
            Self::Adjustment(value) => content.apply_adjustment(layer, value, selection),
            Self::Filter(spec @ FilterSpec::Shadow { .. }) => content
                .place_drop_shadow(layer, spec)
                .map_or(Dirty::None, Dirty::Region),
            Self::Filter(value) => content.apply_filter(layer, value, selection),
        }
    }
}

#[derive(Clone, Copy)]
struct PendingEffect {
    tab: usize,
    generation: u64,
    order: u64,
    effect: Effect,
}

#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Preferences {
    values: [Option<Effect>; 10],
    last: Option<Effect>,
}

impl Preferences {
    pub(super) fn valid(&self) -> bool {
        self.values.iter().enumerate().all(|(slot, effect)| {
            effect.is_none_or(|effect| {
                effect.valid() && effect.slot() == slot && !effect.is_identity()
            })
        }) && self
            .last
            .is_none_or(|effect| effect.valid() && self.values[effect.slot()] == Some(effect))
    }
}

#[derive(Default)]
pub(super) struct EffectMemory {
    // Keep only small parameter values, never document pixels or selections.
    values: [Option<(u64, Effect)>; 10],
    last: Option<(u64, Effect)>,
    order: u64,
    pending: Vec<PendingEffect>,
}

impl EffectMemory {
    pub(super) fn preferences(&self) -> Preferences {
        Preferences {
            values: self.values.map(|value| value.map(|(_, effect)| effect)),
            last: self.last.map(|(_, effect)| effect),
        }
    }

    pub(super) fn restore(&mut self, preferences: &Preferences) {
        for (current, saved) in self.values.iter_mut().zip(preferences.values) {
            if current.is_none() {
                *current = saved.map(|effect| (0, effect));
            }
        }
        if self.last.is_none() {
            self.last = preferences.last.map(|effect| (0, effect));
        }
    }

    pub(super) fn adjustment(&self, kind: AdjustKind) -> Adjustment {
        match self.values[kind as usize] {
            Some((_, Effect::Adjustment(value))) => value,
            _ => kind.defaults(),
        }
    }

    pub(super) fn filter(&self, kind: FilterKind) -> Option<FilterSpec> {
        match self.values[7 + kind as usize] {
            Some((_, Effect::Filter(value))) => Some(value),
            _ => None,
        }
    }

    fn queue(&mut self, tab: usize, generation: u64, effect: Effect) {
        self.order = self.order.saturating_add(1);
        self.pending.push(PendingEffect {
            tab,
            generation,
            order: self.order,
            effect,
        });
    }

    pub(super) fn finish(&mut self, tab: usize, generation: u64, published: bool) {
        let Some(index) = self
            .pending
            .iter()
            .position(|pending| pending.tab == tab && pending.generation == generation)
        else {
            return;
        };
        let pending = self.pending.remove(index);
        // No-op edits, rejected generations, and closed tabs must not become repeatable.
        if !published || pending.effect.is_identity() {
            return;
        }
        let slot = &mut self.values[pending.effect.slot()];
        if slot.is_none_or(|(order, _)| pending.order > order) {
            *slot = Some((pending.order, pending.effect));
        }
        if self.last.is_none_or(|(order, _)| pending.order > order) {
            self.last = Some((pending.order, pending.effect));
        }
    }
}

impl Workspace {
    pub(super) fn bind_effect_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        if !self.last_effect_enabled() {
            return root;
        }
        root.on_action(cx.listener(|this, _: &RepeatLastEffect, window, cx| {
            this.run_last_effect(false, window, cx);
        }))
        .on_action(cx.listener(|this, _: &ReopenLastEffect, window, cx| {
            this.run_last_effect(true, window, cx);
        }))
    }

    fn last_effect_enabled(&self) -> bool {
        self.effect_memory.last.is_some()
            && self.form.is_none()
            && !self.dialog_open
            && self.pdf_job.is_none()
            && self.stroke.is_none()
            && self.guide_drag.is_none()
            && self.path_gesture.is_none()
            && self.selection_drag.is_none()
            && self.opacity_drag.is_none()
            && (self.move_drag.is_none() || self.numeric_edit.is_some())
            && self
                .scale_drag
                .as_ref()
                .is_none_or(|scale| !scale.changed())
            && self.gradient.is_none()
            && self.warp.as_ref().is_none_or(|warp| !warp.changed())
            && self.crop.is_none()
            && self.free_points.is_empty()
            && self.active_tab().is_some_and(|tab| {
                !tab.busy()
                    && tab
                        .document
                        .content
                        .layer(tab.active_layer)
                        .is_some_and(|layer| layer.visible && layer.text.is_none())
            })
    }

    pub(super) fn run_last_effect(
        &mut self,
        reopen: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.last_effect_enabled() {
            return;
        }
        if self.commit_numeric_with_followup(
            super::numeric::NumericFollowup::Effect { reopen },
            window,
            cx,
        ) {
            return;
        }
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| this.run_last_effect(reopen, window, cx));
            return;
        }
        // Resolve the target only after typed text/geometry has landed.
        if !self.last_effect_enabled() {
            return;
        }
        let Some((_, effect)) = self.effect_memory.last else {
            return;
        };
        if reopen {
            match effect {
                Effect::Adjustment(value) => {
                    self.adjustment_form(AdjustKind::of(value), window, cx)
                }
                Effect::Filter(value) => self.filter_form(FilterKind::of(value), window, cx),
            }
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        if !effect.fits(&tab.document.content, tab.active_layer) {
            self.notice("The repeated drop shadow does not fit in the layer stack or its offset is too large.", cx);
            return;
        }
        let target = (tab.id, tab.history.generation());
        self.edit_effect(
            effect,
            target,
            tab.active_layer,
            tab.selection.clone(),
            false,
            window,
            cx,
        );
    }

    pub(super) fn edit_retained_layer_effect(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.form.is_some() || self.dialog_open || self.active_tab().is_none_or(|tab| tab.busy())
        {
            return;
        }
        let Some(effect) = self
            .active_tab()
            .and_then(|tab| tab.document.content.layer(tab.active_layer))
            .and_then(|layer| layer.effect.as_ref())
            .map(|effect| effect.spec)
        else {
            return;
        };
        self.effect_reediting = true;
        match effect {
            crate::document::layer_effect::EffectSpec::Adjustment(value) => {
                self.adjustment_form(AdjustKind::of(value), window, cx)
            }
            spec => self.filter_form(FilterKind::of(spec.filter().unwrap()), window, cx),
        }
        self.effect_reediting = false;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn edit_effect(
        &mut self,
        effect: Effect,
        target: (usize, u64),
        layer: LayerId,
        selection: Selection,
        reediting: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if (effect.is_identity() && !reediting)
            || self.active_tab().is_none_or(|tab| {
                tab.id != target.0 || tab.history.generation() != target.1 || tab.busy()
            })
        {
            return;
        }
        if effect.retained_spec().is_some()
            && self.active_tab().is_some_and(|tab| {
                !tab.document
                    .content
                    .can_retain_effect(layer, &selection, reediting)
            })
        {
            self.notice("There is not enough layer memory to retain this effect. Bake an existing effect or use a smaller layer.", cx);
            return;
        }
        self.edit_content(
            move |content, _, _| {
                if let Some(spec) = effect.retained_spec() {
                    content.apply_retained_effect(layer, spec, &selection, reediting)
                } else {
                    effect.apply(content, layer, &selection)
                }
            },
            window,
            cx,
        );
        if self
            .active_tab()
            .is_some_and(|tab| tab.id == target.0 && tab.editing)
        {
            self.effect_memory.queue(target.0, target.1, effect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_keeps_successful_effects_without_pending_document_context() {
        let mut memory = EffectMemory::default();
        let invert = Effect::Adjustment(Adjustment::Invert);
        memory.queue(1, 10, invert);
        memory.finish(1, 10, true);
        memory.queue(2, 50, Effect::Filter(FilterSpec::Blur { radius: 8 }));
        let bytes = serde_json::to_vec(&memory.preferences()).unwrap();
        let preferences: Preferences = serde_json::from_slice(&bytes).unwrap();
        assert!(preferences.valid());
        let mut restarted = EffectMemory::default();
        restarted.restore(&preferences);
        assert!(restarted.pending.is_empty());
        assert_eq!(restarted.last, Some((0, invert)));
        assert_eq!(restarted.filter(FilterKind::Blur), None);
        let blur = Effect::Filter(FilterSpec::Blur { radius: 4 });
        restarted.queue(3, 1, blur);
        restarted.finish(3, 1, false);
        assert_eq!(restarted.last.map(|(_, effect)| effect), Some(invert));
        restarted.queue(3, 2, blur);
        restarted.finish(3, 2, true);
        assert_eq!(restarted.last.map(|(_, effect)| effect), Some(blur));
    }
    use crate::adjustments::{ColorBalance, Curves, Levels};
    use crate::document::LayerLocks;
    use image::{Rgba, RgbaImage};

    #[test]
    fn successful_parameters_are_independent_for_every_effect_kind() {
        let adjustments = [
            Adjustment::BrightnessContrast {
                brightness: 12,
                contrast: -9,
            },
            Adjustment::HueSaturation {
                hue: -45,
                saturation: 20,
                lightness: -8,
            },
            Adjustment::Desaturate,
            Adjustment::Invert,
            Adjustment::ColorBalance(ColorBalance {
                midtones: [8, -7, 6],
                ..Default::default()
            }),
            Adjustment::Levels(Levels::default().set_stops(
                crate::adjustments::MapChannel::Blue,
                crate::adjustments::LevelStops::IDENTITY.set_gamma(175),
            )),
            Adjustment::Curves(Curves {
                blue: crate::adjustments::Curve::identity()
                    .try_add(100, 175)
                    .unwrap(),
                ..Default::default()
            }),
        ];
        let filters = [
            FilterSpec::Blur { radius: 3 },
            FilterSpec::Sharpen {
                radius: 7,
                amount: 42,
            },
            FilterSpec::Shadow {
                radius: 2,
                offset: (-8, 12),
                opacity: 65,
                color: Rgba([40, 60, 80, 90]),
            },
        ];
        let mut memory = EffectMemory::default();
        for adjustment in adjustments {
            assert_eq!(
                memory.adjustment(AdjustKind::of(adjustment)),
                AdjustKind::of(adjustment).defaults()
            );
        }
        for filter in filters {
            assert_eq!(memory.filter(FilterKind::of(filter)), None);
        }
        for (generation, effect) in adjustments
            .into_iter()
            .map(Effect::Adjustment)
            .chain(filters.into_iter().map(Effect::Filter))
            .enumerate()
        {
            memory.queue(1, generation as u64, effect);
            memory.finish(1, generation as u64, true);
        }
        for adjustment in adjustments {
            assert_eq!(memory.adjustment(AdjustKind::of(adjustment)), adjustment);
        }
        for filter in filters {
            assert_eq!(memory.filter(FilterKind::of(filter)), Some(filter));
        }
        assert_eq!(
            memory.last.map(|(_, effect)| effect),
            Some(Effect::Filter(filters[2]))
        );
        assert!(memory.pending.is_empty());
    }

    #[test]
    fn failed_noop_and_stale_completions_do_not_replace_the_last_effect() {
        let mut memory = EffectMemory::default();
        let invert = Effect::Adjustment(Adjustment::Invert);
        memory.queue(1, 4, invert);
        memory.finish(1, 4, true);
        let last = memory.last;
        memory.queue(1, 5, Effect::Filter(FilterSpec::Blur { radius: 8 }));
        memory.finish(2, 5, true);
        memory.finish(1, 6, true);
        assert_eq!(memory.pending.len(), 1);
        memory.finish(1, 5, false);
        memory.queue(1, 6, Effect::Filter(FilterSpec::Blur { radius: 0 }));
        memory.finish(1, 6, true);
        assert_eq!(memory.last, last);
        assert_eq!(memory.filter(FilterKind::Blur), None);
        assert!(memory.pending.is_empty());
    }

    #[test]
    fn cross_tab_completion_order_cannot_restore_older_settings() {
        let mut memory = EffectMemory::default();
        let old = Effect::Filter(FilterSpec::Blur { radius: 2 });
        let new = Effect::Filter(FilterSpec::Blur { radius: 8 });
        memory.queue(1, 2, old);
        memory.queue(2, 40, new);
        memory.finish(2, 40, true);
        memory.finish(1, 2, true);
        assert_eq!(memory.last.map(|(_, value)| value), Some(new));
        assert_eq!(
            memory.filter(FilterKind::Blur),
            Some(FilterSpec::Blur { radius: 8 })
        );
        // An older operation of another kind is still remembered independently.
        memory.queue(1, 3, Effect::Adjustment(Adjustment::Invert));
        memory.queue(2, 41, new);
        memory.finish(2, 41, true);
        memory.finish(1, 3, true);
        assert_eq!(memory.adjustment(AdjustKind::Invert), Adjustment::Invert);
        assert_eq!(memory.last.map(|(_, value)| value), Some(new));
    }

    #[test]
    fn repeating_uses_the_current_target_and_selection_and_keeps_layer_locks() {
        let mut content = DocumentContent::single_layer(
            "base".into(),
            RgbaImage::from_pixel(3, 1, Rgba([10, 20, 30, 255])),
        );
        let base = content.top_layer_id();
        let original = content.layer(base).unwrap().pixels.clone();
        let target = content
            .add_layer(base, "target".into(), Rgba([80, 100, 120, 128]))
            .unwrap();
        let locks = LayerLocks {
            alpha: true,
            position: true,
        };
        content.set_layer_locks(target, locks);
        let selection = Selection::rectangle((1, 0), (1, 0), 3, 1);
        assert_ne!(
            Effect::Adjustment(Adjustment::Invert).apply(&mut content, target, &selection),
            Dirty::None
        );
        let layer = content.layer(target).unwrap();
        assert_eq!(*layer.pixels.get_pixel(0, 0), Rgba([80, 100, 120, 128]));
        assert_eq!(*layer.pixels.get_pixel(1, 0), Rgba([175, 155, 135, 128]));
        assert_eq!(*layer.pixels.get_pixel(2, 0), Rgba([80, 100, 120, 128]));
        assert_eq!(layer.locks, locks);
        assert!(std::sync::Arc::ptr_eq(
            &content.layer(base).unwrap().pixels,
            &original
        ));
        // An empty current selection yields no edit, so publication cannot update memory.
        assert_eq!(
            Effect::Adjustment(Adjustment::Invert).apply(&mut content, target, &Selection::Empty),
            Dirty::None
        );
    }

    #[test]
    fn repeated_shadow_preflights_the_new_destination_and_inserts_below_its_source() {
        let mut content = DocumentContent::single_layer(
            "source".into(),
            RgbaImage::from_pixel(2, 2, Rgba([60, 80, 100, 255])),
        );
        let source = content.top_layer_id();
        let pixels = content.layer(source).unwrap().pixels.clone();
        let effect = Effect::Filter(FilterSpec::Shadow {
            radius: 1,
            offset: (4, -3),
            opacity: 75,
            color: Rgba([20, 10, 0, 255]),
        });
        assert!(effect.fits(&content, source));
        assert_ne!(
            effect.apply(&mut content, source, &Selection::Empty),
            Dirty::None
        );
        assert_eq!(content.layers().len(), 2);
        assert_eq!(content.layers()[1].id, source);
        assert!(std::sync::Arc::ptr_eq(
            &content.layer(source).unwrap().pixels,
            &pixels
        ));
        let invalid = Effect::Filter(FilterSpec::Shadow {
            radius: 1,
            offset: (i32::MIN, 0),
            opacity: 75,
            color: Rgba([0, 0, 0, 255]),
        });
        assert!(!invalid.fits(&content, source));
        assert_eq!(
            invalid.apply(&mut content, source, &Selection::Full),
            Dirty::None
        );
        assert_eq!(content.layers().len(), 2);
    }
}
