use super::{Workspace, settings_ui::Preferences};
use crate::commands::{ResetAllTools, ResetCurrentTool, TOOL_GROUPS, Tool};
use crate::settings::Patch;
use gpui::{Context, Div, PromptButton, PromptLevel, Window, prelude::*};

fn has_options(tool: Tool) -> bool {
    !matches!(tool, Tool::Pan | Tool::Zoom | Tool::Eyedropper)
}

impl Preferences {
    /// Only personal tool options belong here. This operation has no access to
    /// documents, color memory, gradient libraries, or active editing sessions.
    pub(super) fn reset_tool(&mut self, tool: Tool) -> Vec<&'static str> {
        let defaults = Self::default();
        let mut paths = Vec::new();
        macro_rules! reset {
            ($field:ident) => {{
                self.$field = defaults.$field;
                paths.push(stringify!($field));
            }};
        }
        match tool {
            Tool::Move => {
                reset!(snap_grid);
                reset!(snap_guides);
                reset!(grid_spacing);
            }
            Tool::Select | Tool::OvalSelect => {
                reset!(selection_ratio);
                reset!(selection_ratio_value);
                reset!(selection_antialias);
                reset!(oval_circle);
                if tool == Tool::Select {
                    reset!(selection_radius);
                }
            }
            Tool::FreeSelect => reset!(free_antialias),
            Tool::MagicWand => {
                reset!(wand_match);
                reset!(wand_sample_all);
                reset!(wand_antialias);
            }
            Tool::Crop => {
                reset!(crop_ratio);
                reset!(crop_ratio_value);
            }
            Tool::ScaleLayer => {
                reset!(scale_lock);
                reset!(scale_anchor);
                reset!(warp_resample);
            }
            Tool::Rotate | Tool::Flip | Tool::Perspective => {
                reset!(warp_resample)
            }
            Tool::Pencil
            | Tool::Brush
            | Tool::Eraser
            | Tool::Blur
            | Tool::Sharpen
            | Tool::CloneStamp => {
                paths.extend(self.brushes.reset_tool(tool));
                if tool == Tool::CloneStamp {
                    reset!(clone_aligned);
                    reset!(clone_sample_all);
                    reset!(clone_show_source);
                }
            }
            Tool::BucketFill => {
                reset!(fill_mode);
                reset!(fill_match);
                reset!(sample_all);
            }
            Tool::Gradient => {
                reset!(gradient_shape);
                reset!(gradient_blend);
                reset!(gradient_custom);
                reset!(gradient_use_custom);
                reset!(gradient_reverse);
                reset!(gradient_selected_stop);
                reset!(gradient_selected_preset);
            }
            Tool::Text => reset!(text_defaults),
            Tool::Path => {
                reset!(path_handle_mode);
                reset!(path_stroke_style);
                paths.extend(self.brushes.reset_tool(tool));
            }
            Tool::Pan | Tool::Zoom | Tool::Eyedropper => {}
        }
        if matches!(
            tool,
            Tool::Select | Tool::OvalSelect | Tool::FreeSelect | Tool::MagicWand
        ) {
            reset!(selection_feather_enabled);
            reset!(selection_feather_radius);
        }
        paths
    }

    pub(super) fn reset_all_tools(&mut self) -> Vec<&'static str> {
        let mut paths = Vec::new();
        for (_, tools) in TOOL_GROUPS {
            for &tool in *tools {
                paths.extend(self.reset_tool(tool));
            }
        }
        paths
    }
}

impl Workspace {
    pub(super) fn tool_reset_blocker(&self) -> Option<&'static str> {
        if self.settings_store.is_loading() {
            Some("Wait for saved settings to finish loading.")
        } else if self.form.is_some()
            || self.dialog_open
            || self.pdf_job.is_some()
            || self.creating
            || self.tabs.iter().any(|tab| tab.busy())
            || self.gradient_store.saving
        {
            Some("Finish the current dialog or background operation before resetting tools.")
        } else if self.numeric_edit.is_some()
            || self.numeric_followup.is_some()
            || self.text_draft.is_some()
            || self.toolbox_drag.is_some()
            || self.pan_drag.is_some()
            || self.pending_nudge.is_some()
            || self.pending_edit_name().is_some()
        {
            Some("Finish or cancel the current edit or preview before resetting tools.")
        } else {
            None
        }
    }

    pub(super) fn current_tool_reset_blocker(&self) -> Option<&'static str> {
        self.tool_reset_blocker().or_else(|| {
            (!has_options(self.preferences.tool)).then_some("This tool has no settings to reset.")
        })
    }

    pub(super) fn bind_tool_reset_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        root.when(self.current_tool_reset_blocker().is_none(), |root| {
            root.on_action(cx.listener(|this, _: &ResetCurrentTool, window, cx| {
                this.reset_tool_settings(false, window, cx);
            }))
        })
        .when(self.tool_reset_blocker().is_none(), |root| {
            root.on_action(cx.listener(|this, _: &ResetAllTools, window, cx| {
                this.confirm_reset_all_tools(window, cx);
            }))
        })
    }

    fn confirm_reset_all_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tool_reset_blocker().is_some() {
            return;
        }
        self.dialog_open = true;
        let prompt = window.prompt(
            PromptLevel::Warning,
            "Reset all tool settings to their defaults?",
            Some("Resets every tool, including shared options. Keeps artwork, colors, saved gradients, workspace layout, import/export preferences, and filter settings."),
            &[PromptButton::cancel("Cancel"), PromptButton::new("Reset All Tools")],
            cx,
        );
        cx.spawn_in(window, async move |workspace, cx| {
            let answer = prompt.await;
            let _ = workspace.update_in(cx, |this, window, cx| {
                this.dialog_open = false;
                if answer == Ok(1) {
                    this.reset_tool_settings(true, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn reset_tool_settings(&mut self, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.tool_reset_blocker().is_some() || (!all && !has_options(self.preferences.tool)) {
            return;
        }
        let mut candidate = self.preferences.clone();
        let paths = if all {
            candidate.reset_all_tools()
        } else {
            candidate.reset_tool(self.preferences.tool)
        };
        let mut reset = Patch::default();
        if let Err(error) = reset.replace(&candidate, &paths) {
            self.errors
                .push(format!("Could not reset tool settings: {error:#}"));
            return;
        }
        // Preserve other changes before the explicit reset replaces its entire
        // scope, including queued leaves and locally unchanged default values.
        self.sync_settings(window, cx);
        self.queue_settings_patch(&reset);
        // Use Clone's existing source/alignment invalidation rules. Raster data
        // and the chosen source remain intact when its policy is unchanged.
        self.set_clone_sampling(candidate.clone_sample_all, cx);
        self.set_clone_aligned(candidate.clone_aligned, cx);
        self.preferences = candidate;
        if all || self.preferences.tool == Tool::Gradient {
            self.gradient_revision = self.gradient_revision.wrapping_add(1);
        }
        self.flush_settings(window, cx);
        self.notice(
            if all {
                "All tool settings reset to defaults."
            } else {
                "Tool defaults reset. Existing selections and layers are unchanged."
            },
            cx,
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::super::settings_ui::tests::configured;
    use super::*;
    use crate::{
        color_memory::Changes,
        settings::{self, Patch},
    };
    use serde_json::json;

    #[test]
    fn current_paint_reset_leaves_the_other_tools_and_options_intact() {
        let before = configured();
        let defaults = serde_json::to_value(Preferences::default()).unwrap();
        for (tool, brush, smoothing) in [
            (Tool::Pencil, "pencil", None),
            (Tool::Brush, "brush", Some(0)),
            (Tool::Eraser, "eraser", Some(1)),
            (Tool::Blur, "blur", Some(2)),
            (Tool::Sharpen, "sharpen", Some(3)),
            (Tool::CloneStamp, "clone_stamp", Some(4)),
        ] {
            let mut preferences = before.clone();
            preferences.reset_tool(tool);
            let mut expected = serde_json::to_value(&before).unwrap();
            expected["brushes"][brush] = defaults["brushes"][brush].clone();
            if let Some(index) = smoothing {
                expected["brushes"]["smoothing"][index] = json!("Off");
            }
            if tool == Tool::CloneStamp {
                for key in ["clone_aligned", "clone_sample_all", "clone_show_source"] {
                    expected[key] = defaults[key].clone();
                }
            }
            assert_eq!(
                serde_json::to_value(preferences).unwrap(),
                expected,
                "{tool:?}"
            );
        }
    }

    #[test]
    fn selection_resets_share_only_the_existing_shared_preferences() {
        let mut preferences = configured();
        let before = preferences.clone();
        preferences.reset_tool(Tool::Select);
        assert_eq!(preferences.selection_radius, 0);
        assert!(!preferences.selection_antialias);
        assert_eq!(preferences.selection_ratio, None);
        assert!(!preferences.selection_feather_enabled);
        assert_eq!(preferences.selection_feather_radius, 8);
        assert_eq!(preferences.oval_circle, before.oval_circle);
        assert_eq!(preferences.wand_match, before.wand_match);
        assert_eq!(preferences.free_antialias, before.free_antialias);
        assert_eq!(preferences.crop_ratio, before.crop_ratio);
        preferences.reset_tool(Tool::MagicWand);
        assert!(!preferences.wand_antialias);
        assert!(!preferences.wand_sample_all);
        assert_eq!(preferences.wand_match, Default::default());
        assert_eq!(preferences.fill_match, before.fill_match);
        assert_eq!(preferences.sample_all, before.sample_all);
    }

    #[test]
    fn all_tools_reset_preserves_layout_active_tool_exports_and_effect_memory() {
        let before = configured();
        let mut preferences = before.clone();
        preferences.reset_all_tools();
        let actual = serde_json::to_value(&preferences).unwrap();
        let mut expected = serde_json::to_value(Preferences::default()).unwrap();
        let original = serde_json::to_value(before).unwrap();
        for key in [
            "tool",
            "show_grid",
            "show_guides",
            "show_layer_bounds",
            "inspector_tab",
            "toolbox_width",
            "toolbox_folded",
            "move_snapping_expanded",
            "gradient_presets_expanded",
            "tool_sections",
            "pdf_preferences",
            "jpeg_quality",
            "pdf_export_dpi",
            "effects",
        ] {
            expected[key] = original[key].clone();
        }
        assert_eq!(actual, expected);
        // Move's tool options reset snapping/spacing, without touching overlays.
        assert_eq!(preferences.grid_spacing, 16);
        assert!(!preferences.snap_grid && !preferences.snap_guides);
    }

    #[test]
    fn utility_tools_have_no_reset_and_current_crop_does_not_reset_selection_options() {
        let before = configured();
        for tool in [Tool::Pan, Tool::Zoom, Tool::Eyedropper] {
            let mut preferences = before.clone();
            preferences.reset_tool(tool);
            assert!(!has_options(tool));
            assert!(preferences == before);
        }
        let mut preferences = before.clone();
        preferences.reset_tool(Tool::Crop);
        assert_eq!(preferences.crop_ratio, None);
        assert_eq!(
            preferences.crop_ratio_value,
            crate::constraints::AspectRatio::SQUARE
        );
        assert_eq!(preferences.selection_ratio, before.selection_ratio);
        assert_eq!(preferences.selection_radius, before.selection_radius);
    }

    #[test]
    fn explicit_resets_include_locally_default_values_changed_by_another_instance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let defaults = Preferences::default();
        let remote = configured();
        let scopes = TOOL_GROUPS
            .iter()
            .flat_map(|(_, tools)| tools.iter().copied().map(Some))
            .chain(std::iter::once(None));
        for scope in scopes {
            let mut initial = Patch {
                colors: Changes::color(0x123456),
                ..Default::default()
            };
            initial.record(&defaults, &remote).unwrap();
            settings::save::<Preferences>(&path, &initial).unwrap();
            let mut candidate = defaults.clone();
            let mut expected = remote.clone();
            let paths = if let Some(tool) = scope {
                expected.reset_tool(tool);
                candidate.reset_tool(tool)
            } else {
                expected.reset_all_tools();
                candidate.reset_all_tools()
            };
            let mut reset = Patch::default();
            reset.replace(&candidate, &paths).unwrap();
            // Include an older queued leaf edit so an explicit reset must
            // supersede it rather than letting it override the reset later.
            let mut pending = Patch::default();
            if scope == Some(Tool::Brush) || scope.is_none() {
                pending
                    .preferences
                    .insert("brushes/brush/diameter".into(), json!(93));
            }
            pending.merge(&reset);
            let saved = settings::save::<Preferences>(&path, &pending).unwrap();
            assert!(saved.preferences == expected, "reset scope {scope:?}");
            assert!(saved.colors.recent.contains(&0x123456));
            let restarted = settings::load::<Preferences>(&path).unwrap();
            assert!(restarted.preferences == expected, "restart scope {scope:?}");
        }
    }

    #[test]
    fn resets_survive_restart_and_preserve_colors_and_saved_gradients() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let gradients = dir.path().join("gradients.json");
        let before = configured();
        let mut library = crate::gradient::library::Library::default();
        library
            .insert(
                "Personal",
                crate::gradient::library::PresetValue {
                    ramp: before.gradient_custom.unwrap(),
                    shape: before.gradient_shape,
                    reverse: before.gradient_reverse,
                },
            )
            .unwrap();
        crate::gradient::library::save(&gradients, None, &library).unwrap();
        let library_bytes = std::fs::read(&gradients).unwrap();
        let mut first = Patch {
            colors: Changes::swatch(0, 0x123456),
            ..Default::default()
        };
        first.colors.merge(&Changes::swatch(1, 0xabcdef));
        first.record(&Preferences::default(), &before).unwrap();
        let original = settings::save::<Preferences>(&path, &first).unwrap();
        for all in [false, true] {
            let mut candidate = before.clone();
            let paths = if all {
                candidate.reset_all_tools()
            } else {
                candidate.reset_tool(Tool::Gradient)
            };
            let mut patch = Patch::default();
            patch.replace(&candidate, &paths).unwrap();
            settings::save::<Preferences>(&path, &patch).unwrap();
            let loaded = settings::load::<Preferences>(&path).unwrap();
            assert_eq!(loaded.colors, original.colors);
            assert_eq!(std::fs::read(&gradients).unwrap(), library_bytes);
            assert_eq!(loaded.preferences.gradient_custom, None);
            assert!(!loaded.preferences.gradient_use_custom);
            // Each loop models an independent user starting from the same settings.
            settings::save::<Preferences>(&path, &first).unwrap();
        }
    }
}
