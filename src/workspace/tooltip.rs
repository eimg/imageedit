use gpui::{AnyView, App, Div, SharedString, Window, div, prelude::*, px, rgb};

use super::view::{BORDER, CHROME, MUTED, TEXT};

pub(super) struct Tooltip {
    title: SharedString,
    detail: Option<&'static str>,
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CHROME))
            .text_color(rgb(TEXT))
            .text_size(px(12.))
            .child(self.title.clone())
            .when_some(self.detail, |tooltip, detail| {
                tooltip.child(
                    div()
                        .max_w(px(260.))
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child(detail),
                )
            })
    }
}

pub(super) fn tooltip(
    title: impl Into<SharedString> + 'static,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let title = title.into();
    move |_, cx| {
        let title = title.clone();
        cx.new(|_| Tooltip {
            title,
            detail: None,
        })
        .into()
    }
}

pub(super) fn tooltip_with_detail(
    title: impl Into<SharedString> + 'static,
    detail: &'static str,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let title = title.into();
    move |_, cx| {
        let title = title.clone();
        cx.new(|_| Tooltip {
            title,
            detail: Some(detail),
        })
        .into()
    }
}

pub(super) fn label_view(text: SharedString) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CHROME))
        .text_color(rgb(TEXT))
        .text_size(px(12.))
        .child(text)
}
