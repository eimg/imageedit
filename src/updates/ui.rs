use gpui::{
    App, AppContext as _, Bounds, Context, Div, FocusHandle, IntoElement, Render, Stateful,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};

use super::{CheckStatus, RELEASES, UpdateState, check};
use crate::theme::{ACCENT, BORDER, CHROME, HOVER, MUTED, PANEL, SELECTED, TEXT};
use std::rc::Rc;

pub(crate) fn badge(cx: &App) -> Option<Stateful<Div>> {
    let update = cx.try_global::<UpdateState>()?.available.as_ref()?;
    Some(
        div()
            .id("update-available")
            .tab_index(0)
            .flex_shrink_0()
            .ml_4()
            .px_2()
            .py(px(2.))
            .rounded_md()
            .bg(rgb(SELECTED))
            .text_color(rgb(ACCENT))
            .text_size(px(11.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(HOVER)))
            .child(format!("↑ Lime {} available", update.version))
            .on_click(|_, _, cx| show(cx))
            .on_key_down(|event, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    show(cx);
                    cx.stop_propagation();
                }
            }),
    )
}

pub(crate) fn show(cx: &mut App) {
    if let Some(handle) = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<UpdatesWindow>())
    {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let height = dialog_height(cx.global::<UpdateState>());
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(480.), px(height)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Lime Updates".into()),
                ..Default::default()
            }),
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                let focus = cx.focus_handle();
                focus.focus(window);
                UpdatesWindow { focus, height }
            })
        },
    );
    if let Err(error) = result {
        eprintln!("Could not open the update window: {error:#}");
    }
}

pub(super) struct UpdatesWindow {
    focus: FocusHandle,
    height: f32,
}

fn dialog_height(state: &UpdateState) -> f32 {
    match &state.available {
        None => 280.,
        Some(update)
            if update.download.is_none()
                || state.checking
                || matches!(state.status, CheckStatus::Failed) =>
        {
            380.
        }
        Some(_) => 340.,
    }
}

fn button(
    id: &'static str,
    label: impl Into<gpui::SharedString>,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let action = Rc::new(action);
    let click = action.clone();
    div()
        .id(id)
        .tab_index(0)
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CHROME))
        .text_color(rgb(TEXT))
        .text_size(px(13.))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(HOVER)))
        .focus(|s| s.border_color(rgb(ACCENT)))
        .on_click(move |_, window, cx| click(window, cx))
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                action(window, cx);
                cx.stop_propagation();
            }
        })
        .child(label.into())
}

impl Render for UpdatesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = cx.global::<UpdateState>();
        let height = dialog_height(state);
        if self.height != height {
            self.height = height;
            window.resize(size(px(480.), px(height)));
        }
        let update = state.available.clone();
        let (title, detail) = if let Some(update) = &update {
            (
                format!("Lime {} is available", update.version),
                format!(
                    "You're using Lime {}. Download the new version and install it when you're ready.",
                    env!("CARGO_PKG_VERSION")
                ),
            )
        } else if state.checking {
            (
                "Checking for updates…".into(),
                "Looking for the latest stable release of Lime.".into(),
            )
        } else if matches!(state.status, CheckStatus::Current) {
            (
                "You're up to date".into(),
                format!(
                    "Lime {} is the latest stable version.",
                    env!("CARGO_PKG_VERSION")
                ),
            )
        } else if matches!(state.status, CheckStatus::Failed) {
            (
                "Couldn't check for updates".into(),
                "Check your internet connection and try again, or browse the releases on GitHub."
                    .into(),
            )
        } else {
            (
                "Lime Updates".into(),
                format!("You're using Lime {}.", env!("CARGO_PKG_VERSION")),
            )
        };
        let checking = state.checking;
        let failed = matches!(state.status, CheckStatus::Failed);
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(PANEL))
            .text_color(rgb(TEXT))
            .track_focus(&self.focus)
            .on_key_down(|event, window, cx| match event.keystroke.key.as_str() {
                "escape" => {
                    window.remove_window();
                    cx.stop_propagation();
                }
                "tab" => {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    cx.stop_propagation();
                }
                _ => {}
            })
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(ACCENT))
                    .child("LIME UPDATES"),
            )
            .child(div().text_size(px(23.)).child(title))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(MUTED))
                    .child(detail),
            )
            .when_some(update.clone(), |root, update| {
                let url = update.changelog;
                root.child(button(
                    "update-changelog",
                    "View changelog ↗",
                    move |_, cx| cx.open_url(&url),
                ))
                .when(update.download.is_none(), |root| {
                    root.child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .child("A download for your platform hasn't been published yet."),
                    )
                })
            })
            .when(update.is_some() && failed && !checking, |root| {
                root.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child("The latest check failed. Showing the last available release."),
                )
            })
            .when(update.is_some() && checking, |root| {
                root.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child("Checking for newer releases…"),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(button("update-close", "Close", |window, _| {
                        window.remove_window()
                    }))
                    .child(
                        if let Some(url) =
                            update.as_ref().and_then(|update| update.download.clone())
                        {
                            button(
                                "update-download",
                                if cfg!(target_os = "macos") {
                                    "Download for macOS ↗"
                                } else {
                                    "Download for Windows ↗"
                                },
                                move |_, cx| cx.open_url(&url),
                            )
                            .bg(rgb(SELECTED))
                            .border_color(rgb(ACCENT))
                            .text_color(rgb(ACCENT))
                        } else if checking {
                            button("update-checking", "Checking…", |_, _| {})
                                .text_color(rgb(MUTED))
                                .cursor_default()
                        } else {
                            button("update-retry", "Check again", |_, cx| check(true, cx))
                        },
                    )
                    .when(failed && update.is_none(), |row| {
                        row.child(button("update-releases", "Releases ↗", |_, cx| {
                            cx.open_url(RELEASES)
                        }))
                    }),
            )
    }
}
