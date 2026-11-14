//! Linux menus use the canonical GPUI command tree without a GTK window/event loop.
//! Keep editor focus in place so clipboard and typing-history actions keep their
//! normal targets. The workspace's keystroke interceptor routes menu navigation
//! before any inline editor, text tool, or canvas shortcut sees it.
use gpui::{
    Action, Bounds, Context, CursorStyle, Decorations, Div, Keystroke, MouseButton, OwnedMenu,
    OwnedMenuItem, Pixels, Point, ResizeEdge, ScrollHandle, Size, Window, deferred, div, point,
    prelude::*, px, rgb, size,
};

use super::Workspace;
use super::view::{BORDER, CHROME, DISABLED, HOVER, MUTED, SELECTED, TEXT, icon};

const BAR_HEIGHT: f32 = 30.;
const MENU_WIDTH: f32 = 340.;
const TITLE_HEIGHT: f32 = 32.;
const ROW_HEIGHT: f32 = 28.;
const SEPARATOR_HEIGHT: f32 = 9.;
const MENU_PADDING: f32 = 4.;
const MENU_BORDER: f32 = 1.;
const WINDOW_MARGIN: f32 = 4.;

#[derive(Default)]
struct MenuPanel {
    selected: Option<usize>,
    scroll: ScrollHandle,
}

pub(super) struct LinuxMenu {
    menus: Vec<OwnedMenu>,
    top: Option<usize>,
    parents: Vec<usize>,
    panels: Vec<MenuPanel>,
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
            panels: Vec::new(),
        }
    }
}

impl LinuxMenu {
    fn menu_at(&self, depth: usize) -> Option<&OwnedMenu> {
        if depth > self.parents.len() {
            return None;
        }
        let mut menu = self.menus.get(self.top?)?;
        for &index in &self.parents[..depth] {
            let OwnedMenuItem::Submenu(child) = menu.items.get(index)? else {
                return None;
            };
            menu = child;
        }
        Some(menu)
    }

    fn current(&self) -> Option<&OwnedMenu> {
        self.menu_at(self.parents.len())
    }

    fn selected(&self) -> Option<usize> {
        self.panels.last()?.selected
    }

    // Event handlers from a previous frame must not act on a different branch.
    fn matches_panel(&self, top: usize, path: &[usize]) -> bool {
        self.top == Some(top) && self.parents.starts_with(path) && path.len() < self.panels.len()
    }

    pub(super) fn close(&mut self) {
        self.top = None;
        self.parents.clear();
        self.panels.clear();
    }

    fn open(&mut self, top: usize) {
        self.top = Some(top);
        self.parents.clear();
        self.panels.clear();
        self.panels.push(MenuPanel::default());
    }

    fn truncate(&mut self, depth: usize) {
        self.parents.truncate(depth);
        self.panels.truncate(depth + 1);
    }

    fn back(&mut self) {
        if !self.parents.is_empty() {
            self.truncate(self.parents.len() - 1);
        } else {
            self.close();
        }
    }

    fn open_submenu(&mut self, depth: usize, index: usize) -> bool {
        if !self
            .menu_at(depth)
            .and_then(|menu| menu.items.get(index))
            .is_some_and(|item| matches!(item, OwnedMenuItem::Submenu(_)))
            || self.parents.get(depth) == Some(&index)
        {
            return false;
        }
        self.truncate(depth);
        self.panels[depth].selected = Some(index);
        self.parents.push(index);
        self.panels.push(MenuPanel::default());
        true
    }

    fn hover(&mut self, depth: usize, index: usize, enabled: bool) {
        if self.parents.get(depth) != Some(&index) {
            self.truncate(depth);
        }
        self.panels[depth].selected = enabled.then_some(index);
        if enabled {
            self.open_submenu(depth, index);
        }
    }

    fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Workspace>) {
        let Some(menu) = self.current() else { return };
        let choices: Vec<_> = menu
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| available(item, window, cx).then_some(i))
            .collect();
        let Some(panel) = self.panels.last_mut() else {
            return;
        };
        panel.selected = next_choice(&choices, panel.selected, forward);
        if let Some(index) = panel.selected {
            panel.scroll.scroll_to_item(index);
        }
    }
}

fn item_height(item: &OwnedMenuItem) -> f32 {
    if matches!(item, OwnedMenuItem::Separator) {
        SEPARATOR_HEIGHT
    } else {
        ROW_HEIGHT
    }
}

fn menu_height(menu: &OwnedMenu, available: Pixels) -> Pixels {
    px(menu.items.iter().map(item_height).sum::<f32>() + 2. * (MENU_PADDING + MENU_BORDER))
        .min(available)
}

// Keep a cascade beside its parent, reversing direction near a window edge.
// Only very narrow windows need overlapping panels to keep every action reachable.
fn submenu_origin(
    parent: Bounds<Pixels>,
    row_top: Pixels,
    child: Size<Pixels>,
    viewport: Size<Pixels>,
    min_y: Pixels,
    prefer_left: bool,
) -> (Point<Pixels>, bool) {
    let right = parent.right() - px(MENU_BORDER);
    let left = parent.left() - child.width + px(MENU_BORDER);
    let fits_right = right + child.width <= viewport.width - px(WINDOW_MARGIN);
    let fits_left = left >= px(WINDOW_MARGIN);
    let opens_left = if prefer_left {
        fits_left || !fits_right
    } else {
        !fits_right && fits_left
    };
    let x = if opens_left { left } else { right };
    let max_x = (viewport.width - child.width - px(WINDOW_MARGIN)).max(px(0.));
    let max_y = (viewport.height - child.height - px(WINDOW_MARGIN)).max(min_y);
    (
        point(x.clamp(px(0.), max_x), row_top.clamp(min_y, max_y)),
        opens_left,
    )
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
                "escape" => self.linux_menu.back(),
                "up" | "down" => self.linux_menu.navigate(key.key == "down", window, cx),
                "home" | "end" => {
                    self.linux_menu.panels.last_mut().unwrap().selected = None;
                    self.linux_menu.navigate(key.key == "home", window, cx);
                }
                "enter" | "space" => {
                    if let Some(index) = self.linux_menu.selected() {
                        self.activate_linux_menu(self.linux_menu.parents.len(), index, window, cx);
                    } else {
                        self.linux_menu.navigate(true, window, cx);
                    }
                }
                "right" => {
                    let submenu = self
                        .linux_menu
                        .current()
                        .and_then(|menu| menu.items.get(self.linux_menu.selected()?))
                        .is_some_and(|item| matches!(item, OwnedMenuItem::Submenu(_)));
                    if self.linux_menu.selected().is_none() {
                        // Hover-opened panels have no keyboard selection yet.
                        self.linux_menu.navigate(true, window, cx);
                    } else if submenu {
                        self.activate_linux_menu(
                            self.linux_menu.parents.len(),
                            self.linux_menu.selected().unwrap(),
                            window,
                            cx,
                        );
                    } else if self.linux_menu.parents.is_empty() {
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
        self.linux_menu.back();
        cx.notify();
    }

    fn activate_linux_menu(
        &mut self,
        depth: usize,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self
            .linux_menu
            .menu_at(depth)
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
                self.linux_menu.open_submenu(depth, index);
                self.linux_menu.truncate(depth + 1);
                if self.linux_menu.selected().is_none() {
                    self.linux_menu.navigate(true, window, cx);
                }
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

    fn linux_menu_panel(
        &self,
        depth: usize,
        menu: &OwnedMenu,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let top = self.linux_menu.top.unwrap();
        let path = self.linux_menu.parents[..depth].to_vec();
        let scroll_path = path.clone();
        let panel = &self.linux_menu.panels[depth];
        div()
            .id(("linux-menu-panel", depth))
            .absolute()
            .left(bounds.left())
            .top(bounds.top())
            .w(bounds.size.width)
            .h(bounds.size.height)
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
                    .id("linux-menu-items")
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&panel.scroll)
                    .py(px(MENU_PADDING))
                    // A scrolled parent invalidates the anchor of its descendants.
                    .on_scroll_wheel(cx.listener(move |this, _, _, cx| {
                        if this.linux_menu.matches_panel(top, &scroll_path) {
                            this.linux_menu.truncate(depth);
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }))
                    .children(menu.items.iter().enumerate().map(|(index, item)| {
                        let enabled = available(item, window, cx);
                        let hover_path = path.clone();
                        let click_path = path.clone();
                        let row = div()
                            .id(("linux-menu-item", index))
                            .h(px(item_height(item)))
                            .flex_shrink_0()
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(rgb(if enabled { TEXT } else { DISABLED }))
                            .when(enabled && panel.selected == Some(index), |row| {
                                row.bg(rgb(SELECTED))
                            })
                            .on_hover(cx.listener(move |this, hovered, _, cx| {
                                if *hovered && this.linux_menu.matches_panel(top, &hover_path) {
                                    this.linux_menu.hover(depth, index, enabled);
                                    cx.notify();
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    if this.linux_menu.matches_panel(top, &click_path) {
                                        this.activate_linux_menu(depth, index, window, cx);
                                    }
                                    cx.stop_propagation();
                                }),
                            );
                        match item {
                            OwnedMenuItem::Separator => row
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
            )
            .into_any_element()
    }

    pub(super) fn linux_menu_overlay(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let top = self.linux_menu.top?;
        let viewport = window.viewport_size();
        let left: f32 = self.linux_menu.menus[..top].iter().map(label_width).sum();
        let bar_bottom = px(BAR_HEIGHT
            + if matches!(window.window_decorations(), Decorations::Client { .. }) {
                TITLE_HEIGHT
            } else {
                0.
            });
        let available_height = (viewport.height - bar_bottom - px(WINDOW_MARGIN)).max(px(1.));
        let width = px(MENU_WIDTH).min((viewport.width - px(2. * WINDOW_MARGIN)).max(px(1.)));
        let mut origin = point(
            px(left).min((viewport.width - width - px(WINDOW_MARGIN)).max(px(0.))),
            bar_bottom,
        );
        let mut opens_left = false;
        let mut popups = Vec::new();
        for depth in 0..self.linux_menu.panels.len() {
            let menu = self.linux_menu.menu_at(depth)?;
            let bounds = Bounds {
                origin,
                size: size(width, menu_height(menu, available_height)),
            };
            popups.push(self.linux_menu_panel(depth, menu, bounds, window, cx));
            if let Some(&index) = self.linux_menu.parents.get(depth) {
                let child = self.linux_menu.menu_at(depth + 1)?;
                // Account for scroll clamping during a window resize before
                // GPUI lays out the panels for this frame.
                let min_offset = (bounds.size.height - menu_height(menu, Pixels::MAX)).min(px(0.));
                let offset = self.linux_menu.panels[depth]
                    .scroll
                    .offset()
                    .y
                    .clamp(min_offset, px(0.));
                let row_top = origin.y
                    + px(menu.items[..index].iter().map(item_height).sum::<f32>())
                    + offset;
                (origin, opens_left) = submenu_origin(
                    bounds,
                    row_top,
                    size(width, menu_height(child, available_height)),
                    viewport,
                    bar_bottom,
                    opens_left,
                );
            }
        }
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
                            .top(bar_bottom)
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
                    .children(popups),
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
        assert!(state.open_submenu(0, child));
        assert_eq!(state.panels.len(), 2);
        assert_eq!(state.menu_at(0).unwrap().name, state.menus[top].name);
        assert!(state.current().is_some());
        assert_eq!(state.panels[0].selected, Some(child));
        state.back();
        assert_eq!(state.current().unwrap().name, state.menus[top].name);
        state.close();
        assert!(state.current().is_none());
        assert!(state.panels.is_empty());
    }

    fn cascade() -> LinuxMenu {
        fn menu(name: &str, items: Vec<OwnedMenuItem>) -> OwnedMenu {
            OwnedMenu {
                name: name.to_owned().into(),
                items,
            }
        }
        fn action() -> OwnedMenuItem {
            OwnedMenuItem::Action {
                name: "Open".into(),
                action: Box::new(crate::commands::OpenImage),
                os_action: None,
            }
        }
        let mut state = LinuxMenu {
            menus: vec![menu(
                "Root",
                vec![
                    OwnedMenuItem::Submenu(menu(
                        "First",
                        vec![
                            OwnedMenuItem::Submenu(menu("Nested", vec![action()])),
                            action(),
                        ],
                    )),
                    OwnedMenuItem::Submenu(menu("Second", vec![action()])),
                    OwnedMenuItem::Separator,
                    action(),
                ],
            )],
            ..Default::default()
        };
        state.open(0);
        state
    }

    #[test]
    fn hover_keeps_ancestors_and_switches_only_the_affected_branch() {
        let mut state = cascade();
        state.panels[0].scroll.set_offset(point(px(0.), px(-70.)));
        state.hover(0, 0, true);
        state.hover(1, 0, true);
        state.hover(2, 0, true);
        state.panels[1].scroll.set_offset(point(px(0.), px(-12.)));
        // Crossing the same parent row must not reset its open descendants.
        state.hover(0, 0, true);
        assert_eq!(state.parents, vec![0, 0]);
        assert_eq!(state.panels.len(), 3);
        assert_eq!(state.selected(), Some(0));
        assert_eq!(state.panels[1].scroll.offset().y, px(-12.));
        assert!(state.matches_panel(0, &[0, 0]));
        state.hover(0, 1, true);
        assert_eq!(state.parents, vec![1]);
        assert_eq!(state.panels.len(), 2);
        assert_eq!(state.current().unwrap().name.as_ref(), "Second");
        assert_eq!(state.panels[0].scroll.offset().y, px(-70.));
        assert!(state.selected().is_none());
        assert!(!state.matches_panel(0, &[0]));
        assert!(!state.matches_panel(1, &[1]));
        // Disabled items and separators dismiss an obsolete child without selection.
        state.hover(0, 2, false);
        assert!(state.parents.is_empty());
        assert_eq!(state.panels.len(), 1);
        assert!(state.selected().is_none());
        state.hover(0, 3, true);
        assert_eq!(state.selected(), Some(3));
        assert_eq!(state.panels.len(), 1);
    }

    #[test]
    fn returning_from_nested_submenus_restores_parent_selection_and_scroll() {
        let mut state = cascade();
        assert!(state.open_submenu(0, 0));
        assert!(state.open_submenu(1, 0));
        state.panels[1].scroll.set_offset(point(px(0.), px(-28.)));
        state.back();
        assert_eq!(state.parents, vec![0]);
        assert_eq!(state.selected(), Some(0));
        assert_eq!(state.panels[1].scroll.offset().y, px(-28.));
        state.back();
        assert!(state.parents.is_empty());
        assert_eq!(state.selected(), Some(0));
        state.back();
        assert!(state.top.is_none());
        assert!(state.panels.is_empty());
        assert!(!state.matches_panel(0, &[]));
    }

    #[test]
    fn submenus_open_beside_the_parent_and_flip_at_window_edges() {
        let viewport = size(px(1200.), px(800.));
        let child = size(px(340.), px(200.));
        let parent = Bounds {
            origin: point(px(20.), px(62.)),
            size: child,
        };
        let (origin, left) = submenu_origin(parent, px(180.), child, viewport, px(62.), false);
        assert_eq!(origin, point(px(359.), px(180.)));
        assert!(!left);

        let parent = Bounds {
            origin: point(px(850.), px(62.)),
            size: child,
        };
        let (origin, left) = submenu_origin(parent, px(740.), child, viewport, px(62.), false);
        assert_eq!(origin, point(px(511.), px(596.)));
        assert!(left);
        // A nested cascade continues left when both sides have room.
        let parent = Bounds {
            origin,
            size: child,
        };
        let (origin, left) = submenu_origin(parent, px(650.), child, viewport, px(62.), left);
        assert_eq!(origin, point(px(172.), px(596.)));
        assert!(left);
        // Reverse again when the left edge is reached.
        let parent = Bounds {
            origin,
            size: child,
        };
        let (origin, left) = submenu_origin(parent, px(20.), child, viewport, px(62.), left);
        assert_eq!(origin, point(px(511.), px(62.)));
        assert!(!left);
    }

    #[test]
    fn submenus_remain_in_bounds_in_narrow_and_short_windows() {
        let viewport = size(px(500.), px(250.));
        let child = size(px(340.), px(184.));
        let parent = Bounds {
            origin: point(px(100.), px(62.)),
            size: child,
        };
        let (origin, _) = submenu_origin(parent, px(200.), child, viewport, px(62.), false);
        assert!(origin.x >= px(0.));
        assert!(origin.x + child.width <= viewport.width);
        assert_eq!(origin.y, px(62.));
        assert!(origin.y + child.height <= viewport.height);
        let state = cascade();
        let menu = state.current().unwrap();
        assert_eq!(menu_height(menu, px(1000.)), px(103.));
        assert_eq!(menu_height(menu, px(80.)), px(80.));
    }
}
