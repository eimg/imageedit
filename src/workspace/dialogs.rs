use gpui::{
    Bounds, ClipboardItem, Context, CursorStyle, Div, FocusHandle, KeyDownEvent, Pixels, Point,
    Window, div, prelude::*, px, rgb, rgba,
};

use super::Workspace;
use super::color::{ColorControl, Hsv, control};
use super::view::{ACCENT, BORDER, CANVAS, CHROME, HOVER, MUTED, PANEL, SELECTED, TEXT};
use crate::document::{Document, FileFormat, solid_color};

pub(super) struct Form {
    focus: FocusHandle,
    kind: FormKind,
    error: Option<String>,
}

enum FormKind {
    New {
        fields: [Field; 2],
        active: usize,
        transparent: bool,
    },
    Color {
        swatch: usize,
        hex: Field,
        hsv: Hsv,
        drag: Option<ColorControl>,
    },
    JpegQuality {
        quality: Field,
    },
}

struct Field {
    text: String,
    cursor: usize,
    selected: bool,
}

impl Field {
    fn new(text: String) -> Self {
        Self {
            cursor: text.len(),
            text,
            selected: true,
        }
    }

    fn replace(&mut self, text: &str) {
        if self.selected {
            self.text.clear();
            self.cursor = 0;
            self.selected = false;
        }
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, hex: bool, cx: &mut Context<Workspace>) {
        let key = &event.keystroke;
        if key.modifiers.secondary() {
            match key.key.as_str() {
                "a" => self.selected = true,
                "c" | "x" if self.selected => {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.text.clone()));
                    if key.key == "x" {
                        self.replace("");
                    }
                }
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.insert(text.trim(), hex);
                    }
                }
                _ => {}
            }
        } else if !key.modifiers.alt && !key.modifiers.control && !key.modifiers.platform {
            match key.key.as_str() {
                "backspace" | "delete" if self.selected => self.replace(""),
                "backspace" if self.cursor > 0 => {
                    self.cursor -= 1;
                    self.text.remove(self.cursor);
                }
                "delete" if self.cursor < self.text.len() => {
                    self.text.remove(self.cursor);
                }
                "left" | "home" => {
                    self.cursor = if self.selected || key.key == "home" {
                        0
                    } else {
                        self.cursor.saturating_sub(1)
                    };
                    self.selected = false;
                }
                "right" | "end" => {
                    self.cursor = if self.selected || key.key == "end" {
                        self.text.len()
                    } else {
                        (self.cursor + 1).min(self.text.len())
                    };
                    self.selected = false;
                }
                _ => self.insert(key.key_char.as_deref().unwrap_or(&key.key), hex),
            }
        }
    }

    fn insert(&mut self, text: &str, hex: bool) {
        let text = if hex {
            text.trim_start_matches('#')
        } else {
            text
        };
        let existing = if self.selected { 0 } else { self.text.len() };
        let allowed = if hex { 6 } else { 5 };
        if !text.is_empty()
            && existing + text.len() <= allowed
            && text.bytes().all(|byte| {
                if hex {
                    byte.is_ascii_hexdigit()
                } else {
                    byte.is_ascii_digit()
                }
            })
        {
            self.replace(text);
        }
    }

    fn color(&self) -> Option<u32> {
        (self.text.len() == 6)
            .then(|| u32::from_str_radix(&self.text, 16).ok())
            .flatten()
    }

    fn view(&self, active: bool) -> Div {
        let mut field = div()
            .h(px(36.))
            .px_3()
            .flex()
            .items_center()
            .cursor(CursorStyle::IBeam)
            .bg(rgb(CANVAS))
            .border_1()
            .border_color(rgb(if active { ACCENT } else { BORDER }))
            .rounded_md();
        if !active {
            field.child(self.text.clone())
        } else if self.selected && !self.text.is_empty() {
            field.child(
                div()
                    .bg(rgb(ACCENT))
                    .text_color(rgb(CANVAS))
                    .child(self.text.clone()),
            )
        } else {
            field = field
                .child(self.text[..self.cursor].to_owned())
                .child(div().w(px(1.)).h(px(16.)).bg(rgb(ACCENT)))
                .child(self.text[self.cursor..].to_owned());
            field
        }
    }
}

impl Workspace {
    pub(super) fn new_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating {
            return;
        }
        self.show_form(
            FormKind::New {
                fields: [Field::new("1280".into()), Field::new("720".into())],
                active: 0,
                transparent: false,
            },
            window,
            cx,
        );
    }

    pub(super) fn color_form(
        &mut self,
        swatch: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_form(
            FormKind::Color {
                swatch,
                hex: Field::new(format!("{:06X}", self.colors[swatch])),
                hsv: Hsv::from_rgb(self.colors[swatch]),
                drag: None,
            },
            window,
            cx,
        );
    }

    pub(super) fn jpeg_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().is_some_and(|tab| !tab.busy()) {
            let quality = Field::new(self.jpeg_quality.to_string());
            self.show_form(FormKind::JpegQuality { quality }, window, cx);
        }
    }

    fn show_form(&mut self, kind: FormKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open || self.form.is_some() {
            return;
        }
        let focus = cx.focus_handle();
        focus.focus(window);
        self.form = Some(Form {
            focus,
            kind,
            error: None,
        });
        cx.notify();
    }

    fn dismiss_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        self.focus.focus(window);
        cx.notify();
    }

    fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else {
            return;
        };
        match &form.kind {
            FormKind::New {
                fields,
                transparent,
                ..
            } => {
                let [width, height] = fields
                    .each_ref()
                    .map(|field| field.text.parse().unwrap_or(0));
                if let Err(error) = Document::validate_size(width, height) {
                    form.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
                let background = if *transparent {
                    image::Rgba([0; 4])
                } else {
                    solid_color(self.colors[1])
                };
                self.dismiss_form(window, cx);
                self.create_image(width, height, background, window, cx);
            }
            FormKind::Color { swatch, hex, .. } => {
                let Some(color) = hex.color() else {
                    form.error = Some("Enter six hexadecimal digits, such as 76B7FF.".into());
                    cx.notify();
                    return;
                };
                let swatch = *swatch;
                self.set_color(swatch, color);
                self.dismiss_form(window, cx);
            }
            FormKind::JpegQuality { quality } => {
                let Some(quality) = quality.text.parse().ok().filter(|q| (1..=100).contains(q))
                else {
                    form.error = Some("Enter a quality from 1 to 100.".into());
                    cx.notify();
                    return;
                };
                self.jpeg_quality = quality;
                self.dismiss_form(window, cx);
                self.export(FileFormat::Jpeg, quality, window, cx);
            }
        }
    }

    fn form_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else {
            return;
        };
        match event.keystroke.key.as_str() {
            "escape" => self.dismiss_form(window, cx),
            "enter" => self.submit_form(window, cx),
            key => {
                form.error = None;
                match &mut form.kind {
                    FormKind::New { fields, active, .. } => {
                        if key == "tab" {
                            *active = 1 - *active;
                            fields[*active].selected = true;
                        } else {
                            fields[*active].handle_key(event, false, cx);
                        }
                    }
                    FormKind::JpegQuality { quality } => quality.handle_key(event, false, cx),
                    FormKind::Color { hex, hsv, .. } => {
                        let previous = hex.color();
                        hex.handle_key(event, true, cx);
                        if let Some(color) = hex.color().filter(|color| Some(*color) != previous) {
                            *hsv = Hsv::from_rgb(color);
                        }
                    }
                }
                cx.notify();
            }
        }
    }

    pub(super) fn drag_color(
        &mut self,
        control: ColorControl,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        start: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(Form {
            kind: FormKind::Color { hex, hsv, drag, .. },
            error,
            ..
        }) = &mut self.form
        else {
            return;
        };
        if (start || *drag == Some(control))
            && let Some(selected) = control.at(*hsv, bounds, position, start)
        {
            *hsv = selected;
            *hex = Field::new(format!("{:06X}", selected.rgb()));
            *drag = Some(control);
            *error = None;
            cx.notify();
        }
    }

    pub(super) fn end_color_drag(
        &mut self,
        control: ColorControl,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.drag_color(control, bounds, position, false, cx);
        if let Some(Form {
            kind: FormKind::Color { drag, .. },
            ..
        }) = &mut self.form
            && *drag == Some(control)
        {
            *drag = None;
        }
    }

    pub(super) fn form_overlay(&self, cx: &Context<Self>) -> Option<Div> {
        let form = self.form.as_ref()?;
        let (title, accept, content) =
            match &form.kind {
                FormKind::New {
                    fields,
                    active,
                    transparent,
                } => {
                    let dimensions = div().flex().gap_3().children(
                        ["Width (px)", "Height (px)"].into_iter().enumerate().map(
                            |(index, label)| {
                                div().flex_1().child(label).child(
                                    fields[index]
                                        .view(*active == index)
                                        .id(label)
                                        .mt_1()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(Form {
                                                kind: FormKind::New { active, fields, .. },
                                                ..
                                            }) = &mut this.form
                                            {
                                                *active = index;
                                                fields[index].selected = true;
                                                cx.notify();
                                            }
                                        })),
                                )
                            },
                        ),
                    );
                    let background = div().flex().gap_2().children(
                        [("Background color", false), ("Transparent", true)]
                            .into_iter()
                            .map(|(label, value)| {
                                form_button(label, *transparent == value).on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if let Some(Form {
                                            kind: FormKind::New { transparent, .. },
                                            ..
                                        }) = &mut this.form
                                        {
                                            *transparent = value;
                                            cx.notify();
                                        }
                                    },
                                ))
                            }),
                    );
                    (
                        "New Image",
                        "Create",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(dimensions)
                            .child(div().child("Canvas background").child(background.mt_1())),
                    )
                }
                FormKind::JpegQuality { quality } => (
                    "Export as JPEG",
                    "Choose File…",
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child("Quality (1–100)")
                        .child(quality.view(true))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child("Higher values keep more detail and make larger files. Transparent areas become white."),
                        ),
                ),
                FormKind::Color {
                    swatch, hex, hsv, ..
                } => {
                    let color = hsv.rgb();
                    let recent = div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_color(rgb(MUTED)).child("Recent colors"))
                        .when_else(
                            self.recent_colors.is_empty(),
                            |row| {
                                row.child(
                                    div()
                                        .h(px(24.))
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child("Colors you set or sample appear here."),
                                )
                            },
                            |row| {
                                row.child(div().flex().gap_1().children(
                                    self.recent_colors.iter().copied().map(|value| {
                                        div()
                                            .id(("recent-color", value as usize))
                                            .size(px(23.))
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(rgb(if color == value {
                                                ACCENT
                                            } else {
                                                BORDER
                                            }))
                                            .bg(rgb(value))
                                            .cursor_pointer()
                                            .hover(|chip| chip.border_color(rgb(TEXT)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(Form {
                                                    kind: FormKind::Color { hex, hsv, .. },
                                                    error,
                                                    ..
                                                }) = &mut this.form
                                                {
                                                    *hex = Field::new(format!("{value:06X}"));
                                                    *hsv = Hsv::from_rgb(value);
                                                    *error = None;
                                                    cx.notify();
                                                }
                                            }))
                                    }),
                                ))
                            },
                        );
                    let value = div()
                        .flex()
                        .items_end()
                        .gap_3()
                        .child(div().flex_1().child("Hex color").child(
                            hex.view(true).id("hex").mt_1().on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(Form {
                                        kind: FormKind::Color { hex, .. },
                                        ..
                                    }) = &mut this.form
                                    {
                                        hex.selected = true;
                                        cx.notify();
                                    }
                                },
                            )),
                        ))
                        .child(
                            div()
                                .size(px(36.))
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(BORDER))
                                .bg(rgb(color)),
                        );
                    let title = if *swatch == 0 {
                        "Foreground Color"
                    } else {
                        "Background Color"
                    };
                    (
                        title,
                        "Set Color",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(div().flex().justify_center().child(control(
                                ColorControl::Wheel,
                                *hsv,
                                cx,
                            )))
                            .child(div().child("Brightness").child(control(
                                ColorControl::Brightness,
                                *hsv,
                                cx,
                            )))
                            .child(value)
                            .child(recent),
                    )
                }
            };

        Some(
            div()
                .absolute()
                .size_full()
                .occlude()
                .bg(rgba(0x0b0d12bb))
                .flex()
                .items_center()
                .justify_center()
                .track_focus(&form.focus)
                .on_key_down(cx.listener(Self::form_key))
                .child(
                    div()
                        .w(px(380.))
                        .p_4()
                        .max_h_full()
                        .id("form-panel")
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(PANEL))
                        .shadow_lg()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(content)
                        .children(form.error.as_ref().map(|error| {
                            div()
                                .text_color(rgb(0xffb8ad))
                                .text_sm()
                                .child(error.clone())
                        }))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(form_button("Cancel", false).on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.dismiss_form(window, cx);
                                    },
                                )))
                                .child(form_button(accept, true).on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.submit_form(window, cx);
                                    },
                                ))),
                        ),
                ),
        )
    }
}

pub(super) fn form_button(label: &'static str, primary: bool) -> gpui::Stateful<Div> {
    div()
        .id(label)
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(if primary { ACCENT } else { BORDER }))
        .bg(rgb(if primary { SELECTED } else { CHROME }))
        .text_color(rgb(if primary { TEXT } else { MUTED }))
        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .cursor_pointer()
        .child(label)
}
