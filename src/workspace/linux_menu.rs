//! Linux menus use the canonical GPUI command tree without a GTK window/event loop.
//! Keep editor focus in place so clipboard and typing-history actions keep their
//! normal targets. The workspace's keystroke interceptor routes menu navigation
//! before any inline editor, text tool, or canvas shortcut sees it.
use gpui::{
    Action, Context, CursorStyle, Decorations, Div, Keystroke, MouseButton, OwnedMenu,
    OwnedMenuItem, ResizeEdge, ScrollHandle, Window, anchored, deferred, div, point, prelude::*,
    px, rgb,
};

use super::Workspace;
use super::view::{BORDER, CHROME, DISABLED, HOVER, MUTED, SELECTED, TEXT, icon};

const BAR_HEIGHT: f32 = 30.;
const MENU_WIDTH: f32 = 340.;
const TITLE_HEIGHT: f32 = 32.;

pub(super) struct LinuxMenu {
    menus: Vec<OwnedMenu>,
    top: Option<usize>,
    parents: Vec<usize>,
    selected: Option<usize>,
    scroll: ScrollHandle,
}

impl Default for LinuxMenu {
    fn default() -> Self {
        Self {
            menus: crate::commands::application_menus(false)
                .into_iter()
                .map(|menu| menu.owned())
                .collect(),
            top: None,
            parents: Vec::new(),
            selected: None,
            scroll: ScrollHandle::new(),
        }
    }
}

impl LinuxMenu {
    fn current(&self) -> Option<&OwnedMenu> {
        let mut menu = self.menus.get(self.top?)?;
        for &index in &self.parents {
            let OwnedMenuItem::Submenu(child) = menu.items.get(index)? else {
                return None;
            };
            menu = child;
        }
        Some(menu)
    }

    pub(super) fn close(&mut self) {
        self.top = None;
        self.parents.clear();
        self.selected = None;
        self.scroll = ScrollHandle::new();
    }

    fn open(&mut self, top: usize) {
        self.top = Some(top);
        self.parents.clear();
        self.selected = None;
        self.scroll = ScrollHandle::new();
    }

    fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Workspace>) {
        let Some(menu) = self.current() else { return };
        let choices: Vec<_> = menu
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| available(item, window, cx).then_some(i))
            .collect();
        self.selected = next_choice(&choices, self.selected, forward);
        if let Some(index) = self.selected {
            self.scroll.scroll_to_item(index);
        }
    }
}

fn next_choice(choices: &[usize], selected: Option<usize>, forward: bool) -> Option<usize> {
    if choices.is_empty() {
        return None;
    }
    let index = selected.and_then(|selected| choices.iter().position(|&i| i == selected));
    Some(
        choices[match (index, forward) {
            (Some(i), true) => (i + 1) % choices.len(),
            (Some(i), false) => (i + choices.len() - 1) % choices.len(),
            (None, true) => 0,
            (None, false) => choices.len() - 1,
        }],
    )
}

fn available(item: &OwnedMenuItem, window: &mut Window, cx: &mut Context<Workspace>) -> bool {
    match item {
        OwnedMenuItem::Action { action, .. } => action_available(action.as_ref(), window, cx),
        OwnedMenuItem::Submenu(menu) => menu.items.iter().any(|item| available(item, window, cx)),
        OwnedMenuItem::Separator | OwnedMenuItem::SystemMenu(_) => false,
    }
}

fn action_available(action: &dyn Action, window: &Window, cx: &mut gpui::App) -> bool {
    // Window's query only checks the focused element tree. App's query also
    // includes global handlers (Open, Quit, Updates), even while this window is
    // already borrowed and App cannot query it again.
    window.is_action_available(action, cx) || cx.is_action_available(action)
}

fn label_width(menu: &OwnedMenu) -> f32 {
    menu.name.chars().count() as f32 * 7.5 + 20.
}

impl Workspace {
    pub(super) fn linux_menu_key(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Keystroke interceptors are application-wide; never take keys from
        // the separate Updates window or another workspace's focused view.
        if !self.focus.contains_focused(window, cx) {
            return false;
        }
        let interactive = self.form.is_none() && !self.dialog_open && self.pdf_job.is_none();
        if !interactive {
            self.linux_menu.close();
            return false;
        }
        let unmodified = !key.modifiers.control && !key.modifiers.platform && !key.modifiers.alt;
        let mnemonic = key.modifiers.alt
            && !key.modifiers.control
            && !key.modifiers.platform
            && !key.modifiers.shift;
        if mnemonic
            && let Some(index) = self.linux_menu.menus.iter().position(|menu| {
                menu.name
                    .chars()
                    .next()
                    .is_some_and(|c| c.to_lowercase().to_string() == key.key)
            })
        {
            self.linux_menu.open(index);
            self.linux_menu.navigate(true, window, cx);
            cx.notify();
            return true;
        }
        if key.key == "f10" && unmodified {
            if self.linux_menu.top.is_some() {
                self.linux_menu.close();
            } else {
                self.linux_menu.open(0);
                self.linux_menu.navigate(true, window, cx);
            }
            cx.notify();
            return true;
        }
        if self.linux_menu.top.is_none() {
            return false;
        }
        if unmodified {
            match key.key.as_str() {
                "escape" => self.linux_menu.close(),
                "up" | "down" => self.linux_menu.navigate(key.key == "down", window, cx),
                "home" | "end" => {
                    self.linux_menu.selected = None;
                    self.linux_menu.navigate(key.key == "home", window, cx);
                }
                "enter" | "space" => {
                    if let Some(index) = self.linux_menu.selected {
                        self.activate_linux_menu(index, window, cx);
                    }
                }
                "right" => {
                    let submenu = self
                        .linux_menu
                        .current()
                        .and_then(|menu| menu.items.get(self.linux_menu.selected?))
                        .is_some_and(|item| matches!(item, OwnedMenuItem::Submenu(_)));
                    if submenu {
                        self.activate_linux_menu(self.linux_menu.selected.unwrap(), window, cx);
                    } else {
                        self.cycle_linux_menu(true, window, cx);
                    }
                }
                "left" => {
                    if self.linux_menu.parents.is_empty() {
                        self.cycle_linux_menu(false, window, cx);
                    } else {
                        self.back_linux_menu(cx);
                    }
                }
                "tab" => self.cycle_linux_menu(!key.modifiers.shift, window, cx),
                _ => {}
            }
        }
        // Open menus own keystrokes, including editor shortcuts and text input.
        cx.notify();
        true
    }

    fn cycle_linux_menu(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.linux_menu.menus.len();
        let top = self.linux_menu.top.unwrap_or(0);
        self.linux_menu.open(if forward {
            (top + 1) % count
        } else {
            (top + count - 1) % count
        });
        self.linux_menu.navigate(true, window, cx);
    }

    fn back_linux_menu(&mut self, cx: &mut Context<Self>) {
        self.linux_menu.selected = self.linux_menu.parents.pop();
        self.linux_menu.scroll = ScrollHandle::new();
        cx.notify();
    }

    fn activate_linux_menu(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self
            .linux_menu
            .current()
            .and_then(|menu| menu.items.get(index))
            .cloned()
        else {
            return;
        };
        if !available(&item, window, cx) {
            return;
        }
        match item {
            OwnedMenuItem::Submenu(_) => {
                self.linux_menu.parents.push(index);
                self.linux_menu.selected = None;
                self.linux_menu.scroll = ScrollHandle::new();
                self.linux_menu.navigate(true, window, cx);
            }
            OwnedMenuItem::Action { action, .. } => {
                self.linux_menu.close();
                // Dispatch after closing the popup, using the unchanged editor
                // focus and ordinary action handlers (including numeric/text commits).
                window.defer(cx, move |window, cx| {
                    if action_available(action.as_ref(), window, cx) {
                        window.dispatch_action(action, cx);
                    }
                });
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn linux_titlebar(&self, window: &Window, cx: &Context<Self>) -> Option<Div> {
        if !matches!(window.window_decorations(), Decorations::Client { .. }) {
            return None;
        }
        let controls = window.window_controls();
        let button = |id, label: &'static str| {
            div()
                .id(id)
                .w(px(38.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .hover(|item| item.bg(rgb(HOVER)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(label)
        };
        Some(
            div()
                .h(px(TITLE_HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .bg(rgb(CHROME))
                .border_b_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .id("linux-window-drag")
                        .h_full()
                        .flex_1()
                        .px_3()
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child("Lime — Lite Image Editor")
                        .on_mouse_down(MouseButton::Left, |event, window, cx| {
                            if event.click_count == 2 {
                                window.zoom_window();
                            } else {
                                window.start_window_move();
                            }
                            cx.stop_propagation();
                        })
                        .on_mouse_down(MouseButton::Right, |event, window, cx| {
                            window.show_window_menu(event.position);
                            cx.stop_propagation();
                        }),
                )
                .when(controls.minimize, |bar| {
                    bar.child(
                        button("linux-window-minimize", "−")
                            .on_click(|_, window, _| window.minimize_window()),
                    )
                })
                .when(controls.maximize, |bar| {
                    bar.child(
                        button("linux-window-maximize", "□")
                            .on_click(|_, window, _| window.zoom_window()),
                    )
                })
                .child(button("linux-window-close", "×").on_click(cx.listener(
                    |this, _, window, cx| {
                        if this.request_exit(false, window, cx) {
                            window.remove_window();
                        }
                    },
                ))),
        )
    }

    pub(super) fn linux_resize_edges(&self, window: &Window) -> Option<Div> {
        if !matches!(window.window_decorations(), Decorations::Client { .. }) {
            return None;
        }
        // Server-decorated windows already have compositor resize borders. On
        // client-decorated Wayland windows provide small edge/corner hit areas.
        Some(
            div().absolute().top_0().left_0().size_full().children(
                [
                    (ResizeEdge::Top, CursorStyle::ResizeUpDown),
                    (ResizeEdge::Bottom, CursorStyle::ResizeUpDown),
                    (ResizeEdge::Left, CursorStyle::ResizeLeftRight),
                    (ResizeEdge::Right, CursorStyle::ResizeLeftRight),
                    (ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight),
                    (ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft),
                    (ResizeEdge::BottomLeft, CursorStyle::ResizeUpRightDownLeft),
                    (ResizeEdge::BottomRight, CursorStyle::ResizeUpLeftDownRight),
                ]
                .into_iter()
                .map(|(edge, cursor)| {
                    div()
                        .absolute()
                        .occlude()
                        .cursor(cursor)
                        .map(|area| match edge {
                            ResizeEdge::Top => area.top_0().left(px(8.)).right(px(8.)).h(px(4.)),
                            ResizeEdge::Bottom => {
                                area.bottom_0().left(px(8.)).right(px(8.)).h(px(4.))
                            }
                            ResizeEdge::Left => area.left_0().top(px(8.)).bottom(px(8.)).w(px(4.)),
                            ResizeEdge::Right => {
                                area.right_0().top(px(8.)).bottom(px(8.)).w(px(4.))
                            }
                            ResizeEdge::TopLeft => area.top_0().left_0().size(px(8.)),
                            ResizeEdge::TopRight => area.top_0().right_0().size(px(8.)),
                            ResizeEdge::BottomLeft => area.bottom_0().left_0().size(px(8.)),
                            ResizeEdge::BottomRight => area.bottom_0().right_0().size(px(8.)),
                        })
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.start_window_resize(edge);
                            cx.stop_propagation();
                        })
                }),
            ),
        )
    }

    pub(super) fn linux_menu_bar(&mut self, interactive: bool, cx: &mut Context<Self>) -> Div {
        if !interactive {
            self.linux_menu.close();
        }
        let menus = self.linux_menu.menus.clone();
        div()
            .h(px(BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(rgb(CHROME))
            .border_b_1()
            .border_color(rgb(BORDER))
            .children(menus.iter().enumerate().map(|(index, menu)| {
                div()
                    .id(("linux-menu", index))
                    .w(px(label_width(menu)))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(self.linux_menu.top == Some(index), |item| {
                        item.bg(rgb(SELECTED))
                    })
                    .when(interactive, |item| item.hover(|item| item.bg(rgb(HOVER))))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            if !interactive {
                                return;
                            }
                            if this.linux_menu.top == Some(index) {
                                this.linux_menu.close();
                            } else {
                                this.linux_menu.open(index);
                                this.linux_menu.navigate(true, window, cx);
                            }
                            cx.notify();
                        }),
                    )
                    .on_hover(cx.listener(move |this, hovered, window, cx| {
                        if *hovered
                            && this.linux_menu.top.is_some()
                            && this.linux_menu.top != Some(index)
                        {
                            this.linux_menu.open(index);
                            this.linux_menu.navigate(true, window, cx);
                            cx.notify();
                        }
                    }))
                    .child(menu.name.clone())
            }))
    }

    pub(super) fn linux_menu_overlay(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let menu = self.linux_menu.current()?.clone();
        let top = self.linux_menu.top?;
        let left: f32 = self.linux_menu.menus[..top].iter().map(label_width).sum();
        let bar_bottom = BAR_HEIGHT
            + if matches!(window.window_decorations(), Decorations::Client { .. }) {
                TITLE_HEIGHT
            } else {
                0.
            };
        let height = (f32::from(window.viewport_size().height) - bar_bottom - 12.).max(60.);
        let popup = div()
            .w(px(MENU_WIDTH))
            .max_h(px(height))
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
            .when(!self.linux_menu.parents.is_empty(), |panel| {
                panel.child(
                    div()
                        .id("linux-menu-back")
                        .h(px(30.))
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
                                this.back_linux_menu(cx);
                                cx.stop_propagation();
                            }),
                        )
                        .child(icon("chevron-left", 14.))
                        .child(menu.name.clone()),
                )
            })
            .child(
                div()
                    .id("linux-menu-items")
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.linux_menu.scroll)
                    .py_1()
                    .children(menu.items.iter().enumerate().map(|(index, item)| {
                        let enabled = available(item, window, cx);
                        let row = div()
                            .id(("linux-menu-item", index))
                            .h(px(28.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(rgb(if enabled { TEXT } else { DISABLED }))
                            .when(enabled && self.linux_menu.selected == Some(index), |row| {
                                row.bg(rgb(SELECTED))
                            })
                            .on_hover(cx.listener(move |this, hovered, _, cx| {
                                if *hovered && enabled {
                                    this.linux_menu.selected = Some(index);
                                    cx.notify();
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.activate_linux_menu(index, window, cx);
                                    cx.stop_propagation();
                                }),
                            );
                        match item {
                            OwnedMenuItem::Separator => div()
                                .h(px(9.))
                                .px_2()
                                .flex()
                                .items_center()
                                .child(div().w_full().h(px(1.)).bg(rgb(BORDER)))
                                .into_any_element(),
                            OwnedMenuItem::Action { name, action, .. } => {
                                let shortcut = shortcut(action.as_ref(), window, cx);
                                row.child(
                                    div().w(px(16.)).children(
                                        crate::menu_icons::for_action(action.as_ref())
                                            .map(|name| icon(name, 14.)),
                                    ),
                                )
                                .child(div().flex_1().child(name.clone()))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(MUTED))
                                        .child(shortcut),
                                )
                                .into_any_element()
                            }
                            OwnedMenuItem::Submenu(menu) => row
                                .child(div().w(px(16.)))
                                .child(div().flex_1().child(menu.name.clone()))
                                .child(icon("chevron-right", 14.))
                                .into_any_element(),
                            OwnedMenuItem::SystemMenu(_) => div().into_any_element(),
                        }
                    })),
            );
        Some(
            deferred(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    // Only the area below the bar occludes the editor; the top
                    // labels remain available for mouse switching and dismissal.
                    .child(
                        div()
                            .absolute()
                            .top(px(bar_bottom))
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .occlude()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.linux_menu.close();
                                    cx.notify();
                                    cx.stop_propagation();
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, _, _, cx| {
                                    this.linux_menu.close();
                                    cx.notify();
                                    cx.stop_propagation();
                                }),
                            ),
                    )
                    .child(
                        anchored()
                            .position(point(px(left), px(bar_bottom)))
                            .snap_to_window()
                            .child(popup),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

fn shortcut(action: &dyn Action, window: &Window, cx: &Context<Workspace>) -> String {
    window
        .focused(cx)
        .and_then(|focus| window.highest_precedence_binding_for_action_in(action, &focus))
        .map(|binding| {
            binding
                .keystrokes()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_skips_unavailable_rows_and_wraps() {
        let choices = [1, 4, 7];
        assert_eq!(next_choice(&choices, None, true), Some(1));
        assert_eq!(next_choice(&choices, None, false), Some(7));
        assert_eq!(next_choice(&choices, Some(1), false), Some(7));
        assert_eq!(next_choice(&choices, Some(7), true), Some(1));
        assert_eq!(next_choice(&choices, Some(2), true), Some(1));
        assert_eq!(next_choice(&[], Some(1), true), None);
    }

    #[test]
    fn shared_submenus_resolve_without_losing_the_parent() {
        let mut state = LinuxMenu::default();
        state.open(0);
        assert_eq!(state.current().unwrap().name.as_ref(), "File");
        assert!(
            state
                .current()
                .unwrap()
                .items
                .iter()
                .any(|item| matches!(item, OwnedMenuItem::Action { name, .. } if name == "Exit"))
        );
        let (top, child) = state
            .menus
            .iter()
            .enumerate()
            .find_map(|(top, menu)| {
                menu.items
                    .iter()
                    .position(|item| matches!(item, OwnedMenuItem::Submenu(_)))
                    .map(|child| (top, child))
            })
            .unwrap();
        state.open(top);
        state.parents.push(child);
        assert!(state.current().is_some());
        state.parents.pop();
        assert_eq!(state.current().unwrap().name, state.menus[top].name);
        state.close();
        assert!(state.current().is_none());
    }
}
