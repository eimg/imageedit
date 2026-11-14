//! Compact layer-row actions reuse the application menu action handlers.
use gpui::{
    Context, Keystroke, MouseButton, OwnedMenu, OwnedMenuItem, Pixels, Point, ScrollHandle, Window,
    anchored, deferred, div, prelude::*, px, rgb,
};

use super::tooltip::tooltip;
use super::view::{BORDER, CHROME, DISABLED, HOVER, SELECTED, TEXT, icon};
use super::{Workspace, numeric::NumericFollowup};
use crate::{commands::*, document::LayerId};

pub(super) struct LayerMenu {
    tab: usize,
    layer: LayerId,
    generation: u64,
    ready: bool,
    position: Point<Pixels>,
    menu: OwnedMenu,
    parents: Vec<usize>,
    selected: Option<usize>,
    scroll: ScrollHandle,
}

impl LayerMenu {
    fn current(&self) -> Option<&OwnedMenu> {
        let mut menu = &self.menu;
        for &index in &self.parents {
            let OwnedMenuItem::Submenu(child) = menu.items.get(index)? else {
                return None;
            };
            menu = child;
        }
        Some(menu)
    }

    fn valid(&self, workspace: &Workspace) -> bool {
        workspace.form.is_none()
            && !workspace.dialog_open
            && workspace.pdf_job.is_none()
            && workspace.active_tab().is_some_and(|tab| {
                tab.id == self.tab
                    && tab.active_layer == self.layer
                    && !tab.busy()
                    && tab.history.generation() == self.generation
                    && tab.document.content.layer(self.layer).is_some()
            })
    }
}

fn available(item: &OwnedMenuItem, window: &Window, cx: &mut gpui::App) -> bool {
    match item {
        OwnedMenuItem::Action { action, .. } => window.is_action_available(action.as_ref(), cx),
        OwnedMenuItem::Submenu(menu) => menu.items.iter().any(|item| available(item, window, cx)),
        _ => false,
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
        let mut menu = crate::commands::application_menus(cfg!(target_os = "macos"))
            .into_iter()
            .find(|menu| menu.name == "Layer")
            .unwrap()
            .owned();
        // Keep row-local conveniences only; the application menu owns the
        // broader image-editing operations and nested menus.
        menu.items.retain(|item| match item {
            OwnedMenuItem::Action { action, .. } => {
                let action = action.as_any();
                action.is::<RenameLayer>()
                    || action.is::<DuplicateLayer>()
                    || action.is::<DeleteLayer>()
                    || action.is::<ToggleAlphaLock>()
                    || (action.is::<DetachShadow>() && content.shadow_source(layer_id).is_some())
                    || (action.is::<RasterizeText>() && layer.text.is_some())
            }
            _ => false,
        });
        for item in &mut menu.items {
            if let OwnedMenuItem::Action { name, action, .. } = item
                && action.as_any().is::<ToggleAlphaLock>()
            {
                *name = if layer.locks.alpha {
                    "Unlock Alpha"
                } else {
                    "Lock Alpha"
                }
                .into();
            }
        }
        self.layer_menu = Some(LayerMenu {
            tab: tab_id,
            layer: layer_id,
            generation: tab.history.generation(),
            ready: false,
            position,
            menu,
            parents: vec![],
            selected: None,
            scroll: ScrollHandle::new(),
        });
        #[cfg(any(target_os = "linux", test))]
        self.linux_menu.close();
        self.focus.focus(window);
        // Availability uses GPUI's rendered dispatch tree. Let the selected row
        // and its action handlers render before exposing the popup's choices.
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
                let choices: Vec<_> = menu
                    .current()
                    .unwrap()
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, item)| available(item, window, cx).then_some(i))
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
            "left" => self.layer_menu_back(),
            "right" | "enter" | "return" | "space" => {
                if let Some(index) = menu.selected {
                    self.activate_layer_menu(index, key.key == "right", window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
        true
    }

    fn layer_menu_back(&mut self) {
        if let Some(menu) = self.layer_menu.as_mut() {
            menu.selected = menu.parents.pop();
            menu.scroll = ScrollHandle::new();
        }
    }

    fn activate_layer_menu(
        &mut self,
        index: usize,
        submenu_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.layer_menu.as_ref().filter(|menu| menu.valid(self)) else {
            self.layer_menu = None;
            return;
        };
        let Some(item) = menu
            .current()
            .and_then(|menu| menu.items.get(index))
            .cloned()
        else {
            return;
        };
        if !menu.ready || !available(&item, window, cx) {
            return;
        }
        match item {
            OwnedMenuItem::Submenu(_) => {
                let menu = self.layer_menu.as_mut().unwrap();
                menu.parents.push(index);
                menu.selected = menu
                    .current()
                    .unwrap()
                    .items
                    .iter()
                    .position(|item| available(item, window, cx));
                menu.scroll = ScrollHandle::new();
            }
            OwnedMenuItem::Action { action, .. } if !submenu_only => {
                let menu = self.layer_menu.take().unwrap();
                // Recheck stable target and generation after the next render has
                // installed handlers for the right-clicked layer.
                let entity = cx.weak_entity();
                window.defer(cx, move |window, cx| {
                    let valid = entity
                        .update(cx, |workspace, _| menu.valid(workspace))
                        .unwrap_or(false);
                    if valid && window.is_action_available(action.as_ref(), cx) {
                        window.dispatch_action(action, cx);
                    }
                });
            }
            _ => {}
        }
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
        let state = self.layer_menu.as_ref()?;
        if !state.ready {
            return None;
        }
        let menu = state.current()?;
        let popup = div()
            .w(px(310.))
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
            .when(!state.parents.is_empty(), |popup| {
                popup.child(
                    div()
                        .id("layer-menu-back")
                        .h(px(28.))
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .border_b_1()
                        .border_color(rgb(BORDER))
                        .hover(|row| row.bg(rgb(HOVER)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.layer_menu_back();
                                cx.notify();
                                cx.stop_propagation();
                            }),
                        )
                        .child(icon("chevron-left", 14.))
                        .child(menu.name.clone()),
                )
            })
            .child(
                div()
                    .id("layer-menu-items")
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&state.scroll)
                    .py_1()
                    .children(menu.items.iter().enumerate().map(|(index, item)| {
                        if matches!(item, OwnedMenuItem::Separator) {
                            return div()
                                .h(px(7.))
                                .px_2()
                                .flex()
                                .items_center()
                                .child(div().w_full().h(px(1.)).bg(rgb(BORDER)))
                                .into_any_element();
                        }
                        let enabled = available(item, window, cx);
                        let (name, symbol, submenu) = match item {
                            OwnedMenuItem::Action { name, action, .. } => (
                                name.clone(),
                                crate::menu_icons::for_action(action.as_ref()),
                                false,
                            ),
                            OwnedMenuItem::Submenu(menu) => (menu.name.to_string(), None, true),
                            _ => return div().into_any_element(),
                        };
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
                            .when(!enabled, |row| {
                                row.tooltip(tooltip(self.layer_menu_disabled_reason(item)))
                            })
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
                                    this.activate_layer_menu(index, false, window, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div()
                                    .w(px(16.))
                                    .children(symbol.map(|symbol| icon(symbol, 14.))),
                            )
                            .child(div().flex_1().child(name))
                            .when(submenu, |row| row.child(icon("chevron-right", 14.)))
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

    fn layer_menu_disabled_reason(&self, item: &OwnedMenuItem) -> String {
        let layer = self
            .active_tab()
            .and_then(|tab| tab.document.content.layer(tab.active_layer));
        if let OwnedMenuItem::Action { action, .. } = item {
            if action.as_any().is::<DeleteLayer>() {
                return "The image must keep at least one layer".into();
            }
            if action.as_any().is::<ToggleAlphaLock>()
                && layer.is_some_and(|layer| layer.text.is_some())
            {
                return "Rasterize the text layer before locking alpha".into();
            }
            if action.as_any().is::<RaiseLayer>() {
                return "This layer is already at the top".into();
            }
            if action.as_any().is::<LowerLayer>() {
                return "This layer is already at the bottom".into();
            }
        }
        if let Some(tab) = self.active_tab() {
            let align = matches!(item, OwnedMenuItem::Submenu(menu) if menu.name == "Align")
                || matches!(item, OwnedMenuItem::Action { action, .. } if action.as_any().is::<AlignLayer>());
            if align
                && let Some(blocker) = tab
                    .document
                    .content
                    .move_blocker(tab.active_layer)
                    .and_then(|id| tab.document.content.layer(id))
            {
                return format!(
                    "Unlock Position on \"{}\" before moving these layers",
                    blocker.name
                );
            }
        }
        if self.pending_edit_name().is_some() {
            return "Finish the current canvas preview first".into();
        }
        if layer.is_some_and(|layer| layer.locks.position) {
            return "Unlock Position to change layer geometry".into();
        }
        if layer.is_some_and(|layer| layer.text.is_some()) {
            return "Rasterize the text layer to use this action".into();
        }
        if layer.is_some_and(|layer| !layer.visible) {
            return "Show the layer to use this action".into();
        }
        "Unavailable for this layer or the current image state".into()
    }
}
