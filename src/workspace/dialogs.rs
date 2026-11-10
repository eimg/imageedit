use gpui::{
    Bounds, ClipboardItem, Context, CursorStyle, Div, FocusHandle, KeyDownEvent, Pixels, Point,
    Window, div, prelude::*, px, rgb, rgba,
};

use super::Workspace;
use super::color::{ColorControl, Hsv, control};
use super::layers::{opacity_from_percent, opacity_percent};
use super::view::{ACCENT, BORDER, CANVAS, CHROME, HOVER, MUTED, PANEL, SELECTED, TEXT};
use crate::document::{Document, FileFormat, LayerId, MAX_LAYER_NAME_CHARS, solid_color};
use crate::preview::Dirty;
use crate::transform::{Anchor, Resample};

use super::selection::MaskEdit;

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
    NewLayer {
        name: Field,
        fill: LayerFill,
    },
    RenameLayer {
        id: LayerId,
        name: Field,
    },
    LayerOpacity {
        id: LayerId,
        percent: Field,
    },
    Scale {
        width: Field,
        height: Field,
        source: (u32, u32),
        layer: bool,
        lock: bool,
        active: usize,
        resample: Resample,
    },
    Canvas {
        width: Field,
        height: Field,
        active: usize,
        anchor: Anchor,
    },
    Radius {
        edit: MaskEdit,
        radius: Field,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LayerFill {
    Transparent,
    Foreground,
    Background,
}

#[derive(Clone, Copy)]
enum FieldMode {
    Digits { max: usize },
    Hex,
    Name,
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

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map(|(index, _)| index)
            .unwrap_or(self.text.len())
    }

    fn replace(&mut self, text: &str) {
        if self.selected {
            self.text.clear();
            self.cursor = 0;
            self.selected = false;
        }
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, text);
        self.cursor += text.chars().count();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, mode: FieldMode, cx: &mut Context<Workspace>) {
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
                        self.insert(text.trim(), mode);
                    }
                }
                _ => {}
            }
        } else if !key.modifiers.alt && !key.modifiers.control && !key.modifiers.platform {
            match key.key.as_str() {
                "backspace" | "delete" if self.selected => self.replace(""),
                "backspace" if self.cursor > 0 => {
                    self.cursor -= 1;
                    let start = self.byte_at(self.cursor);
                    let end = self.byte_at(self.cursor + 1);
                    self.text.replace_range(start..end, "");
                }
                "delete" if self.cursor < self.len() => {
                    let start = self.byte_at(self.cursor);
                    let end = self.byte_at(self.cursor + 1);
                    self.text.replace_range(start..end, "");
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
                        self.len()
                    } else {
                        (self.cursor + 1).min(self.len())
                    };
                    self.selected = false;
                }
                _ => self.insert(key.key_char.as_deref().unwrap_or(&key.key), mode),
            }
        }
    }

    fn insert(&mut self, text: &str, mode: FieldMode) {
        let text = match mode {
            FieldMode::Hex => text.trim_start_matches('#'),
            _ => text,
        };
        let existing = if self.selected { 0 } else { self.len() };
        let allowed = match mode {
            FieldMode::Digits { max } => max,
            FieldMode::Hex => 6,
            FieldMode::Name => MAX_LAYER_NAME_CHARS,
        };
        let ok = match mode {
            FieldMode::Digits { .. } => text.bytes().all(|byte| byte.is_ascii_digit()),
            FieldMode::Hex => text.bytes().all(|byte| byte.is_ascii_hexdigit()),
            FieldMode::Name => text.chars().all(|ch| !ch.is_control()),
        };
        if !text.is_empty() && existing + text.chars().count() <= allowed && ok {
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
            let at = self.byte_at(self.cursor);
            field = field
                .child(self.text[..at].to_owned())
                .child(div().w(px(1.)).h(px(16.)).bg(rgb(ACCENT)))
                .child(self.text[at..].to_owned());
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

    pub(super) fn new_layer_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().filter(|tab| !tab.busy()) else {
            return;
        };
        let next = tab.document.content.layers().len() + 1;
        self.show_form(
            FormKind::NewLayer {
                name: Field::new(format!("Layer {next}")),
                fill: LayerFill::Transparent,
            },
            window,
            cx,
        );
    }

    pub(super) fn rename_layer_form(
        &mut self,
        id: LayerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self
            .active_tab()
            .filter(|tab| !tab.busy())
            .and_then(|tab| tab.document.content.layer(id))
            .map(|layer| layer.name.clone())
        else {
            return;
        };
        self.show_form(
            FormKind::RenameLayer {
                id,
                name: Field::new(name),
            },
            window,
            cx,
        );
    }

    pub(super) fn layer_opacity_form(
        &mut self,
        id: LayerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(opacity) = self
            .active_tab()
            .filter(|tab| !tab.busy())
            .and_then(|tab| tab.document.content.layer(id))
            .map(|layer| layer.opacity)
        else {
            return;
        };
        self.show_form(
            FormKind::LayerOpacity {
                id,
                percent: Field::new(opacity_percent(opacity).to_string()),
            },
            window,
            cx,
        );
    }

    pub(super) fn radius_form(
        &mut self,
        edit: MaskEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_form(
            FormKind::Radius {
                edit,
                radius: Field::new("1".into()),
            },
            window,
            cx,
        );
    }

    pub(super) fn scale_image_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((width, height)) = self.active_tab().map(|tab| tab.document.content.size()) else {
            return;
        };
        self.scale_form(width, height, false, window, cx);
    }

    pub(super) fn scale_layer_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((width, height)) = self.active_tab().and_then(|tab| {
            tab.document
                .content
                .layer(tab.active_layer)
                .map(|layer| layer.pixels.dimensions())
        }) else {
            return;
        };
        self.scale_form(width, height, true, window, cx);
    }

    fn scale_form(
        &mut self,
        width: u32,
        height: u32,
        layer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_form(
            FormKind::Scale {
                width: Field::new(width.to_string()),
                height: Field::new(height.to_string()),
                source: (width, height),
                layer,
                lock: true,
                active: 0,
                resample: Resample::Bilinear,
            },
            window,
            cx,
        );
    }

    pub(super) fn canvas_size_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((width, height)) = self.active_tab().map(|tab| tab.document.content.size()) else {
            return;
        };
        self.show_form(
            FormKind::Canvas {
                width: Field::new(width.to_string()),
                height: Field::new(height.to_string()),
                active: 0,
                anchor: Anchor::Center,
            },
            window,
            cx,
        );
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
            FormKind::NewLayer { name, fill } => {
                let name = name.text.trim();
                if name.is_empty() {
                    form.error = Some("Enter a layer name.".into());
                    cx.notify();
                    return;
                }
                if name.chars().count() > MAX_LAYER_NAME_CHARS {
                    form.error = Some(format!("Use at most {MAX_LAYER_NAME_CHARS} characters."));
                    cx.notify();
                    return;
                }
                let name = name.to_owned();
                let fill = match *fill {
                    LayerFill::Transparent => image::Rgba([0; 4]),
                    LayerFill::Foreground => solid_color(self.colors[0]),
                    LayerFill::Background => solid_color(self.colors[1]),
                };
                let above = self.active_tab().map(|tab| tab.active_layer);
                self.dismiss_form(window, cx);
                let Some(above) = above else {
                    return;
                };
                self.edit_content(
                    move |content, active, _| {
                        if let Some(id) = content.add_layer(above, name, fill) {
                            *active = id;
                            Dirty::Canvas
                        } else {
                            Dirty::None
                        }
                    },
                    window,
                    cx,
                );
            }
            FormKind::RenameLayer { id, name } => {
                let name = name.text.trim();
                if name.is_empty() {
                    form.error = Some("Enter a layer name.".into());
                    cx.notify();
                    return;
                }
                if name.chars().count() > MAX_LAYER_NAME_CHARS {
                    form.error = Some(format!("Use at most {MAX_LAYER_NAME_CHARS} characters."));
                    cx.notify();
                    return;
                }
                let id = *id;
                let name = name.to_owned();
                self.dismiss_form(window, cx);
                self.edit_content(
                    move |content, _, _| {
                        if content.rename_layer(id, name) {
                            Dirty::Unpainted
                        } else {
                            Dirty::None
                        }
                    },
                    window,
                    cx,
                );
            }
            FormKind::Scale {
                width,
                height,
                source,
                layer,
                lock,
                active,
                resample,
            } => {
                let entered = [width, height]
                    .each_ref()
                    .map(|field| field.text.parse::<u32>().unwrap_or(0));
                let (mut width, mut height) = (entered[0], entered[1]);
                if *lock {
                    if *active == 0 {
                        height = locked_edge(width, source.1, source.0);
                    } else {
                        width = locked_edge(height, source.0, source.1);
                    }
                }
                if let Err(error) = Document::validate_size(width, height) {
                    form.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
                let (layer, resample) = (*layer, *resample);
                self.dismiss_form(window, cx);
                self.apply_scale(width, height, resample, layer, window, cx);
            }
            FormKind::Canvas {
                width,
                height,
                anchor,
                ..
            } => {
                let width = width.text.parse().unwrap_or(0);
                let height = height.text.parse().unwrap_or(0);
                if let Err(error) = Document::validate_size(width, height) {
                    form.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
                let anchor = *anchor;
                self.dismiss_form(window, cx);
                self.apply_canvas_size(width, height, anchor, window, cx);
            }
            FormKind::Radius { edit, radius } => {
                let Ok(radius) = radius.text.parse::<u32>() else {
                    form.error = Some("Enter a radius from 0 to 128.".into());
                    cx.notify();
                    return;
                };
                if radius > 128 {
                    form.error = Some("Enter a radius from 0 to 128.".into());
                    cx.notify();
                    return;
                }
                let edit = *edit;
                self.dismiss_form(window, cx);
                self.reshape_selection(edit, radius, window, cx);
            }
            FormKind::LayerOpacity { id, percent } => {
                let Some(percent) = percent
                    .text
                    .parse()
                    .ok()
                    .filter(|percent: &u32| *percent <= 100)
                else {
                    form.error = Some("Enter an opacity from 0 to 100.".into());
                    cx.notify();
                    return;
                };
                let id = *id;
                let opacity = opacity_from_percent(percent);
                self.dismiss_form(window, cx);
                self.edit_content(
                    move |content, _, _| {
                        if content.set_opacity(id, opacity) {
                            content.layer_dirty(id)
                        } else {
                            Dirty::None
                        }
                    },
                    window,
                    cx,
                );
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
                            fields[*active].handle_key(event, FieldMode::Digits { max: 5 }, cx);
                        }
                    }
                    FormKind::JpegQuality { quality } => {
                        quality.handle_key(event, FieldMode::Digits { max: 3 }, cx)
                    }
                    FormKind::NewLayer { name, .. } | FormKind::RenameLayer { name, .. } => {
                        name.handle_key(event, FieldMode::Name, cx)
                    }
                    FormKind::LayerOpacity { percent, .. } => {
                        percent.handle_key(event, FieldMode::Digits { max: 3 }, cx)
                    }
                    FormKind::Radius { radius, .. } => {
                        radius.handle_key(event, FieldMode::Digits { max: 3 }, cx)
                    }
                    FormKind::Scale {
                        width,
                        height,
                        active,
                        ..
                    }
                    | FormKind::Canvas {
                        width,
                        height,
                        active,
                        ..
                    } => {
                        if key == "tab" {
                            *active = 1 - *active;
                        } else if *active == 0 {
                            width.handle_key(event, FieldMode::Digits { max: 5 }, cx);
                        } else {
                            height.handle_key(event, FieldMode::Digits { max: 5 }, cx);
                        }
                    }
                    FormKind::Color { hex, hsv, .. } => {
                        let previous = hex.color();
                        hex.handle_key(event, FieldMode::Hex, cx);
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
                FormKind::NewLayer { name, fill } => {
                    let fills = div().flex().gap_2().children(
                        [
                            ("Transparent", LayerFill::Transparent),
                            ("Foreground", LayerFill::Foreground),
                            ("Background", LayerFill::Background),
                        ]
                        .into_iter()
                        .map(|(label, value)| {
                            form_button(label, *fill == value).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(Form {
                                        kind: FormKind::NewLayer { fill, .. },
                                        ..
                                    }) = &mut this.form
                                    {
                                        *fill = value;
                                        cx.notify();
                                    }
                                },
                            ))
                        }),
                    );
                    (
                        "New Layer",
                        "Create",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(div().child("Name").child(name.view(true).mt_1()))
                            .child(div().child("Fill").child(fills.mt_1())),
                    )
                }
                FormKind::RenameLayer { name, .. } => (
                    "Rename Layer",
                    "Rename",
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child("Name")
                        .child(name.view(true)),
                ),
                FormKind::LayerOpacity { percent, .. } => (
                    "Layer Opacity",
                    "Set Opacity",
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child("Opacity (0–100%)")
                        .child(percent.view(true)),
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
                FormKind::Scale {
                    width,
                    height,
                    layer,
                    lock,
                    active,
                    resample,
                    ..
                } => (
                    if *layer { "Scale Layer" } else { "Scale Image" },
                    "Scale",
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(size_fields(width, height, *active, cx))
                        .child(
                            form_button("Lock aspect ratio", *lock).on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(Form {
                                        kind: FormKind::Scale { lock, .. },
                                        ..
                                    }) = &mut this.form
                                    {
                                        *lock = !*lock;
                                        cx.notify();
                                    }
                                },
                            )),
                        )
                        .child(resample_choices(*resample, cx)),
                ),
                FormKind::Radius { edit, radius } => (
                    match edit {
                        MaskEdit::Feather => "Feather Selection",
                        MaskEdit::Grow => "Grow Selection",
                        MaskEdit::Shrink => "Shrink Selection",
                    },
                    "Apply",
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child("Radius (px)")
                        .child(radius.view(true)),
                ),
                FormKind::Canvas {
                    width,
                    height,
                    active,
                    anchor,
                } => (
                    "Canvas Size",
                    "Resize",
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(size_fields(width, height, *active, cx))
                        .child(anchor_choices(*anchor, cx)),
                ),
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

fn locked_edge(edited: u32, other: u32, source: u32) -> u32 {
    if edited == 0 || source == 0 {
        return 1;
    }
    ((u64::from(edited) * u64::from(other) + u64::from(source) / 2) / u64::from(source)).max(1)
        as u32
}

fn size_fields(width: &Field, height: &Field, active: usize, cx: &Context<Workspace>) -> Div {
    div()
        .flex()
        .gap_3()
        .child(dimension_field("Width (px)", width, active == 0, 0, cx))
        .child(dimension_field("Height (px)", height, active == 1, 1, cx))
}

fn dimension_field(
    label: &'static str,
    field: &Field,
    selected: bool,
    index: usize,
    cx: &Context<Workspace>,
) -> Div {
    div()
        .flex_1()
        .child(label)
        .child(field.view(selected).id(label).mt_1().on_click(cx.listener(
            move |this, _, _, cx| {
                let Some(form) = &mut this.form else {
                    return;
                };
                let (active, width, height) = match &mut form.kind {
                    FormKind::Scale {
                        active,
                        width,
                        height,
                        ..
                    }
                    | FormKind::Canvas {
                        active,
                        width,
                        height,
                        ..
                    } => (active, width, height),
                    _ => return,
                };
                *active = index;
                if index == 0 {
                    width.selected = true;
                } else {
                    height.selected = true;
                }
                cx.notify();
            },
        )))
}

fn resample_choices(current: Resample, cx: &Context<Workspace>) -> Div {
    div().flex().gap_2().children(
        [
            (Resample::Nearest, "Nearest"),
            (Resample::Bilinear, "Bilinear"),
            (Resample::Bicubic, "Bicubic"),
        ]
        .map(|(value, label)| {
            form_button(label, current == value).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(Form {
                    kind: FormKind::Scale { resample, .. },
                    ..
                }) = &mut this.form
                {
                    *resample = value;
                    cx.notify();
                }
            }))
        }),
    )
}

fn anchor_choices(current: Anchor, cx: &Context<Workspace>) -> Div {
    let anchors = [
        Anchor::NorthWest,
        Anchor::North,
        Anchor::NorthEast,
        Anchor::West,
        Anchor::Center,
        Anchor::East,
        Anchor::SouthWest,
        Anchor::South,
        Anchor::SouthEast,
    ];
    div()
        .flex()
        .flex_wrap()
        .gap_1()
        .children(anchors.map(|anchor| {
            let label = match anchor {
                Anchor::NorthWest => "Top left",
                Anchor::North => "Top",
                Anchor::NorthEast => "Top right",
                Anchor::West => "Left",
                Anchor::Center => "Center",
                Anchor::East => "Right",
                Anchor::SouthWest => "Bottom left",
                Anchor::South => "Bottom",
                Anchor::SouthEast => "Bottom right",
            };
            form_button(label, current == anchor).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(Form {
                    kind: FormKind::Canvas { anchor: chosen, .. },
                    ..
                }) = &mut this.form
                {
                    *chosen = anchor;
                    cx.notify();
                }
            }))
        }))
}
