use gpui::{Context, Div, Pixels, Point, Window, div, point, prelude::*, px, rgb};

use super::Workspace;
use super::canvas_tools::PreviewGeometry;
use super::numeric::NumericField;
use super::stroke_ui::{PathStrokeStyle, StrokeSource};
use super::tooltip::{help_icon, tooltip};
use super::view::{
    DISABLED, HOVER, MUTED, SELECTED, TEXT, choice_button, icon, option_action, option_row,
    section_label, segmented, step_button,
};
use crate::commands::{PathToSelection, SelectionToPath};
use crate::paths::{self, Anchor, AnchorId, HandleMode, PathId, VectorPath};
use crate::preview::Dirty;

const HIT_PIXELS: f32 = 8.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AnchorSelection {
    pub path: PathId,
    pub ids: Vec<AnchorId>,
}

impl AnchorSelection {
    fn single(path: PathId, anchor: AnchorId) -> Self {
        Self {
            path,
            ids: vec![anchor],
        }
    }

    fn click(selection: &mut Option<Self>, path: PathId, anchor: AnchorId, toggle: bool) {
        if let Some(selected) = selection.as_mut().filter(|selected| selected.path == path) {
            if let Some(index) = selected.ids.iter().position(|id| *id == anchor) {
                if toggle {
                    selected.ids.remove(index);
                    if selected.ids.is_empty() {
                        *selection = None;
                    }
                }
                return;
            }
            if toggle {
                selected.ids.push(anchor);
                return;
            }
        }
        *selection = Some(Self::single(path, anchor));
    }

    fn anchors(&self, path: &VectorPath) -> Vec<Anchor> {
        if path.id != self.path || !path.visible {
            return Vec::new();
        }
        path.anchors
            .iter()
            .filter(|anchor| self.ids.contains(&anchor.id))
            .copied()
            .collect()
    }
}

pub(super) struct PathGesture {
    pub path: VectorPath,
    baseline: Option<VectorPath>,
    edit: PathEdit,
}

enum PathEdit {
    Place {
        anchor: AnchorId,
        origin: (f64, f64),
    },
    MovePoints {
        anchor: AnchorId,
        bases: Vec<Anchor>,
        down: (f64, f64),
    },
    MoveHandle {
        anchor: AnchorId,
        incoming: bool,
        grab: (f64, f64),
        mode: HandleMode,
    },
}

impl PathGesture {
    fn update(&mut self, point: (f64, f64)) {
        match &self.edit {
            PathEdit::Place { anchor, origin } => {
                let Some(anchor) = self.path.anchor_mut(*anchor) else {
                    return;
                };
                let delta = (point.0 - origin.0, point.1 - origin.1);
                anchor.handle_out = point;
                anchor.handle_in = (origin.0 - delta.0, origin.1 - delta.1);
            }
            PathEdit::MovePoints { bases, down, .. } => {
                self.path
                    .move_anchors_from(bases, (point.0 - down.0, point.1 - down.1));
            }
            PathEdit::MoveHandle {
                anchor,
                incoming,
                grab,
                mode,
            } => {
                let next = (point.0 + grab.0, point.1 + grab.1);
                let Some(anchor) = self.path.anchor_mut(*anchor) else {
                    return;
                };
                let Some(base) = self
                    .baseline
                    .as_ref()
                    .and_then(|path| path.anchor(anchor.id))
                    .copied()
                else {
                    return;
                };
                *anchor = base;
                anchor.move_handle(*incoming, next, *mode);
            }
        }
    }

    fn close_on_click(&mut self, point: (f64, f64), radius: f64) {
        if let PathEdit::MovePoints {
            anchor,
            down,
            bases,
        } = &self.edit
            && bases.len() == 1
            && let Some(base) = self.baseline.as_ref()
            && !base.closed
            && base.anchors.len() >= 3
            && base.anchors[0].id == *anchor
            && (point.0 - down.0).hypot(point.1 - down.1) <= radius
        {
            self.path = base.clone();
            self.path.closed = true;
        }
    }
}

enum Target {
    Handle {
        path: PathId,
        anchor: AnchorId,
        incoming: bool,
        at: (f64, f64),
    },
    Anchor {
        path: PathId,
        anchor: AnchorId,
    },
}

impl Workspace {
    pub(super) fn path_edit_options(&self, cx: &Context<Self>) -> Div {
        let count = self
            .selected_path_anchors()
            .map_or(0, |(_, anchors)| anchors.len());
        let ready = self.path_gesture.is_none() && self.active_tab().is_some_and(|tab| !tab.busy());
        let enabled = ready && count > 0;
        let has_path = ready
            && self.active_tab().is_some_and(|tab| {
                tab.document
                    .content
                    .active_path()
                    .is_some_and(|path| path.visible && !path.anchors.is_empty())
            });
        div().flex().flex_col().gap_2()
            .child(section_label("HANDLE DRAG"))
            .child(segmented([
                ("Independent", HandleMode::Independent), ("Aligned", HandleMode::Aligned), ("Symmetric", HandleMode::Symmetric),
            ].map(|(label, mode)| choice_button(label, self.path_handle_mode == mode).on_click(cx.listener(move |this, _, _, cx| {
                if this.path_gesture.is_none() { this.path_handle_mode = mode; cx.notify(); }
            })))))
            .child(div().flex().justify_between().child(section_label("SELECTED ANCHORS")).child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(count.to_string())))
            .child(div().flex().gap_2().children([
                ("All Anchors", true, has_path), ("Deselect", false, enabled),
            ].map(|(label, all, available)| option_action(label, false).flex_1()
                .when(available, |button| button.on_click(cx.listener(move |this, _, _, cx| this.select_path_anchors(all, cx))))
                .when(!available, |button| button.opacity(0.45).cursor(gpui::CursorStyle::Arrow)))))
            .child(div().flex().gap_2().children([
                ("Corner", false, "Collapse both handles at the selected anchors"),
                ("Smooth", true, "Align handles, preserving their lengths or creating them from neighboring points"),
            ].map(|(label, smooth, help)| option_action(label, false).flex_1().tooltip(tooltip(help))
                .when(enabled, |button| button.on_click(cx.listener(move |this, _, window, cx| this.shape_selected_anchor(smooth, window, cx))))
                .when(!enabled, |button| button.opacity(0.45).cursor(gpui::CursorStyle::Arrow)))))
    }

    fn selected_path_anchors(&self) -> Option<(PathId, Vec<Anchor>)> {
        let selected = self.path_selection.as_ref()?;
        let tab = self.active_tab()?;
        let path = self
            .path_gesture
            .as_ref()
            .map(|gesture| &gesture.path)
            .or(self.path_hold.as_ref())
            .or_else(|| tab.document.content.active_path())?;
        let anchors = selected.anchors(path);
        (!anchors.is_empty()).then_some((path.id, anchors))
    }

    fn select_path_anchors(&mut self, all: bool, cx: &mut Context<Self>) {
        if self.path_gesture.is_some() || self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        self.path_selection = if all {
            self.active_tab()
                .and_then(|tab| tab.document.content.active_path())
                .filter(|path| path.visible && !path.anchors.is_empty())
                .map(|path| AnchorSelection {
                    path: path.id,
                    ids: path.anchors.iter().map(|a| a.id).collect(),
                })
        } else {
            None
        };
        cx.notify();
    }

    fn shape_selected_anchor(&mut self, smooth: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_gesture.is_some() || self.form.is_some() || self.dialog_open {
            return;
        }
        let Some((id, anchors)) = self.selected_path_anchors() else {
            return;
        };
        let Some(tab) = self.active_tab().filter(|tab| !tab.busy()) else {
            return;
        };
        let Some(mut path) = tab
            .document
            .content
            .active_path()
            .filter(|path| path.id == id && path.visible)
            .cloned()
        else {
            return;
        };
        let mut changed = false;
        for anchor in anchors {
            changed |= if smooth {
                path.make_smooth(anchor.id)
            } else {
                path.make_corner(anchor.id)
            };
        }
        if !changed {
            return;
        }
        self.path_hold = Some(path.clone());
        self.edit_content(
            move |content, _, _| {
                if content.upsert_path(path) {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn bind_path_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        let available = self.active_tab().is_some_and(|tab| {
            !tab.busy()
                && tab
                    .document
                    .content
                    .paths()
                    .iter()
                    .any(|path| path.closed && path.anchors.len() >= 3)
        });
        let selection = self.active_tab().is_some_and(|tab| {
            !tab.busy()
                && tab.document.content.paths().len() < crate::paths::MAX_PATHS
                && !matches!(
                    tab.selection,
                    crate::selection::Selection::Unrestricted | crate::selection::Selection::Empty
                )
        });
        root.when(available, |root| {
            root.on_action(cx.listener(|this, _: &PathToSelection, window, cx| {
                this.path_to_selection(window, cx);
            }))
        })
        .when(selection, |root| {
            root.on_action(cx.listener(|this, _: &SelectionToPath, window, cx| {
                this.selection_to_path(window, cx);
            }))
        })
    }

    pub(super) fn path_panel(&self, cx: &Context<Self>) -> Div {
        self.path_options(div().flex().flex_col().gap_3().p_4(), cx)
    }

    pub(super) fn path_options(&self, panel: Div, cx: &Context<Self>) -> Div {
        let tab = self.active_tab();
        let paths = tab.map_or(&[][..], |tab| tab.document.content.paths());
        let active = tab.and_then(|tab| tab.document.content.active_path_id());
        let rows = paths.iter().enumerate().map(|(index, path)| {
            let id = path.id;
            let visible = path.visible;
            let selected = active == Some(id);
            div()
                .id(("path-row", id.0))
                .flex()
                .items_center()
                .gap_1()
                .px_1()
                .py_1()
                .rounded_md()
                .when(selected, |row| row.bg(rgb(SELECTED)))
                .hover(|row| row.bg(rgb(HOVER)))
                .cursor_pointer()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(rgb(if visible { TEXT } else { MUTED }))
                        .child(format!(
                            "Path {} · {} anchors",
                            index + 1,
                            path.anchors.len()
                        )),
                )
                .child(
                    div()
                        .id(("path-eye", id.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(22.))
                        .flex_shrink_0()
                        .rounded_sm()
                        .text_color(rgb(if visible { MUTED } else { DISABLED }))
                        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                        .tooltip(tooltip(if visible { "Hide path" } else { "Show path" }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.set_path_visible(id, !visible, window, cx);
                        }))
                        .child(icon(if visible { "eye" } else { "eye-off" }, 14.)),
                )
                .child(
                    div()
                        .id(("path-delete", id.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(22.))
                        .flex_shrink_0()
                        .rounded_sm()
                        .text_color(rgb(MUTED))
                        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                        .tooltip(tooltip("Delete path"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.delete_path(id, window, cx);
                        }))
                        .child(icon("trash", 14.)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_path(id, cx);
                }))
        });
        let enabled = tab.is_some_and(|tab| {
            !tab.busy()
                && tab
                    .document
                    .content
                    .active_path()
                    .is_some_and(|path| path.visible && path.anchors.len() >= 2)
                && tab
                    .document
                    .content
                    .layer(tab.active_layer)
                    .is_some_and(|layer| layer.visible && layer.text.is_none())
                && self.brushes.outline.opacity > 0
        });
        let style = self.path_stroke_style;
        let selection = tab.is_some_and(|tab| {
            !tab.busy()
                && tab.document.content.paths().len() < crate::paths::MAX_PATHS
                && !matches!(
                    tab.selection,
                    crate::selection::Selection::Unrestricted | crate::selection::Selection::Empty
                )
        });
        panel
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_label("PATHS"))
                    .child(help_icon(
                        "paths-help",
                        "Paths",
                        crate::commands::Tool::Path.help(),
                    )),
            )
            .child(div().flex().flex_col().gap_1().children(rows))
            .when(paths.is_empty(), |panel| {
                panel.child(div().text_color(rgb(MUTED)).child("No paths yet."))
            })
            .child(
                option_action("Path to Selection", false)
                    .w_full()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.path_to_selection(window, cx);
                    })),
            )
            .child(
                option_action("Selection to Path", false)
                    .w_full()
                    .when(selection, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.selection_to_path(window, cx);
                        }))
                    })
                    .when(!selection, |button| {
                        button.opacity(0.45).cursor(gpui::CursorStyle::Arrow)
                    }),
            )
            .child(section_label("STROKE"))
            .child(segmented([
                choice_button("Pencil", style == PathStrokeStyle::Pencil).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.set_path_stroke_style(PathStrokeStyle::Pencil, cx);
                    },
                )),
                choice_button("Brush", style == PathStrokeStyle::Brush).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.set_path_stroke_style(PathStrokeStyle::Brush, cx);
                    },
                )),
            ]))
            .child(option_row(
                "Size",
                self.numeric_stepper(
                    NumericField::PathStrokeSize,
                    step_button("path-stroke-down", "−").on_click(cx.listener(|this, _, _, cx| {
                        this.brushes.outline.diameter =
                            this.brushes.outline.diameter.saturating_sub(1).max(1);
                        cx.notify();
                    })),
                    step_button("path-stroke-up", "+").on_click(cx.listener(|this, _, _, cx| {
                        this.brushes.outline.diameter =
                            this.brushes.outline.diameter.saturating_add(1).min(500);
                        cx.notify();
                    })),
                    cx,
                ),
            ))
            .child(
                option_action("Stroke", enabled)
                    .w_full()
                    .when(enabled, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.stroke_outline(StrokeSource::Path, window, cx);
                        }))
                    })
                    .when(!enabled, |button| {
                        button.opacity(0.45).cursor(gpui::CursorStyle::Arrow)
                    }),
            )
    }

    fn select_path(&mut self, id: PathId, cx: &mut Context<Self>) {
        if let Some(index) = self.active
            && !self.tabs[index].busy()
            && self.tabs[index].document.content.select_path(id)
        {
            self.path_selection = None;
            self.path_gesture = None;
            self.path_hold = None;
            self.tabs[index].notice = None;
            cx.notify();
        }
    }

    fn set_path_visible(
        &mut self,
        id: PathId,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !visible
            && self
                .path_selection
                .as_ref()
                .is_some_and(|selected| selected.path == id)
            && self.active_tab().is_some_and(|tab| !tab.busy())
        {
            self.clear_path_edit();
        }
        self.edit_content(
            move |content, _, _| {
                if content.set_path_visible(id, visible) {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    fn delete_path(&mut self, id: PathId, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .path_selection
            .as_ref()
            .is_some_and(|selected| selected.path == id)
            && self.active_tab().is_some_and(|tab| !tab.busy())
        {
            self.clear_path_edit();
        }
        self.edit_content(
            move |content, _, _| {
                if content.delete_path(id) {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn displayed_paths(&self) -> Vec<VectorPath> {
        let Some(tab) = self.active_tab() else {
            return Vec::new();
        };
        let mut paths: Vec<_> = tab
            .document
            .content
            .paths()
            .iter()
            .filter(|path| path.visible)
            .cloned()
            .collect();
        let overlay = self
            .path_gesture
            .as_ref()
            .map(|gesture| &gesture.path)
            .or_else(|| self.path_hold.as_ref().filter(|hold| !paths.contains(hold)));
        if let Some(overlay) = overlay {
            if let Some(existing) = paths.iter_mut().find(|path| path.id == overlay.id) {
                *existing = overlay.clone();
            } else {
                paths.push(overlay.clone());
            }
        }
        paths
    }

    pub(super) fn displayed_active_path(&self) -> Option<PathId> {
        self.path_gesture
            .as_ref()
            .map(|gesture| gesture.path.id)
            .or_else(|| self.active_tab()?.document.content.active_path_id())
    }

    pub(super) fn begin_path(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        insert: bool,
        toggle: bool,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) || self.path_gesture.is_some() {
            return;
        }
        let Some(point) = self.canvas_point(position, scale) else {
            return;
        };
        if let Some(target) = self.path_target(position, scale) {
            let path_id = match target {
                Target::Handle { path, .. } | Target::Anchor { path, .. } => path,
            };
            let Some(path) = self
                .active_tab()
                .and_then(|tab| {
                    tab.document
                        .content
                        .paths()
                        .iter()
                        .find(|path| path.id == path_id)
                })
                .cloned()
            else {
                return;
            };
            let anchor_id = match target {
                Target::Handle { anchor, .. } | Target::Anchor { anchor, .. } => anchor,
            };
            let toggle_anchor = toggle && matches!(target, Target::Anchor { .. });
            AnchorSelection::click(&mut self.path_selection, path.id, anchor_id, toggle_anchor);
            if let Some(index) = self.active {
                self.tabs[index].document.content.select_path(path.id);
            }
            if toggle_anchor {
                cx.notify();
                return;
            }
            let edit = match target {
                Target::Handle {
                    anchor,
                    incoming,
                    at,
                    ..
                } => PathEdit::MoveHandle {
                    anchor,
                    incoming,
                    grab: (at.0 - point.0, at.1 - point.1),
                    mode: self.path_handle_mode,
                },
                Target::Anchor { anchor, .. } => {
                    let bases = self
                        .path_selection
                        .as_ref()
                        .map(|selected| selected.anchors(&path))
                        .unwrap_or_default();
                    PathEdit::MovePoints {
                        anchor,
                        bases,
                        down: point,
                    }
                }
            };
            self.path_gesture = Some(PathGesture {
                baseline: Some(path.clone()),
                path,
                edit,
            });
            cx.notify();
            return;
        }
        if toggle && !insert {
            return;
        }
        if insert {
            let Some(tab) = self.active_tab() else {
                return;
            };
            let active = tab.document.content.active_path_id();
            let radius = self.path_hit_radius(scale);
            let mut nearest = None;
            for path in tab
                .document
                .content
                .paths()
                .iter()
                .filter(|path| path.visible && Some(path.id) == active)
                .chain(
                    tab.document
                        .content
                        .paths()
                        .iter()
                        .filter(|path| path.visible && Some(path.id) != active),
                )
            {
                if let Some(hit) = path.nearest_segment(point, radius)
                    && nearest.as_ref().is_none_or(
                        |(_, best): &(&VectorPath, paths::SegmentHit)| hit.distance < best.distance,
                    )
                {
                    nearest = Some((path, hit));
                }
            }
            let Some((baseline, hit)) = nearest else {
                self.notice("Alt-click a visible curve to insert an anchor.", cx);
                return;
            };
            if baseline.anchors.len() >= paths::MAX_ANCHORS {
                self.notice("This path has enough anchors.", cx);
                return;
            }
            let Some(id) = tab.document.content.next_anchor_id() else {
                return;
            };
            let baseline = baseline.clone();
            let mut path = baseline.clone();
            let Some(anchor) = path.split_segment(hit.segment, hit.t, id) else {
                return;
            };
            self.path_selection = Some(AnchorSelection::single(path.id, id));
            self.path_gesture = Some(PathGesture {
                baseline: Some(baseline),
                path,
                edit: PathEdit::MovePoints {
                    anchor: id,
                    bases: vec![anchor],
                    down: point,
                },
            });
            cx.notify();
            return;
        }
        let Some((anchor_id, path_id, path_count, appending)) = self.active_tab().map(|tab| {
            let content = &tab.document.content;
            (
                content.next_anchor_id(),
                content.next_path_id(),
                content.paths().len(),
                content.active_path().filter(|path| !path.closed).cloned(),
            )
        }) else {
            return;
        };
        let (Some(anchor_id), Some(path_id)) = (anchor_id, path_id) else {
            return;
        };
        if let Some(active) = appending {
            if active.anchors.len() >= paths::MAX_ANCHORS {
                self.notice("This path has enough anchors.", cx);
                return;
            }
            let mut path = active.clone();
            path.anchors.push(Anchor::corner(anchor_id, point));
            self.path_selection = Some(AnchorSelection::single(path.id, anchor_id));
            self.path_gesture = Some(PathGesture {
                baseline: Some(active),
                edit: PathEdit::Place {
                    anchor: anchor_id,
                    origin: point,
                },
                path,
            });
        } else if path_count >= paths::MAX_PATHS {
            self.notice("The document has enough paths.", cx);
            return;
        } else {
            let path = VectorPath {
                id: path_id,
                visible: true,
                closed: false,
                anchors: vec![Anchor::corner(anchor_id, point)],
            };
            self.path_selection = Some(AnchorSelection::single(path_id, anchor_id));
            self.path_gesture = Some(PathGesture {
                baseline: None,
                edit: PathEdit::Place {
                    anchor: anchor_id,
                    origin: point,
                },
                path,
            });
        }
        cx.notify();
    }

    pub(super) fn extend_path(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(point) = self.canvas_point(position, scale) else {
            return;
        };
        let Some(gesture) = self.path_gesture.as_mut() else {
            return;
        };
        gesture.update(point);
        cx.notify();
    }

    pub(super) fn finish_path(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.extend_path(position, scale, cx);
        let Some(mut gesture) = self.path_gesture.take() else {
            return;
        };
        if let Some(point) = self.canvas_point(position, scale) {
            gesture.close_on_click(point, self.path_hit_radius(scale));
        }
        let path = gesture.path;
        if gesture.baseline.as_ref() == Some(&path)
            && self
                .active_document()
                .is_some_and(|doc| doc.content.active_path_id() == Some(path.id))
        {
            return;
        }
        self.path_hold = Some(path.clone());
        self.edit_content(
            move |content, _, _| {
                if content.upsert_path(path) {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn end_path_session(&mut self, cx: &mut Context<Self>) {
        let gesture = self.path_gesture.take().is_some();
        let hold = self.path_hold.take().is_some();
        let selection = self.path_selection.take().is_some();
        let active = self
            .active
            .is_some_and(|index| self.tabs[index].document.content.clear_active_path());
        if gesture || hold || selection || active {
            cx.notify();
        }
    }

    pub(super) fn cancel_path_gesture(&mut self, cx: &mut Context<Self>) {
        if self.path_gesture.take().is_some() {
            // A canceled insertion/placement has no live anchor. Keep only a
            // selection that still belongs to the document, before IDs can be reused.
            self.path_selection = self.path_selection.take().and_then(|mut selected| {
                let path = self
                    .active_tab()?
                    .document
                    .content
                    .paths()
                    .iter()
                    .find(|path| path.id == selected.path && path.visible)?;
                selected.ids.retain(|id| path.anchor(*id).is_some());
                (!selected.ids.is_empty()).then_some(selected)
            });
            cx.notify();
        }
    }

    pub(super) fn clear_path_edit(&mut self) {
        self.path_gesture = None;
        self.path_hold = None;
        self.path_selection = None;
    }

    pub(super) fn close_active_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut gesture) = self.path_gesture.take() {
            if gesture.path.anchors.len() < 3 {
                self.path_gesture = Some(gesture);
                return;
            }
            gesture.path.closed = true;
            let path = gesture.path;
            self.path_hold = Some(path.clone());
            self.edit_content(
                move |content, _, _| {
                    if content.upsert_path(path) {
                        Dirty::Unpainted
                    } else {
                        Dirty::None
                    }
                },
                window,
                cx,
            );
            return;
        }
        let Some(id) = self.displayed_active_path() else {
            return;
        };
        self.edit_content(
            move |content, _, _| {
                if content.close_path(id) {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn delete_path_anchor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        self.path_gesture = None;
        let Some(tab) = self.active_tab() else {
            return;
        };
        let (path_id, anchor_ids) = if self.path_selection.is_some() {
            let Some((path, anchors)) = self.selected_path_anchors() else {
                self.path_selection = None;
                cx.notify();
                return;
            };
            (
                path,
                anchors.iter().map(|anchor| anchor.id).collect::<Vec<_>>(),
            )
        } else if let Some(path) = tab
            .document
            .content
            .active_path()
            .filter(|path| path.visible)
        {
            let Some(anchor) = path.anchors.last() else {
                return;
            };
            (path.id, vec![anchor.id])
        } else {
            return;
        };
        self.path_selection = None;
        self.edit_content(
            move |content, _, _| {
                if content.delete_anchors(path_id, &anchor_ids) {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn selection_to_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().is_none_or(|tab| {
            tab.busy()
                || tab.document.content.paths().len() >= crate::paths::MAX_PATHS
                || matches!(
                    tab.selection,
                    crate::selection::Selection::Unrestricted | crate::selection::Selection::Empty
                )
        }) {
            return;
        }
        self.edit_content(
            |content, _, selection| {
                let paths = content.paths_from_selection(selection);
                let mut changed = false;
                for path in paths {
                    if !content.upsert_path(path) {
                        break;
                    }
                    changed = true;
                }
                if changed {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn path_to_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        self.start_change(
            super::Change::Selection,
            |content, _, selection| {
                let next = content.path_selection();
                if *selection == next {
                    return Dirty::None;
                }
                *selection = next;
                Dirty::Unpainted
            },
            window,
            cx,
        );
    }

    fn path_hit_radius(&self, scale: f32) -> f64 {
        let Some(tab) = self.active_tab() else {
            return 6.0;
        };
        let Some(geometry) = PreviewGeometry::for_tab(tab, self.viewport.get(), scale) else {
            return 6.0;
        };
        let width = f64::from(tab.document.width());
        let screen = f64::from(geometry.bounds.size.width);
        if screen <= 0.0 {
            return 6.0;
        }
        f64::from(HIT_PIXELS) * width / screen
    }

    fn path_target(&self, position: Point<Pixels>, scale: f32) -> Option<Target> {
        let tab = self.active_tab()?;
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let (width, height) = tab.document.content.size();
        if width == 0 || height == 0 {
            return None;
        }
        let paths: Vec<_> = tab
            .document
            .content
            .paths()
            .iter()
            .filter(|path| path.visible)
            .cloned()
            .collect();
        let active = tab.document.content.active_path_id();
        let paths = paths.as_slice();
        let mut ordered: Vec<&VectorPath> = paths
            .iter()
            .filter(|path| Some(path.id) == active)
            .collect();
        ordered.extend(paths.iter().filter(|path| Some(path.id) != active));
        let near = |at: (f64, f64)| -> bool {
            let projected = geometry.project(point(
                (at.0 / f64::from(width)) as f32,
                (at.1 / f64::from(height)) as f32,
            ));
            let dx = f32::from(projected.x - position.x);
            let dy = f32::from(projected.y - position.y);
            dx * dx + dy * dy <= HIT_PIXELS * HIT_PIXELS
        };
        for path in &ordered {
            for anchor in &path.anchors {
                if anchor.handle_in != anchor.point && near(anchor.handle_in) {
                    return Some(Target::Handle {
                        path: path.id,
                        anchor: anchor.id,
                        incoming: true,
                        at: anchor.handle_in,
                    });
                }
                if anchor.handle_out != anchor.point && near(anchor.handle_out) {
                    return Some(Target::Handle {
                        path: path.id,
                        anchor: anchor.id,
                        incoming: false,
                        at: anchor.handle_out,
                    });
                }
            }
        }
        for path in &ordered {
            for anchor in &path.anchors {
                if near(anchor.point) {
                    return Some(Target::Anchor {
                        path: path.id,
                        anchor: anchor.id,
                    });
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Change, Tab, history::State};
    use super::*;

    fn publish(tab: &mut Tab, state: State, change: Change) -> bool {
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            Dirty::Unpainted,
        )
        .unwrap();
        tab.publish_change(
            tab.history.generation(),
            change,
            Some((state.content, state.active_layer, state.selection, patch)),
            1,
        )
        .0
    }

    fn selection_path() -> VectorPath {
        VectorPath {
            id: PathId(1),
            visible: true,
            closed: true,
            anchors: vec![
                Anchor::corner(AnchorId(1), (0., 0.)),
                Anchor::corner(AnchorId(2), (2., 0.)),
                Anchor::corner(AnchorId(3), (2., 2.)),
                Anchor::corner(AnchorId(4), (0., 2.)),
            ],
        }
    }

    #[test]
    fn group_gestures_preserve_grab_offset_restore_exactly_and_do_not_close_open_paths() {
        let mut path = selection_path();
        path.closed = false;
        let down = (0.125, -0.375);
        let mut gesture = PathGesture {
            path: path.clone(),
            baseline: Some(path.clone()),
            edit: PathEdit::MovePoints {
                anchor: AnchorId(1),
                bases: vec![path.anchors[0], path.anchors[2]],
                down,
            },
        };
        gesture.update(down);
        gesture.close_on_click(down, 8.);
        assert_eq!(gesture.path, path);
        gesture.update((down.0 + 0.5, down.1 - 0.25));
        assert_eq!(gesture.path.anchors[0].point, (0.5, -0.25));
        assert_eq!(gesture.path.anchors[2].point, (2.5, 1.75));
        assert_eq!(gesture.path.anchors[1], path.anchors[1]);
        gesture.update(down);
        assert_eq!(gesture.path, path);
        // A single anchor retains the original click-first-anchor closure gesture.
        gesture.edit = PathEdit::MovePoints {
            anchor: AnchorId(1),
            bases: vec![path.anchors[0]],
            down,
        };
        gesture.close_on_click(down, 8.);
        assert!(gesture.path.closed);
        gesture.path = path.clone();
        gesture.update((down.0 + 10., down.1));
        gesture.close_on_click((down.0 + 10., down.1), 8.);
        assert!(!gesture.path.closed);
        // An inserted point is absent from the old path but still has a valid gesture baseline.
        let inserted = gesture.path.split_segment(0, 0.5, AnchorId(5)).unwrap();
        gesture.edit = PathEdit::MovePoints {
            anchor: inserted.id,
            bases: vec![inserted],
            down,
        };
        gesture.update((down.0 + 3., down.1 + 4.));
        assert_eq!(
            gesture.path.anchor(inserted.id).unwrap().point,
            (inserted.point.0 + 3., inserted.point.1 + 4.)
        );
        gesture.update(down);
        assert_eq!(gesture.path.anchor(inserted.id), Some(&inserted));
    }

    #[test]
    fn anchor_clicks_toggle_membership_keep_groups_for_drag_and_replace_other_targets() {
        let mut selected = None;
        AnchorSelection::click(&mut selected, PathId(1), AnchorId(1), false);
        AnchorSelection::click(&mut selected, PathId(1), AnchorId(3), true);
        let group = selected.clone();
        AnchorSelection::click(&mut selected, PathId(1), AnchorId(1), false);
        assert_eq!(selected, group);
        AnchorSelection::click(&mut selected, PathId(1), AnchorId(1), true);
        assert_eq!(selected.as_ref().unwrap().ids, vec![AnchorId(3)]);
        AnchorSelection::click(&mut selected, PathId(1), AnchorId(3), true);
        assert!(selected.is_none());
        AnchorSelection::click(&mut selected, PathId(1), AnchorId(2), true);
        AnchorSelection::click(&mut selected, PathId(1), AnchorId(4), false);
        assert_eq!(selected.as_ref().unwrap().ids, vec![AnchorId(4)]);
        AnchorSelection::click(&mut selected, PathId(2), AnchorId(2), true);
        assert_eq!(
            selected,
            Some(AnchorSelection::single(PathId(2), AnchorId(2)))
        );
    }

    #[test]
    fn anchor_selection_validates_visible_path_and_intersects_live_ids() {
        let mut path = selection_path();
        let original = path.clone();
        let selected = AnchorSelection {
            path: path.id,
            ids: vec![AnchorId(3), AnchorId(999), AnchorId(1)],
        };
        assert_eq!(
            selected.anchors(&path),
            vec![path.anchors[0], path.anchors[2]]
        );
        assert_eq!(path, original);
        path.visible = false;
        assert!(selected.anchors(&path).is_empty());
        path.visible = true;
        path.id = PathId(2);
        assert!(selected.anchors(&path).is_empty());
    }

    #[test]
    fn group_move_shape_and_delete_are_single_metadata_edits_with_exact_undo() {
        let mut tab = super::super::tests::opacity_tab();
        let original = selection_path();
        assert!(tab.document.content.upsert_path(original.clone()));
        let pixels = tab.document.canvas.pixels.clone();
        let layer = tab
            .document
            .content
            .layer(tab.active_layer)
            .unwrap()
            .pixels
            .clone();
        let mut edited = original.clone();
        let bases = vec![edited.anchors[0], edited.anchors[2]];
        assert!(edited.move_anchors_from(&bases, (0.5, -0.25)));
        let mut changed = false;
        for base in &bases {
            changed |= edited.make_smooth(base.id);
        }
        assert!(changed);
        let mut state = tab.state();
        assert!(state.content.upsert_path(edited.clone()));
        assert!(publish(&mut tab, state, Change::Edit));
        let saved = tab.history.undo_target().unwrap().0.clone();
        assert!(publish(&mut tab, saved, Change::Undo));
        assert_eq!(tab.document.content.active_path(), Some(&original));
        assert!(!tab.history.is_dirty());
        let next = tab.history.redo_target().unwrap().0.clone();
        assert!(publish(&mut tab, next, Change::Redo));
        assert_eq!(tab.document.content.active_path(), Some(&edited));
        let mut state = tab.state();
        assert!(
            state
                .content
                .delete_anchors(original.id, &[AnchorId(1), AnchorId(3)])
        );
        assert!(publish(&mut tab, state, Change::Edit));
        let remaining = tab.document.content.active_path().unwrap();
        assert_eq!(
            remaining.anchors.iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![AnchorId(2), AnchorId(4)]
        );
        assert!(!remaining.closed);
        let previous = tab.history.undo_target().unwrap().0.clone();
        assert!(publish(&mut tab, previous, Change::Undo));
        assert_eq!(tab.document.content.active_path(), Some(&edited));
        assert!(std::sync::Arc::ptr_eq(&pixels, &tab.document.canvas.pixels));
        assert!(std::sync::Arc::ptr_eq(
            &layer,
            &tab.document.content.layer(tab.active_layer).unwrap().pixels
        ));
    }

    #[test]
    fn batch_deletion_handles_empty_stale_duplicate_and_last_anchor_targets() {
        let mut tab = super::super::tests::opacity_tab();
        let path = selection_path();
        let mut other = path.clone();
        other.id = PathId(2);
        for anchor in &mut other.anchors {
            anchor.id.0 += 10;
        }
        assert!(tab.document.content.upsert_path(other.clone()));
        assert!(tab.document.content.upsert_path(path.clone()));
        let content = &mut tab.document.content;
        assert!(!content.delete_anchors(path.id, &[]));
        assert!(!content.delete_anchors(path.id, &[AnchorId(999)]));
        assert!(!content.delete_anchors(PathId(999), &[AnchorId(1)]));
        assert_eq!(content.active_path(), Some(&path));
        assert!(content.delete_anchors(path.id, &[AnchorId(2), AnchorId(2), AnchorId(999)]));
        assert!(content.active_path().unwrap().closed);
        assert!(content.delete_anchors(path.id, &[AnchorId(1), AnchorId(3), AnchorId(4)]));
        assert_eq!(content.paths(), &[other.clone()]);
        assert_eq!(content.active_path(), Some(&other));
        assert!(content.delete_anchors(other.id, &[AnchorId(11), AnchorId(12), AnchorId(13)]));
        assert!(content.delete_anchors(other.id, &[AnchorId(14)]));
        assert!(content.paths().is_empty());
        assert!(content.active_path().is_none());
    }

    #[test]
    fn inserted_and_shaped_anchor_undo_redo_preserves_shared_pixels_and_saved_revision() {
        let mut tab = super::super::tests::opacity_tab();
        let original = VectorPath {
            id: PathId(1),
            visible: true,
            closed: false,
            anchors: vec![
                Anchor {
                    id: AnchorId(1),
                    point: (0.2, 0.2),
                    handle_in: (0.2, 0.2),
                    handle_out: (0.2, 1.8),
                },
                Anchor {
                    id: AnchorId(2),
                    point: (1.8, 1.8),
                    handle_in: (1.8, 0.2),
                    handle_out: (1.8, 1.8),
                },
            ],
        };
        assert!(tab.document.content.upsert_path(original.clone()));
        let image = tab.document.canvas.pixels.clone();
        let layer = tab
            .document
            .content
            .layer(tab.active_layer)
            .unwrap()
            .pixels
            .clone();
        let mut edited = original.clone();
        let id = tab.document.content.next_anchor_id().unwrap();
        edited.split_segment(0, 0.37, id).unwrap();
        assert!(edited.make_corner(id));
        assert!(edited.make_smooth(id));
        let mut state = tab.state();
        assert!(state.content.upsert_path(edited.clone()));
        assert!(publish(&mut tab, state, Change::Edit));
        assert!(tab.history.is_dirty());
        assert!(std::sync::Arc::ptr_eq(&tab.document.canvas.pixels, &image));
        assert!(std::sync::Arc::ptr_eq(
            &tab.document.content.layer(tab.active_layer).unwrap().pixels,
            &layer
        ));
        assert_eq!(tab.document.content.active_path().unwrap(), &edited);
        let previous = tab.history.undo_target().unwrap().0.clone();
        assert!(publish(&mut tab, previous, Change::Undo));
        assert_eq!(tab.document.content.active_path().unwrap(), &original);
        assert!(!tab.history.is_dirty());
        let next = tab.history.redo_target().unwrap().0.clone();
        assert!(publish(&mut tab, next, Change::Redo));
        assert_eq!(tab.document.content.active_path().unwrap(), &edited);
        assert!(tab.history.is_dirty());
        assert!(std::sync::Arc::ptr_eq(&tab.document.canvas.pixels, &image));
    }
}
