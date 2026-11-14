//! A flat menu of layer-row conveniences, dispatched through shared actions.
use gpui::{
    Action, Context, Keystroke, MouseButton, Pixels, Point, ScrollHandle, Window, anchored,
    deferred, div, prelude::*, px, rgb,
};

use super::tooltip::tooltip;
use super::view::{BORDER, CHROME, DISABLED, SELECTED, TEXT, icon};
use super::{Workspace, numeric::NumericFollowup};
use crate::{
    commands::*,
    document::{DocumentContent, LayerId},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowAction {
    Rename,
    Duplicate,
    AlphaLock,
    Detach,
    Rasterize,
    Delete,
}

impl RowAction {
    fn action(self) -> Box<dyn Action> {
        match self {
            Self::Rename => Box::new(RenameLayer),
            Self::Duplicate => Box::new(DuplicateLayer),
            Self::AlphaLock => Box::new(ToggleAlphaLock),
            Self::Detach => Box::new(DetachShadow),
            Self::Rasterize => Box::new(RasterizeText),
            Self::Delete => Box::new(DeleteLayer),
        }
    }

    fn label(self, alpha_locked: bool) -> &'static str {
        match self {
            Self::Rename => "Rename Layer…",
            Self::Duplicate => "Duplicate Layer",
            Self::AlphaLock if alpha_locked => "Unlock Alpha",
            Self::AlphaLock => "Lock Alpha",
            Self::Detach => "Detach from Source",
            Self::Rasterize => "Rasterize Text Layer",
            Self::Delete => "Delete Layer",
        }
    }

    fn disabled_reason(self, content: &DocumentContent, id: LayerId) -> Option<&'static str> {
        let Some(layer) = content.layer(id) else {
            return Some("The layer is no longer available");
        };
        match self {
            Self::Duplicate => content.duplicate_layer_unavailable_reason(id),
            Self::AlphaLock if layer.text.is_some() => {
                Some("Rasterize the text layer before locking alpha")
            }
            Self::Delete if content.layers().len() == 1 => {
                Some("The image must keep at least one layer")
            }
            Self::Detach if content.shadow_source(id).is_none() => {
                Some("This layer is no longer attached")
            }
            Self::Rasterize if layer.text.is_none() => {
                Some("This layer has already been rasterized")
            }
            _ => None,
        }
    }
}

pub(super) struct LayerMenu {
    tab: usize,
    layer: LayerId,
    generation: u64,
    ready: bool,
    position: Point<Pixels>,
    items: Vec<RowAction>,
    selected: Option<usize>,
    scroll: ScrollHandle,
}

impl LayerMenu {
    fn valid(&self, workspace: &Workspace) -> bool {
        workspace.form.is_none()
            && !workspace.dialog_open
            && workspace.pdf_job.is_none()
            && workspace.layer_metadata_preview_blocker().is_none()
            && workspace.active_tab().is_some_and(|tab| {
                tab.id == self.tab
                    && tab.active_layer == self.layer
                    && !tab.busy()
                    && tab.history.generation() == self.generation
                    && tab.document.content.layer(self.layer).is_some()
            })
    }
}

impl Workspace {
    pub(super) fn open_layer_context_menu(
        &mut self,
        tab_id: usize,
        layer_id: LayerId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.layer_menu = None;
        if self.active_tab().is_none_or(|tab| {
            tab.id != tab_id || tab.busy() || tab.document.content.layer(layer_id).is_none()
        }) || self.form.is_some()
            || self.dialog_open
            || self.pdf_job.is_some()
        {
            return;
        }
        // Guard before selecting a different row: selection cancels previews.
        if self.refuse_layer_metadata_preview(cx) {
            return;
        }
        if self.commit_numeric_with_followup(
            NumericFollowup::LayerContextMenu {
                tab: tab_id,
                layer: layer_id,
                position,
            },
            window,
            cx,
        ) {
            return;
        }
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| {
                this.open_layer_context_menu(tab_id, layer_id, position, window, cx)
            });
            return;
        }
        self.select_layer(layer_id, window, cx);
        let Some(tab) = self
            .active_tab()
            .filter(|tab| tab.id == tab_id && tab.active_layer == layer_id && !tab.busy())
        else {
            return;
        };
        let content = &tab.document.content;
        let Some(layer) = content.layer(layer_id) else {
            return;
        };
        let mut items = vec![
            RowAction::Rename,
            RowAction::Duplicate,
            RowAction::AlphaLock,
        ];
        if content.shadow_source(layer_id).is_some() {
            items.push(RowAction::Detach);
        }
        if layer.text.is_some() {
            items.push(RowAction::Rasterize);
        }
        items.push(RowAction::Delete);
        self.layer_menu = Some(LayerMenu {
            tab: tab_id,
            layer: layer_id,
            generation: tab.history.generation(),
            ready: false,
            position,
            items,
            selected: None,
            scroll: ScrollHandle::new(),
        });
        #[cfg(any(target_os = "linux", test))]
        self.linux_menu.close();
        self.focus.focus(window);
        // Dispatch must use the handlers for the selected row after it renders.
        let entity = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            let _ = entity.update(cx, |workspace, cx| {
                if workspace.layer_menu.as_ref().is_some_and(|menu| {
                    menu.tab == tab_id && menu.layer == layer_id && menu.valid(workspace)
                }) {
                    workspace.layer_menu.as_mut().unwrap().ready = true;
                    cx.notify();
                }
            });
        });
        cx.notify();
    }

    pub(super) fn layer_menu_key(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.focus.contains_focused(window, cx) {
            return false;
        }
        let Some(menu) = self.layer_menu.as_ref() else {
            return false;
        };
        if !menu.valid(self) {
            self.layer_menu = None;
            return false;
        }
        if !menu.ready && key.key != "escape" {
            return true;
        }
        match key.key.as_str() {
            "escape" => {
                self.layer_menu = None;
            }
            "up" | "down" | "home" | "end" => {
                let content = &self.active_tab().unwrap().document.content;
                let choices: Vec<_> = menu
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, action)| {
                        action
                            .disabled_reason(content, menu.layer)
                            .is_none()
                            .then_some(i)
                    })
                    .collect();
                let selected = menu
                    .selected
                    .and_then(|i| choices.iter().position(|&choice| choice == i));
                let next = if choices.is_empty() {
                    None
                } else {
                    Some(
                        choices[match key.key.as_str() {
                            "home" => 0,
                            "end" => choices.len() - 1,
                            "up" => selected.map_or(choices.len() - 1, |i| {
                                (i + choices.len() - 1) % choices.len()
                            }),
                            _ => selected.map_or(0, |i| (i + 1) % choices.len()),
                        }],
                    )
                };
                let menu = self.layer_menu.as_mut().unwrap();
                menu.selected = next;
                if let Some(i) = next {
                    menu.scroll.scroll_to_item(i);
                }
            }
            "enter" | "return" | "space" => {
                if let Some(index) = menu.selected {
                    self.activate_layer_menu(index, window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
        true
    }

    fn activate_layer_menu(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self
            .layer_menu
            .as_ref()
            .filter(|menu| menu.ready && menu.valid(self))
        else {
            return;
        };
        let Some(&item) = menu.items.get(index) else {
            return;
        };
        if item
            .disabled_reason(&self.active_tab().unwrap().document.content, menu.layer)
            .is_some()
        {
            return;
        }
        let menu = self.layer_menu.take().unwrap();
        let entity = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let valid = entity
                .update(cx, |workspace, _| {
                    menu.valid(workspace)
                        && workspace.active_tab().is_some_and(|tab| {
                            item.disabled_reason(&tab.document.content, menu.layer)
                                .is_none()
                        })
                })
                .unwrap_or(false);
            let action = item.action();
            if valid && window.is_action_available(action.as_ref(), cx) {
                window.dispatch_action(action, cx);
            }
        });
        cx.notify();
    }

    pub(super) fn layer_context_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if self
            .layer_menu
            .as_ref()
            .is_some_and(|menu| !menu.valid(self))
        {
            self.layer_menu = None;
        }
        let state = self.layer_menu.as_ref().filter(|menu| menu.ready)?;
        let content = &self.active_tab()?.document.content;
        let alpha_locked = content.layer(state.layer)?.locks.alpha;
        let popup = div()
            .w(px(260.))
            .max_h((window.viewport_size().height - px(16.)).max(px(60.)))
            .flex()
            .flex_col()
            .bg(rgb(CHROME))
            .text_color(rgb(TEXT))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_sm()
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("layer-menu-items")
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&state.scroll)
                    .py_1()
                    .children(state.items.iter().enumerate().map(|(index, &item)| {
                        let reason = item.disabled_reason(content, state.layer);
                        let enabled = reason.is_none();
                        div()
                            .id(("layer-menu-item", index))
                            .h(px(28.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(rgb(if enabled { TEXT } else { DISABLED }))
                            .when(enabled && state.selected == Some(index), |row| {
                                row.bg(rgb(SELECTED))
                            })
                            .when_some(reason, |row, reason| row.tooltip(tooltip(reason)))
                            .on_hover(cx.listener(move |this, hovered, _, cx| {
                                if *hovered && enabled {
                                    if let Some(menu) = this.layer_menu.as_mut() {
                                        menu.selected = Some(index);
                                    }
                                    cx.notify();
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.activate_layer_menu(index, window, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div().w(px(16.)).children(
                                    crate::menu_icons::for_action(item.action().as_ref())
                                        .map(|symbol| icon(symbol, 14.)),
                                ),
                            )
                            .child(item.label(alpha_locked))
                            .into_any_element()
                    })),
            );
        Some(
            deferred(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .occlude()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.layer_menu = None;
                                    cx.notify();
                                    cx.stop_propagation();
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, _, _, cx| {
                                    this.layer_menu = None;
                                    cx.notify();
                                    cx.stop_propagation();
                                }),
                            ),
                    )
                    .child(
                        anchored()
                            .position(state.position)
                            .snap_to_window()
                            .child(popup),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LayerLocks, MAX_LAYERS};
    use image::{Rgba, RgbaImage};

    #[test]
    fn duplicate_explains_capacity_even_for_hidden_position_locked_layers() {
        let mut content = DocumentContent::single_layer("Base".into(), RgbaImage::new(1, 1));
        let base = content.top_layer_id();
        let mut layer = base;
        for _ in 1..MAX_LAYERS {
            layer = content
                .add_layer(layer, "Layer".into(), Rgba([0; 4]))
                .unwrap();
        }
        content.set_visible(base, false);
        content.set_layer_locks(
            base,
            LayerLocks {
                position: true,
                alpha: false,
            },
        );
        let reason = RowAction::Duplicate
            .disabled_reason(&content, base)
            .unwrap();
        assert!(reason.contains("layer limit"));
        assert!(!content.can_duplicate_layer(base));
        content.delete_layer(layer).unwrap();
        assert_eq!(RowAction::Duplicate.disabled_reason(&content, base), None);
        assert!(content.can_duplicate_layer(base));
        content.duplicate_layer(base).unwrap();
    }
}
