use super::{InspectorTab, Workspace, effects, paint_ui, pdf_ui, stroke_ui, text_ui, view};
use crate::color_memory::Changes;
use crate::settings::{self, Patch};
use crate::{
    commands::Tool,
    document::{DEFAULT_JPEG_QUALITY, FillMode},
};
use anyhow::{Result, ensure};
use gpui::{Context, Window};
use std::{path::PathBuf, time::Duration};

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Preferences {
    pub(super) tool: Tool,
    pub(super) show_grid: bool,
    pub(super) show_guides: bool,
    pub(super) snap_grid: bool,
    pub(super) snap_guides: bool,
    pub(super) grid_spacing: u32,
    pub(super) show_layer_bounds: bool,
    pub(super) oval_circle: bool,
    pub(super) selection_radius: u32,
    pub(super) selection_antialias: bool,
    pub(super) free_antialias: bool,
    pub(super) wand_antialias: bool,
    pub(super) selection_feather_enabled: bool,
    pub(super) selection_feather_radius: u32,
    pub(super) selection_ratio: Option<crate::constraints::AspectRatio>,
    pub(super) selection_ratio_value: crate::constraints::AspectRatio,
    pub(super) crop_ratio: Option<crate::constraints::AspectRatio>,
    pub(super) crop_ratio_value: crate::constraints::AspectRatio,
    pub(super) wand_match: crate::selection::RegionMatch,
    pub(super) fill_match: crate::selection::RegionMatch,
    pub(super) scale_lock: bool,
    pub(super) scale_anchor: crate::transform::Anchor,
    pub(super) warp_resample: crate::transform::Resample,
    pub(super) brushes: paint_ui::BrushSettings,
    pub(super) fill_mode: FillMode,
    pub(super) sample_all: bool,
    pub(super) wand_sample_all: bool,
    pub(super) gradient_shape: crate::paint::GradientShape,
    pub(super) gradient_blend: crate::paint::GradientBlend,
    pub(super) gradient_custom: Option<crate::gradient::ColorRamp>,
    pub(super) gradient_use_custom: bool,
    pub(super) gradient_reverse: bool,
    pub(super) gradient_selected_stop: usize,
    pub(super) gradient_presets_expanded: bool,
    pub(super) gradient_selected_preset: Option<u64>,
    pub(super) clone_aligned: bool,
    pub(super) clone_sample_all: bool,
    pub(super) clone_show_source: bool,
    pub(super) inspector_tab: InspectorTab,
    pub(super) path_handle_mode: crate::paths::HandleMode,
    pub(super) path_stroke_style: stroke_ui::PathStrokeStyle,
    pub(super) text_defaults: text_ui::TextDefaults,
    pub(super) pdf_preferences: pdf_ui::PdfPreferences,
    pub(super) jpeg_quality: u8,
    pub(super) pdf_export_dpi: u32,
    pub(super) toolbox_width: f32,
    pub(super) toolbox_folded: bool,
    pub(super) move_snapping_expanded: bool,
    pub(super) effects: effects::Preferences,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            tool: Tool::Move,
            show_grid: false,
            show_guides: false,
            snap_grid: false,
            snap_guides: false,
            grid_spacing: 16,
            show_layer_bounds: false,
            oval_circle: false,
            selection_radius: 0,
            selection_antialias: false,
            free_antialias: false,
            wand_antialias: false,
            selection_feather_enabled: false,
            selection_feather_radius: 8,
            selection_ratio: None,
            selection_ratio_value: crate::constraints::AspectRatio::SQUARE,
            crop_ratio: None,
            crop_ratio_value: crate::constraints::AspectRatio::SQUARE,
            wand_match: Default::default(),
            fill_match: Default::default(),
            scale_lock: true,
            scale_anchor: crate::transform::Anchor::Center,
            warp_resample: crate::transform::Resample::Bilinear,
            brushes: paint_ui::BrushSettings::default(),
            fill_mode: FillMode::Connected,
            sample_all: false,
            wand_sample_all: false,
            gradient_shape: crate::paint::GradientShape::Linear,
            gradient_blend: crate::paint::GradientBlend::ForegroundToBackground,
            gradient_custom: None,
            gradient_use_custom: false,
            gradient_reverse: false,
            gradient_selected_stop: 0,
            gradient_presets_expanded: false,
            gradient_selected_preset: None,
            clone_aligned: true,
            clone_sample_all: false,
            clone_show_source: true,
            inspector_tab: InspectorTab::Layers,
            path_handle_mode: crate::paths::HandleMode::Independent,
            path_stroke_style: stroke_ui::PathStrokeStyle::Brush,
            text_defaults: text_ui::TextDefaults::default(),
            pdf_preferences: Default::default(),
            jpeg_quality: DEFAULT_JPEG_QUALITY,
            pdf_export_dpi: crate::pdf::export::DEFAULT_DPI,
            toolbox_width: view::TOOLBOX_DEFAULT_WIDTH,
            toolbox_folded: false,
            move_snapping_expanded: false,
            effects: Default::default(),
        }
    }
}

impl Preferences {
    /// Inline numeric previews and a toolbox resize become preferences only
    /// after their normal commit. Other settings can still save meanwhile.
    fn committed(
        &self,
        observed: &Self,
        field: Option<super::numeric::NumericField>,
        resizing_toolbox: bool,
    ) -> Self {
        use super::numeric::NumericField::*;
        let mut value = self.clone();
        if resizing_toolbox {
            value.toolbox_width = observed.toolbox_width;
        }
        match field {
            Some(
                BrushSize | BrushSpacing | BrushHardness | BrushOpacity | BrushFlow
                | PathStrokeSize,
            ) => value.brushes = observed.brushes.clone(),
            Some(TextSize | TextBoxWidth | TextBoxHeight | TextLetterSpacing | TextLineSpacing) => {
                value.text_defaults = observed.text_defaults.clone()
            }
            Some(SelectionRadius) => value.selection_radius = observed.selection_radius,
            Some(SelectionFeatherRadius) => {
                value.selection_feather_radius = observed.selection_feather_radius
            }
            Some(SelectionRatioWidth | SelectionRatioHeight) => {
                value.selection_ratio = observed.selection_ratio;
                value.selection_ratio_value = observed.selection_ratio_value;
            }
            Some(CropRatioWidth | CropRatioHeight) => {
                value.crop_ratio = observed.crop_ratio;
                value.crop_ratio_value = observed.crop_ratio_value;
            }
            Some(WandTolerance | WandGapRadius) => value.wand_match = observed.wand_match,
            Some(FillTolerance | FillGapRadius) => value.fill_match = observed.fill_match,
            Some(GridSpacing) => value.grid_spacing = observed.grid_spacing,
            Some(GradientStopPosition | GradientStopOpacity) => {
                value.gradient_custom = observed.gradient_custom
            }
            _ => {}
        }
        value
    }
}

impl settings::Preferences for Preferences {
    fn merge_groups() -> &'static [&'static [&'static str]] {
        &[
            &["selection_ratio"],
            &["crop_ratio"],
            &[
                "gradient_custom",
                "gradient_use_custom",
                "gradient_selected_stop",
                "gradient_selected_preset",
                "gradient_shape",
                "gradient_blend",
                "gradient_reverse",
            ],
            &["effects/last"],
            &["effects/values/0"],
            &["effects/values/1"],
            &["effects/values/2"],
            &["effects/values/3"],
            &["effects/values/4"],
            &["effects/values/5"],
            &["effects/values/6"],
            &["effects/values/7"],
            &["effects/values/8"],
            &["effects/values/9"],
        ]
    }

    fn reconcile_patch(&mut self) {
        self.effects.reconcile_last();
    }

    fn validate(&self) -> Result<()> {
        let ratio_valid = |ratio: crate::constraints::AspectRatio| {
            (1..=16384).contains(&ratio.width) && (1..=16384).contains(&ratio.height)
        };
        ensure!(self.brushes.valid(), "Invalid brush settings.");
        ensure!(self.text_defaults.valid(), "Invalid text defaults.");
        ensure!(
            self.effects.valid(),
            "Invalid remembered effect parameters."
        );
        // Rounded selections may retain geometry outside the canvas. Their u32
        // radius is clamped to each shape when used, rather than to canvas size.
        ensure!(
            self.selection_feather_radius <= 128,
            "Invalid feather radius."
        );
        ensure!(
            ratio_valid(self.selection_ratio_value)
                && ratio_valid(self.crop_ratio_value)
                && self.selection_ratio.is_none_or(ratio_valid)
                && self.crop_ratio.is_none_or(ratio_valid),
            "Invalid aspect ratio."
        );
        ensure!(
            self.wand_match.gap_radius <= crate::selection::MAX_GAP_RADIUS
                && self.fill_match.gap_radius <= crate::selection::MAX_GAP_RADIUS,
            "Invalid gap radius."
        );
        ensure!(
            (1..=4096).contains(&self.grid_spacing),
            "Invalid grid spacing."
        );
        ensure!(
            self.toolbox_width.is_finite() && (140.0..=400.0).contains(&self.toolbox_width),
            "Invalid toolbox width."
        );
        ensure!(
            (1..=100).contains(&self.jpeg_quality),
            "Invalid JPEG quality."
        );
        ensure!(
            (1..=1200).contains(&self.pdf_export_dpi)
                && (1..=1200).contains(&self.pdf_preferences.dpi),
            "Invalid PDF resolution."
        );
        ensure!(
            self.gradient_selected_stop
                < self
                    .gradient_custom
                    .map_or(crate::gradient::MAX_STOPS, |ramp| ramp.stops().len()),
            "Invalid gradient stop index."
        );
        ensure!(
            !self.gradient_use_custom || self.gradient_custom.is_some(),
            "Missing custom gradient."
        );
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Storage {
    path: Option<PathBuf>,
    loading: bool,
    saving: bool,
    timer: bool,
    token: u64,
    observed: Preferences,
    pending: Patch,
    error_reported: bool,
    exit_after_save: Option<bool>,
    exit_save_failed: bool,
}

impl Storage {
    pub(super) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(super) fn defer_exit(&mut self, quit: bool) -> bool {
        if !self.loading
            && !self.saving
            && !self.timer
            && (self.path.is_none() || self.pending.is_empty())
        {
            return false;
        }
        self.exit_after_save = Some(quit || self.exit_after_save.unwrap_or(false));
        true
    }

    fn take_write(&mut self) -> Option<Patch> {
        if self.loading || self.saving || self.path.is_none() || self.pending.is_empty() {
            return None;
        }
        self.saving = true;
        self.exit_save_failed = false;
        Some(std::mem::take(&mut self.pending))
    }

    fn take_failed_exit(&mut self) -> bool {
        std::mem::take(&mut self.exit_save_failed)
    }

    fn failed_write(&mut self, mut patch: Patch) {
        self.saving = false;
        patch.merge(&self.pending);
        self.pending = patch;
        self.exit_save_failed = self.exit_after_save.take().is_some();
    }
}

impl Workspace {
    pub(super) fn load_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_store.path = settings::default_path();
        self.settings_store.observed = self.preferences.clone();
        let Some(path) = self.settings_store.path.clone() else {
            self.errors.push("The settings location is unavailable. Preferences will only be remembered for this session.".into());
            return;
        };
        self.settings_store.loading = true;
        let job = cx
            .background_executor()
            .spawn(async move { settings::load::<Preferences>(&path) });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |this, window, cx| {
                this.record_settings();
                this.settings_store.loading = false;
                match result.and_then(|mut snapshot| {
                    this.settings_store.pending.apply(&mut snapshot)?;
                    let confirmed = snapshot.preferences.clone();
                    let mut live = Patch::default();
                    live.record(&this.settings_store.observed, &this.preferences)?;
                    live.apply(&mut snapshot)?;
                    Ok((snapshot, confirmed))
                }) {
                    Ok((snapshot, mut confirmed)) => {
                        let tool = this.preferences.tool;
                        // A startup read must not switch tools underneath an active
                        // draft/gesture, even if that gesture used the default tool.
                        let keep_tool = this.form.is_some() || this.pending_edit_name().is_some()
                            || this.numeric_edit.is_some() || this.toolbox_drag.is_some() || this.pan_drag.is_some();
                        this.preferences = snapshot.preferences;
                        if keep_tool {
                            this.preferences.tool = tool;
                            confirmed.tool = tool;
                            this.settings_store.pending.preferences.insert("tool".into(), serde_json::to_value(tool).unwrap());
                        }
                        this.effect_memory.restore(&this.preferences.effects);
                        this.preferences.effects = this.effect_memory.preferences();
                        this.settings_store.observed = confirmed;
                        this.color_memory = snapshot.colors;
                        this.gradient_revision = this.gradient_revision.wrapping_add(1);
                        this.reconcile_gradient_preset();
                        this.refresh_gradient(window, cx);
                        if !keep_tool && tool != this.preferences.tool {
                            this.begin_layer_transform_preview(window, cx);
                        }
                        this.start_settings_write(window, cx);
                        this.finish_settings_exit(window, cx);
                    }
                    Err(error) => {
                        this.settings_store.error_reported = true;
                        this.settings_store.exit_after_save = None;
                        this.errors.push(format!("Could not load settings: {error:#}. Current preferences remain available for this session."));
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    fn record_settings(&mut self) -> bool {
        let committed = self.preferences.committed(
            &self.settings_store.observed,
            self.numeric_edit.as_ref().map(|edit| edit.field),
            self.toolbox_drag.is_some(),
        );
        if self.settings_store.observed == committed {
            return false;
        }
        match self
            .settings_store
            .pending
            .record(&self.settings_store.observed, &committed)
        {
            Ok(()) => {
                self.settings_store.observed = committed;
                self.settings_store.exit_save_failed = false;
                true
            }
            Err(error) => {
                if !self.settings_store.error_reported {
                    self.errors
                        .push(format!("Could not remember settings: {error:#}"));
                    self.settings_store.error_reported = true;
                }
                false
            }
        }
    }

    pub(super) fn sync_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.record_settings() {
            self.schedule_settings_write(window, cx);
        }
    }

    fn schedule_settings_write(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_store.loading || self.settings_store.saving || self.settings_store.timer {
            return;
        }
        self.settings_store.timer = true;
        self.settings_store.token = self.settings_store.token.wrapping_add(1);
        let token = self.settings_store.token;
        cx.spawn_in(window, async move |workspace, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let _ = workspace.update_in(cx, |this, window, cx| {
                if !this.settings_store.timer || this.settings_store.token != token {
                    return;
                }
                this.settings_store.timer = false;
                this.record_settings();
                this.start_settings_write(window, cx);
                this.finish_settings_exit(window, cx);
            });
        })
        .detach();
    }

    pub(super) fn flush_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_settings(window, cx);
        if self.settings_store.timer {
            self.settings_store.timer = false;
            self.settings_store.token = self.settings_store.token.wrapping_add(1);
        }
        // A previous failure leaves pending values without an armed timer.
        self.start_settings_write(window, cx);
    }

    pub(super) fn queue_settings_patch(&mut self, patch: &Patch) {
        self.settings_store.pending.merge(patch);
        self.settings_store.exit_save_failed = false;
    }

    pub(super) fn defer_settings_exit(
        &mut self,
        quit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.record_settings();
        // After a failed exit-time retry, a second explicit Close/Quit may
        // proceed. A permanently unwritable settings file must not trap users.
        if self.settings_store.take_failed_exit() {
            return false;
        }
        self.flush_settings(window, cx);
        self.settings_store.defer_exit(quit)
    }

    pub(super) fn change_color_memory(
        &mut self,
        changes: Changes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.color_memory.apply(&changes);
        self.settings_store.exit_save_failed = false;
        self.settings_store.pending.colors.merge(&changes);
        self.record_settings();
        self.schedule_settings_write(window, cx);
        cx.notify();
    }

    pub(super) fn set_color(
        &mut self,
        swatch: usize,
        color: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_color_memory(Changes::swatch(swatch, color), window, cx);
    }

    pub(super) fn remember_color(
        &mut self,
        color: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change_color_memory(Changes::color(color), window, cx);
    }

    fn start_settings_write(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.settings_store.path.clone() else {
            return;
        };
        let Some(patch) = self.settings_store.take_write() else {
            return;
        };
        let saved = patch.clone();
        let job = cx
            .background_executor()
            .spawn(async move { settings::save::<Preferences>(&path, &saved) });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |this, window, cx| {
                this.settings_store.saving = false;
                this.record_settings();
                match result {
                    Ok(mut snapshot) => {
                        // Never change the active tool or its live preferences on
                        // save completion. Other instances' untouched keys stay on disk.
                        snapshot.colors.apply(&this.settings_store.pending.colors);
                        let colors_changed = this.color_memory != snapshot.colors;
                        this.color_memory = snapshot.colors;
                        this.settings_store.error_reported = false;
                        this.settings_store.exit_save_failed = false;
                        if colors_changed { this.refresh_gradient(window, cx); }
                        this.start_settings_write(window, cx);
                        this.finish_settings_exit(window, cx);
                    }
                    Err(error) => {
                        this.settings_store.failed_write(patch);
                        if !this.settings_store.error_reported || this.settings_store.exit_save_failed {
                            let recovery = if this.settings_store.exit_save_failed {
                                "Close or quit again to exit without saving preferences, or change/reset a setting to retry."
                            } else {
                                "Change or reset a setting, or close the app, to retry saving."
                            };
                            this.errors.push(format!("Could not save settings: {error:#}. Current preferences remain available. {recovery}"));
                            this.settings_store.error_reported = true;
                        }
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    fn finish_settings_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_store.loading || self.settings_store.saving || self.settings_store.timer {
            return;
        }
        if let Some(quit) = self.settings_store.exit_after_save.take()
            && self.request_exit(quit, window, cx)
        {
            if quit {
                cx.quit();
            } else {
                window.remove_window();
            }
        }
    }

    pub(super) fn reconcile_gradient_preset(&mut self) {
        if self.gradient_store.loading {
            return;
        }
        self.preferences.gradient_selected_preset =
            self.preferences.gradient_selected_preset.filter(|id| {
                self.gradient_library
                    .get(*id)
                    .is_some_and(|preset| preset.value == self.current_gradient_preset())
            });
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::settings::Preferences as _;
    use serde_json::{Value, json};

    pub(in crate::workspace) fn configured() -> Preferences {
        let mut value = serde_json::to_value(Preferences::default()).unwrap();
        // Exercise every preference family, including independent paint tools.
        for field in [
            "show_grid",
            "show_guides",
            "snap_grid",
            "snap_guides",
            "show_layer_bounds",
            "oval_circle",
            "selection_antialias",
            "free_antialias",
            "wand_antialias",
            "selection_feather_enabled",
            "sample_all",
            "wand_sample_all",
            "gradient_use_custom",
            "gradient_reverse",
            "gradient_presets_expanded",
            "clone_sample_all",
            "toolbox_folded",
            "move_snapping_expanded",
        ] {
            value[field] = json!(true);
        }
        for field in ["scale_lock", "clone_aligned", "clone_show_source"] {
            value[field] = json!(false);
        }
        value["tool"] = json!("CloneStamp");
        value["grid_spacing"] = json!(29);
        value["selection_radius"] = json!(13);
        value["selection_feather_radius"] = json!(17);
        for field in [
            "selection_ratio",
            "selection_ratio_value",
            "crop_ratio",
            "crop_ratio_value",
        ] {
            value[field] = json!({"width":7,"height":5});
        }
        value["wand_match"] = json!({"tolerance":24,"gap_radius":3});
        value["fill_match"] = json!({"tolerance":38,"gap_radius":5});
        value["scale_anchor"] = json!("SouthEast");
        value["warp_resample"] = json!("Bicubic");
        for (index, tool) in [
            "brush",
            "pencil",
            "eraser",
            "blur",
            "sharpen",
            "clone_stamp",
            "outline",
        ]
        .into_iter()
        .enumerate()
        {
            value["brushes"][tool] = json!({"diameter":21+index,"spacing":27,"hardness":44,"opacity":63,"flow":19,"buildup":true});
        }
        value["brushes"]["smoothing"] = json!(["Light", "Strong", "Off", "Light", "Strong"]);
        value["fill_mode"] = json!("Selection");
        value["gradient_shape"] = json!("Radial");
        value["gradient_blend"] = json!("ForegroundToTransparent");
        value["gradient_custom"] = json!([
            {"position":0,"color":[1,2,3,0]},
            {"position":375,"color":[7,8,9,101]},
            {"position":1000,"color":[90,80,70,255]}
        ]);
        value["gradient_selected_stop"] = json!(1);
        value["gradient_selected_preset"] = json!(9);
        value["inspector_tab"] = json!("Paths");
        value["path_handle_mode"] = json!("Symmetric");
        value["path_stroke_style"] = json!("Pencil");
        value["text_defaults"] = json!({"family":"serif","face":{"bold":true,"italic":true},"size":47.5,"spacing":{"letter":2.25,"line":1.5},"layout":{"width":240,"height":160,"align":"center"}});
        value["pdf_preferences"] =
            json!({"dpi":240,"mode":"Separate","background":"Transparent","canvas":"FitAllPages"});
        value["jpeg_quality"] = json!(81);
        value["pdf_export_dpi"] = json!(360);
        value["toolbox_width"] = json!(225.5);
        let mut effects = vec![Value::Null; 10];
        let last = effects::Effect::Filter(crate::filters::FilterSpec::Shadow {
            radius: 8,
            offset: (-12, 23),
            opacity: 64,
            color: image::Rgba([9, 17, 25, 131]),
        });
        effects[9] = serde_json::to_value(last).unwrap();
        effects[0] = json!({"Adjustment":{"BrightnessContrast":{"brightness":15,"contrast":-12}}});
        value["effects"] = json!({"values":effects,"last":last});
        let preferences: Preferences = serde_json::from_value(value).unwrap();
        preferences.validate().unwrap();
        preferences
    }

    #[test]
    fn every_preference_family_and_colors_round_trip_without_document_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let expected = configured();
        let mut patch = Patch::default();
        patch.record(&Preferences::default(), &expected).unwrap();
        patch.colors = Changes::swatch(0, 0x123456);
        settings::save::<Preferences>(&path, &patch).unwrap();
        let loaded = settings::load::<Preferences>(&path).unwrap();
        assert!(loaded.preferences == expected);
        assert_eq!(loaded.colors.swatches[0], 0x123456);
        let wire: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for field in [
            "tabs",
            "document",
            "selection",
            "crop",
            "clone_source",
            "gradient",
            "warp",
            "text_draft",
            "guides",
            "active_layer",
            "pending",
        ] {
            assert!(wire["preferences"].get(field).is_none(), "{field}");
        }
        assert!(wire["preferences"]["effects"].get("pending").is_none());
    }

    #[test]
    fn edits_during_load_and_multiple_instances_merge_only_changed_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = Preferences::default();
        let mut first = original.clone();
        first.grid_spacing = 29;
        let mut first_patch = Patch::default();
        first_patch.record(&original, &first).unwrap();
        settings::save::<Preferences>(&path, &first_patch).unwrap();
        // The second instance still has startup defaults while a choice is made.
        let mut second = original.clone();
        second.clone_aligned = false;
        let mut second_patch = Patch::default();
        second_patch.record(&original, &second).unwrap();
        let mut loaded = settings::load::<Preferences>(&path).unwrap();
        second_patch.apply(&mut loaded).unwrap();
        assert_eq!(loaded.preferences.grid_spacing, 29);
        assert!(!loaded.preferences.clone_aligned);
        settings::save::<Preferences>(&path, &second_patch).unwrap();
        assert_eq!(
            settings::load::<Preferences>(&path)
                .unwrap()
                .preferences
                .grid_spacing,
            29
        );
        // A queued change back to the original value must override an in-flight save.
        let mut pending = first_patch.clone();
        let mut reset = Patch::default();
        reset.record(&first, &original).unwrap();
        pending.merge(&reset);
        settings::save::<Preferences>(&path, &pending).unwrap();
        let result = settings::load::<Preferences>(&path).unwrap();
        assert_eq!(result.preferences.grid_spacing, 16);
        assert!(!result.preferences.clone_aligned);
    }

    #[test]
    fn legacy_colors_are_imported_once_and_the_original_file_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let colors = dir.path().join("colors.json");
        let path = dir.path().join("settings.json");
        crate::color_memory::save_changes(&colors, &Changes::swatch(1, 0xabcdef)).unwrap();
        let original = std::fs::read(&colors).unwrap();
        let snapshot = settings::load::<Preferences>(&path).unwrap();
        assert_eq!(snapshot.colors.swatches[1], 0xabcdef);
        let patch = Patch {
            colors: Changes::color(0x123456),
            ..Default::default()
        };
        settings::save::<Preferences>(&path, &patch).unwrap();
        assert_eq!(std::fs::read(&colors).unwrap(), original);
        crate::color_memory::save_changes(&colors, &Changes::swatch(1, 0x111111)).unwrap();
        assert_eq!(
            settings::load::<Preferences>(&path)
                .unwrap()
                .colors
                .swatches[1],
            0xabcdef
        );
    }

    #[test]
    fn nested_tool_changes_preserve_other_brushes_smoothing_and_effect_slots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let defaults = Preferences::default();
        let saved = configured();
        let mut first = Patch::default();
        first.record(&defaults, &saved).unwrap();
        settings::save::<Preferences>(&path, &first).unwrap();
        // A brush size chosen before loading must not reset the other tools.
        let mut early_json = serde_json::to_value(&defaults).unwrap();
        early_json["brushes"]["brush"]["diameter"] = json!(93);
        early_json["brushes"]["smoothing"][0] = json!("Strong");
        early_json["effects"]["values"][3] = json!({"Adjustment":"Invert"});
        early_json["effects"]["last"] = json!({"Adjustment":"Invert"});
        let early: Preferences = serde_json::from_value(early_json).unwrap();
        let mut patch = Patch::default();
        patch.record(&defaults, &early).unwrap();
        let result = settings::save::<Preferences>(&path, &patch).unwrap();
        let value = serde_json::to_value(&result.preferences).unwrap();
        assert_eq!(value["brushes"]["brush"]["diameter"], json!(93));
        assert_eq!(value["brushes"]["eraser"]["diameter"], json!(23));
        assert_eq!(
            value["brushes"]["smoothing"],
            json!(["Strong", "Strong", "Off", "Light", "Strong"])
        );
        assert!(!value["effects"]["values"][0].is_null());
        assert!(!value["effects"]["values"][9].is_null());
        assert_eq!(value["effects"]["last"], json!({"Adjustment":"Invert"}));
        // Later replacement of a queued subtree must discard its older leaf edits.
        let mut reset = Patch::default();
        reset.record(&early, &defaults).unwrap();
        patch.merge(&reset);
        let reset_result = settings::save::<Preferences>(&path, &patch).unwrap();
        assert_eq!(
            serde_json::to_value(reset_result.preferences).unwrap()["brushes"]["brush"]["diameter"],
            json!(16)
        );
    }

    #[test]
    fn queued_palette_replacements_and_later_stop_edits_keep_the_latest_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let defaults = Preferences::default();
        let initial = configured();
        let mut edited = initial.clone();
        edited
            .gradient_custom
            .as_mut()
            .unwrap()
            .set_color(1, image::Rgba([20, 30, 40, 50]));
        let mut pending = Patch::default();
        pending.record(&defaults, &initial).unwrap();
        let mut newer = Patch::default();
        newer.record(&initial, &edited).unwrap();
        pending.merge(&newer);
        let saved = settings::save::<Preferences>(&path, &pending).unwrap();
        assert!(saved.preferences == edited);
        let mut reset = edited.clone();
        reset.gradient_custom = None;
        reset.gradient_use_custom = false;
        reset.gradient_selected_stop = 0;
        reset.gradient_selected_preset = None;
        let mut replace = Patch::default();
        replace.record(&edited, &reset).unwrap();
        pending.merge(&replace);
        let saved = settings::save::<Preferences>(&path, &pending).unwrap();
        assert!(saved.preferences == reset);
    }

    fn save_change(path: &std::path::Path, before: &Preferences, after: &Preferences) {
        let mut patch = Patch::default();
        patch.record(before, after).unwrap();
        settings::save::<Preferences>(path, &patch).unwrap();
    }

    #[test]
    fn concurrent_palette_and_optional_ratio_replacements_do_not_block_other_saves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let base = configured();
        save_change(&path, &Preferences::default(), &base);
        let mut local = base.clone();
        local
            .gradient_custom
            .as_mut()
            .unwrap()
            .set_color(1, image::Rgba([12, 34, 56, 78]));
        local.crop_ratio = Some(crate::constraints::AspectRatio::SQUARE);
        local.grid_spacing = 45;
        let mut pending = Patch::default();
        pending.record(&base, &local).unwrap();
        let mut other = base.clone();
        other.reset_tool(Tool::Gradient);
        other.reset_tool(Tool::Crop);
        other.wand_sample_all = false;
        save_change(&path, &base, &other);
        let saved = settings::save::<Preferences>(&path, &pending).unwrap();
        assert_eq!(saved.preferences.gradient_custom, local.gradient_custom);
        assert_eq!(
            saved.preferences.gradient_use_custom,
            local.gradient_use_custom
        );
        assert_eq!(
            saved.preferences.gradient_selected_stop,
            local.gradient_selected_stop
        );
        assert_eq!(saved.preferences.crop_ratio, local.crop_ratio);
        assert_eq!(saved.preferences.grid_spacing, 45);
        assert!(!saved.preferences.wand_sample_all);
        // A later reset of the whole palette also supersedes queued stop edits.
        let mut reset = Patch::default();
        reset.record(&local, &other).unwrap();
        pending.merge(&reset);
        let saved = settings::save::<Preferences>(&path, &pending).unwrap();
        assert_eq!(saved.preferences.gradient_custom, None);
        assert!(!saved.preferences.gradient_use_custom);
    }

    #[test]
    fn concurrent_effect_variants_and_parameter_changes_keep_valid_complete_slots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut base_json = serde_json::to_value(Preferences::default()).unwrap();
        let brightness =
            json!({"Adjustment":{"BrightnessContrast":{"brightness":10,"contrast":0}}});
        base_json["effects"]["values"][0] = brightness.clone();
        base_json["effects"]["last"] = brightness;
        let base: Preferences = serde_json::from_value(base_json.clone()).unwrap();
        for other_effect in ["invert", "contrast"] {
            save_change(&path, &Preferences::default(), &base);
            let mut local_json = base_json.clone();
            local_json["effects"]["values"][0]["Adjustment"]["BrightnessContrast"]["brightness"] =
                json!(20);
            local_json["effects"]["last"] = local_json["effects"]["values"][0].clone();
            let local: Preferences = serde_json::from_value(local_json).unwrap();
            let mut pending = Patch::default();
            pending.record(&base, &local).unwrap();
            pending.colors = Changes::color(0x123456);
            let mut other_json = base_json.clone();
            if other_effect == "invert" {
                other_json["effects"]["values"][3] = json!({"Adjustment":"Invert"});
                other_json["effects"]["last"] = json!({"Adjustment":"Invert"});
            } else {
                other_json["effects"]["values"][0]["Adjustment"]["BrightnessContrast"]["contrast"] =
                    json!(35);
                other_json["effects"]["last"] = other_json["effects"]["values"][0].clone();
            }
            let other: Preferences = serde_json::from_value(other_json).unwrap();
            save_change(&path, &base, &other);
            let saved = settings::save::<Preferences>(&path, &pending).unwrap();
            saved.preferences.validate().unwrap();
            let actual = serde_json::to_value(saved.preferences).unwrap();
            let expected = serde_json::to_value(&local).unwrap();
            assert_eq!(
                actual["effects"]["values"][0],
                expected["effects"]["values"][0]
            );
            assert_eq!(actual["effects"]["last"], expected["effects"]["last"]);
            if other_effect == "invert" {
                assert_eq!(
                    actual["effects"]["values"][3],
                    json!({"Adjustment":"Invert"})
                );
            }
            assert!(saved.colors.recent.contains(&0x123456));
        }
        // A repeated choice can change only `last`; use the winning slot's
        // parameters rather than retaining a stale duplicate in that reference.
        let mut before = base_json.clone();
        before["effects"]["values"][3] = json!({"Adjustment":"Invert"});
        before["effects"]["last"] = json!({"Adjustment":"Invert"});
        let mut after = before.clone();
        after["effects"]["last"] = after["effects"]["values"][0].clone();
        let before: Preferences = serde_json::from_value(before).unwrap();
        let after: Preferences = serde_json::from_value(after).unwrap();
        let mut repeated = Patch::default();
        repeated.record(&before, &after).unwrap();
        let saved = settings::save::<Preferences>(&path, &repeated).unwrap();
        saved.preferences.validate().unwrap();
        let actual = serde_json::to_value(saved.preferences).unwrap();
        assert_eq!(actual["effects"]["last"], actual["effects"]["values"][0]);
    }

    #[test]
    fn failed_reset_is_retryable_without_another_preference_change_and_defers_exit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let defaults = Preferences::default();
        save_change(&path, &defaults, &configured());
        let mut candidate = defaults.clone();
        let paths = candidate.reset_all_tools();
        let mut reset = Patch::default();
        reset.replace(&candidate, &paths).unwrap();
        let mut store = Storage {
            path: Some(path.clone()),
            pending: reset,
            ..Default::default()
        };
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.lock().unwrap();
        let first = store.take_write().unwrap();
        assert!(store.defer_exit(true));
        assert!(settings::save::<Preferences>(&path, &first).is_err());
        store.pending.colors.merge(&Changes::color(0x123456));
        store.failed_write(first);
        assert!(!store.timer && !store.saving);
        assert_eq!(store.exit_after_save, None);
        assert!(store.take_failed_exit()); // A second Close/Quit can bypass a permanent failure.
        assert!(!store.take_failed_exit()); // The bypass applies only to that explicit request.
        assert!(store.defer_exit(true));
        assert!(candidate == defaults); // No new local preference difference to record.
        drop(lock);
        let retry = store
            .take_write()
            .expect("explicit flush must drain the failed queue");
        assert!(store.take_write().is_none()); // Do not overlap an in-flight write.
        let saved = settings::save::<Preferences>(&path, &retry).unwrap();
        store.saving = false;
        assert!(!store.defer_exit(true));
        assert_eq!(saved.preferences.wand_sample_all, defaults.wand_sample_all);
        assert_eq!(saved.preferences.gradient_custom, None);
        assert!(saved.colors.recent.contains(&0x123456));
        let restarted = settings::load::<Preferences>(&path).unwrap();
        assert_eq!(restarted.preferences.grid_spacing, defaults.grid_spacing);
    }

    #[test]
    fn malformed_unknown_and_out_of_range_settings_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let base = json!({"version":1,"preferences":serde_json::to_value(Preferences::default()).unwrap()});
        let mut bad_files = Vec::new();
        for (field, value) in [
            ("tool", json!("Unknown")),
            ("grid_spacing", json!(0)),
            ("selection_feather_radius", json!(129)),
            ("toolbox_width", json!(99999)),
            ("pdf_export_dpi", json!(1201)),
            ("newer_field", json!(true)),
            ("gradient_use_custom", json!(true)),
        ] {
            let mut bad = base.clone();
            bad["preferences"][field] = value;
            bad_files.push(bad);
        }
        let mut bad = base.clone();
        bad["preferences"]["brushes"]["brush"]["diameter"] = json!(0);
        bad_files.push(bad);
        let mut bad = base.clone();
        bad["preferences"]["text_defaults"]["size"] = json!(999);
        bad_files.push(bad);
        let mut bad = base.clone();
        bad["version"] = json!(2);
        bad_files.push(bad);
        let mut bad = base.clone();
        bad["colors"] = json!({"swatches":[0,16777215],"recent":[1,1]});
        bad_files.push(bad);
        let mut bad = base.clone();
        let mut curve =
            serde_json::to_value(crate::adjustments::Adjustment::Curves(Default::default()))
                .unwrap();
        curve["Curves"]["red"]["count"] = json!(255);
        bad["preferences"]["effects"]["values"][6] = json!({"Adjustment":curve});
        bad_files.push(bad);
        let patch = Patch {
            colors: Changes::color(7),
            ..Default::default()
        };
        for bad in bad_files {
            let bytes = serde_json::to_vec(&bad).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert!(settings::load::<Preferences>(&path).is_err());
            assert!(settings::save::<Preferences>(&path, &patch).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        for bytes in [b"broken".to_vec(), vec![b' '; 128 * 1024 + 1]] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(settings::save::<Preferences>(&path, &patch).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn defaults_and_missing_new_fields_are_compatible_and_pending_io_defers_exit() {
        let preferences: Preferences = serde_json::from_value(json!({})).unwrap();
        assert!(preferences == Preferences::default());
        preferences.validate().unwrap();
        let mut store = Storage {
            loading: true,
            ..Default::default()
        };
        assert!(store.defer_exit(false));
        store.loading = false;
        store.timer = true;
        assert!(store.defer_exit(true));
        store.timer = false;
        store.saving = true;
        assert!(store.defer_exit(false));
        assert_eq!(store.exit_after_save, Some(true));
        store.saving = false;
        assert!(!store.defer_exit(false));
    }

    #[test]
    fn numeric_and_resize_drafts_stay_live_but_only_committed_preferences_save() {
        let original = Preferences::default();
        let mut live = configured();
        live.grid_spacing = 71;
        let committed = live.committed(
            &original,
            Some(super::super::numeric::NumericField::GridSpacing),
            true,
        );
        assert_eq!(committed.grid_spacing, original.grid_spacing);
        assert_eq!(committed.toolbox_width, original.toolbox_width);
        assert!(committed.clone_sample_all);
        // A late settings load keeps the numeric preview without putting it in
        // the write queue. Escape restores the original; Return records the edit.
        let mut confirmed = Patch::default();
        confirmed.record(&original, &committed).unwrap();
        let mut loaded = settings::Snapshot {
            preferences: original.clone(),
            colors: Default::default(),
        };
        confirmed.apply(&mut loaded).unwrap();
        let baseline = loaded.preferences.clone();
        let mut preview = Patch::default();
        preview.record(&committed, &live).unwrap();
        preview.apply(&mut loaded).unwrap();
        assert_eq!(loaded.preferences.grid_spacing, 71);
        assert_eq!(baseline.grid_spacing, 16);
        let mut accepted = Patch::default();
        accepted.record(&baseline, &loaded.preferences).unwrap();
        assert_eq!(accepted.preferences["grid_spacing"], json!(71));
        assert!(!confirmed.preferences.contains_key("grid_spacing"));
        assert!(!confirmed.preferences.contains_key("toolbox_width"));
    }

    #[test]
    fn lock_failures_preserve_disk_settings_and_a_retry_keeps_queued_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut first = Patch {
            colors: Changes::color(0x123456),
            ..Default::default()
        };
        settings::save::<Preferences>(&path, &first).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.lock().unwrap();
        first.preferences.insert("grid_spacing".into(), json!(33));
        assert!(settings::save::<Preferences>(&path, &first).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let newer = Patch {
            colors: Changes::swatch(0, 0xabcdef),
            ..Default::default()
        };
        first.merge(&newer);
        drop(lock);
        let result = settings::save::<Preferences>(&path, &first).unwrap();
        assert_eq!(result.preferences.grid_spacing, 33);
        assert_eq!(result.colors.swatches[0], 0xabcdef);
        assert_eq!(result.colors.recent, [0xabcdef, 0x123456]);
    }
}
