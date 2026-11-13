use std::{cell::Cell, path::PathBuf, rc::Rc, sync::Arc};

use gpui::{
    Bounds, ClipboardItem, Context, CursorStyle, Div, FocusHandle, KeyDownEvent, Keystroke,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, PathBuilder, Pixels,
    Point, Window, canvas, div, fill, img, point, prelude::*, px, rgb, rgba, size,
};

use super::Workspace;
use super::adjust_ui::AdjustKind;
use super::color::{ColorControl, Hsv, control};
use super::filter_ui::FilterKind;
use super::layers::{opacity_from_percent, opacity_percent};
use super::pdf_ui::PdfTarget;
use super::view::{ACCENT, BORDER, CANVAS, CHROME, HOVER, MUTED, PANEL, SELECTED, TEXT};
use crate::adjustments::{
    Adjustment, BalanceRange, ColorBalance, Curve, Curves, GAMMA_MAX, GAMMA_MIN, GAMMA_ONE,
    Histogram, Levels, MapChannel,
};
use crate::document::{
    Document, FileFormat, LayerId, MAX_LAYER_NAME_CHARS, file_name, solid_color,
};
use crate::filters::{self, FilterSpec};
use crate::pdf::{self, PdfBackground, PdfCanvas, PdfMode, PdfOptions, PdfSource};
use crate::preview::Dirty;
use crate::transform::{Anchor, Resample};

use super::selection::MaskEdit;

pub(super) struct Form {
    focus: FocusHandle,
    kind: FormKind,
    pub(super) error: Option<String>,
}

#[derive(Clone, Copy)]
enum ColorTarget {
    Swatch(usize),
    GradientStop { revision: u64, index: usize },
}

enum FormKind {
    New {
        fields: [Field; 2],
        active: usize,
        transparent: bool,
    },
    Color {
        target: ColorTarget,
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
    Adjustment {
        operation: AdjustKind,
        brightness: i16,
        contrast: i16,
        hue: i16,
        saturation: i16,
        lightness: i16,
        balance_range: BalanceRange,
        balance: ColorBalance,
        map_channel: MapChannel,
        levels: Levels,
        curves: Curves,
        curve_drag: Option<(u8, (i16, i16))>,
    },
    Filter {
        kind: FilterKind,
        radius: i16,
        amount: i16,
        offset_x: i16,
        offset_y: i16,
        opacity: i16,
        color: image::Rgba<u8>,
    },
    PdfImport {
        path: PathBuf,
        source: Arc<PdfSource>,
        dpi: Field,
        from: Field,
        to: Field,
        all_pages: bool,
        mode: PdfMode,
        background: PdfBackground,
        canvas: PdfCanvas,
        target: PdfTarget,
        active: usize,
    },
}

impl FormKind {
    fn adjustment_parameters(value: Adjustment) -> Self {
        let mut form = Self::adjustment_defaults(AdjustKind::of(value));
        if let Self::Adjustment {
            brightness,
            contrast,
            hue,
            saturation,
            lightness,
            balance,
            levels,
            curves,
            ..
        } = &mut form
        {
            match value {
                Adjustment::BrightnessContrast {
                    brightness: b,
                    contrast: c,
                } => {
                    *brightness = b;
                    *contrast = c;
                }
                Adjustment::HueSaturation {
                    hue: h,
                    saturation: s,
                    lightness: l,
                } => {
                    *hue = h;
                    *saturation = s;
                    *lightness = l;
                }
                Adjustment::ColorBalance(value) => *balance = value,
                Adjustment::Levels(value) => *levels = value,
                Adjustment::Curves(value) => *curves = value,
                Adjustment::Desaturate | Adjustment::Invert => {}
            }
        }
        form
    }

    fn adjustment_value(&self) -> Option<Adjustment> {
        let Self::Adjustment {
            operation,
            brightness,
            contrast,
            hue,
            saturation,
            lightness,
            balance,
            levels,
            curves,
            ..
        } = self
        else {
            return None;
        };
        Some(match operation {
            AdjustKind::BrightnessContrast => Adjustment::BrightnessContrast {
                brightness: *brightness,
                contrast: *contrast,
            },
            AdjustKind::HueSaturation => Adjustment::HueSaturation {
                hue: *hue,
                saturation: *saturation,
                lightness: *lightness,
            },
            AdjustKind::Desaturate => Adjustment::Desaturate,
            AdjustKind::Invert => Adjustment::Invert,
            AdjustKind::ColorBalance => Adjustment::ColorBalance(*balance),
            AdjustKind::Levels => Adjustment::Levels(*levels),
            AdjustKind::Curves => Adjustment::Curves(*curves),
        })
    }

    fn filter_parameters(value: FilterSpec) -> Self {
        let mut form = Self::filter_defaults(FilterKind::of(value), image::Rgba([0, 0, 0, 255]));
        if let Self::Filter {
            radius,
            amount,
            offset_x,
            offset_y,
            opacity,
            color,
            ..
        } = &mut form
        {
            match value {
                FilterSpec::Blur { radius: r } => *radius = r as i16,
                FilterSpec::Sharpen {
                    radius: r,
                    amount: a,
                } => {
                    *radius = r as i16;
                    *amount = i16::from(a);
                }
                FilterSpec::Shadow {
                    radius: r,
                    offset,
                    opacity: o,
                    color: c,
                } => {
                    *radius = r as i16;
                    *offset_x = offset.0 as i16;
                    *offset_y = offset.1 as i16;
                    *opacity = i16::from(o);
                    *color = c;
                }
            }
        }
        form
    }

    fn adjustment_defaults(operation: AdjustKind) -> Self {
        Self::Adjustment {
            operation,
            brightness: 0,
            contrast: 0,
            hue: 0,
            saturation: 0,
            lightness: 0,
            balance_range: BalanceRange::Midtones,
            balance: ColorBalance::default(),
            map_channel: MapChannel::Value,
            levels: Levels::default(),
            curves: Curves::default(),
            curve_drag: None,
        }
    }

    fn filter_defaults(kind: FilterKind, color: image::Rgba<u8>) -> Self {
        Self::Filter {
            kind,
            radius: if kind == FilterKind::Sharpen { 2 } else { 4 },
            amount: 50,
            offset_x: 8,
            offset_y: 8,
            opacity: 75,
            color,
        }
    }

    fn filter_value(&self) -> Option<FilterSpec> {
        match self {
            Self::Filter {
                kind,
                radius,
                amount,
                offset_x,
                offset_y,
                opacity,
                color,
            } => Some(filter_spec(
                *kind, *radius, *amount, *offset_x, *offset_y, *opacity, *color,
            )),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LayerFill {
    Transparent,
    Foreground,
    Background,
}

#[derive(Clone, Copy)]
pub(super) enum FieldMode {
    Digits { max: usize },
    Hex,
    Name,
    FontQuery,
}

#[derive(Clone)]
pub(super) struct Field {
    text: String,
    cursor: usize,
    selected: bool,
}

impl Field {
    pub(super) fn new(text: String) -> Self {
        Self {
            cursor: text.chars().count(),
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
        self.handle_keystroke(&event.keystroke, mode, cx);
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn select_all(&mut self) {
        self.selected = true;
        self.cursor = self.len();
    }

    pub(super) fn caret_item(&self) -> usize {
        if self.selected && !self.text.is_empty() {
            0
        } else {
            1
        }
    }

    pub(super) fn clipboard(&mut self, key: &str, mode: FieldMode, cx: &mut Context<Workspace>) {
        match key {
            "a" => self.select_all(),
            "c" | "x" if self.selected => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.text.clone()));
                if key == "x" {
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
    }

    pub(super) fn handle_keystroke(
        &mut self,
        key: &Keystroke,
        mode: FieldMode,
        cx: &mut Context<Workspace>,
    ) {
        if key.modifiers.secondary() {
            self.clipboard(&key.key, mode, cx);
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
                _ => {
                    if let Some(text) = key.key_char.as_deref() {
                        self.insert(text, mode);
                    } else if key.key == "space" {
                        self.insert(" ", mode);
                    }
                }
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
            FieldMode::FontQuery => crate::text::MAX_FAMILY_CHARS,
        };
        let ok = match mode {
            FieldMode::Digits { .. } => text.bytes().all(|byte| byte.is_ascii_digit()),
            FieldMode::Hex => text.bytes().all(|byte| byte.is_ascii_hexdigit()),
            FieldMode::Name | FieldMode::FontQuery => text.chars().all(|ch| !ch.is_control()),
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

    pub(super) fn view(&self, active: bool) -> Div {
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
                .child(div().w(px(1.)).h(px(16.)).flex_shrink_0().bg(rgb(ACCENT)))
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
        let initial = if swatch == super::text_ui::TEXT_SWATCH {
            let Some(color) = self.text_color_value() else {
                return;
            };
            color
        } else {
            self.colors[swatch]
        };
        self.show_form(
            FormKind::Color {
                target: ColorTarget::Swatch(swatch),
                hex: Field::new(format!("{initial:06X}")),
                hsv: Hsv::from_rgb(initial),
                drag: None,
            },
            window,
            cx,
        );
    }

    pub(super) fn gradient_stop_color_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stop) = self.selected_gradient_stop() else {
            return;
        };
        let initial = u32::from_be_bytes([0, stop.color[0], stop.color[1], stop.color[2]]);
        self.show_form(
            FormKind::Color {
                target: ColorTarget::GradientStop {
                    revision: self.gradient_revision,
                    index: self.gradient_selected_stop,
                },
                hex: Field::new(format!("{initial:06X}")),
                hsv: Hsv::from_rgb(initial),
                drag: None,
            },
            window,
            cx,
        );
    }

    pub(super) fn jpeg_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_pending_file_edit(cx) {
            return;
        }
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
        if self.finish_text(window, cx) {
            self.defer_after_text(|this, window, cx| this.scale_image_form(window, cx));
            return;
        }
        if self.refuse_text_pixels(true, cx) {
            return;
        }
        let Some((width, height)) = self.active_tab().map(|tab| tab.document.content.size()) else {
            return;
        };
        self.scale_form(width, height, false, window, cx);
    }

    pub(super) fn scale_layer_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finish_text(window, cx) {
            self.defer_after_text(|this, window, cx| this.scale_layer_form(window, cx));
            return;
        }
        if self.refuse_text_pixels(false, cx) {
            return;
        }
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

    pub(super) fn pdf_import_form(
        &mut self,
        path: PathBuf,
        source: Arc<PdfSource>,
        target: PdfTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.dialog_open || self.form.is_some() || self.creating || self.pdf_job.is_some() {
            self.errors
                .push("Wait for the current file operation to finish.".into());
            cx.notify();
            return false;
        }
        if source.info.pages == 0 {
            self.errors.push("The PDF has no pages.".into());
            cx.notify();
            return false;
        }
        self.show_form(
            FormKind::PdfImport {
                path,
                dpi: Field::new(self.pdf_preferences.dpi.to_string()),
                from: Field::new("1".into()),
                to: Field::new(source.info.pages.to_string()),
                all_pages: true,
                mode: if target == PdfTarget::NewDocument {
                    self.pdf_preferences.mode
                } else {
                    PdfMode::Layers
                },
                background: self.pdf_preferences.background,
                canvas: self.pdf_preferences.canvas,
                target,
                source,
                active: 0,
            },
            window,
            cx,
        );
        true
    }

    fn show_form(&mut self, kind: FormKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open || self.form.is_some() {
            return;
        }
        if !matches!(kind, FormKind::Color { .. }) && self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| this.show_form(kind, window, cx));
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

    pub(super) fn dismiss_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_adjustment(window, cx);
        self.clear_filter(window, cx);
        self.form = None;
        self.focus.focus(window);
        cx.notify();
    }

    pub(super) fn dismiss_effect_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.as_ref().is_some_and(|form| {
            matches!(
                form.kind,
                FormKind::Adjustment { .. } | FormKind::Filter { .. }
            )
        }) {
            self.dismiss_form(window, cx);
        }
    }

    pub(super) fn adjustment_form(
        &mut self,
        operation: AdjustKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| {
                this.adjustment_form(operation, window, cx);
            });
            return;
        }
        let form = FormKind::adjustment_parameters(self.effect_memory.adjustment(operation));
        let Some(adjustment) = form.adjustment_value() else {
            return;
        };
        if self.dialog_open || self.form.is_some() || !self.begin_adjustment(adjustment, window, cx)
        {
            return;
        }
        self.show_form(form, window, cx);
        if matches!(operation, AdjustKind::Levels | AdjustKind::Curves) {
            self.start_adjustment_histogram(window, cx);
        }
    }

    pub(super) fn filter_form(
        &mut self,
        kind: FilterKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| this.filter_form(kind, window, cx));
            return;
        }
        if self.dialog_open || self.form.is_some() {
            return;
        }
        let color = solid_color(self.colors[0]);
        let form = self.effect_memory.filter(kind).map_or_else(
            || FormKind::filter_defaults(kind, color),
            FormKind::filter_parameters,
        );
        let Some(spec) = form.filter_value() else {
            return;
        };
        if !self.begin_filter(spec, window, cx) {
            return;
        }
        self.show_form(form, window, cx);
    }

    fn reset_effect_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let foreground = solid_color(self.colors[0]);
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let adjustment = match form.kind {
            FormKind::Adjustment { operation, .. } => {
                form.kind = FormKind::adjustment_defaults(operation);
                Some(operation.defaults())
            }
            FormKind::Filter { kind, .. } => {
                form.kind = FormKind::filter_defaults(kind, foreground);
                None
            }
            _ => return,
        };
        let filter = form.kind.filter_value();
        form.error = None;
        self.effect_before = false;
        if let Some(adjustment) = adjustment {
            self.set_adjustment(adjustment, window, cx);
        }
        if let Some(filter) = filter {
            self.set_filter(filter, window, cx);
        }
        cx.notify();
    }

    fn use_shadow_foreground(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let foreground = solid_color(self.colors[0]);
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let FormKind::Filter {
            kind: FilterKind::Shadow,
            color,
            ..
        } = &mut form.kind
        else {
            return;
        };
        *color = foreground;
        let Some(spec) = form.kind.filter_value() else {
            return;
        };
        self.set_filter(spec, window, cx);
        cx.notify();
    }

    fn effect_preview_panel(
        &self,
        images: Option<super::EffectImages>,
        updating: bool,
        cx: &Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(match images {
                Some(images) => div()
                    .h(px(180.))
                    .w_full()
                    .bg(rgb(CANVAS))
                    .rounded_md()
                    .child(
                        img(if self.effect_before {
                            images.before
                        } else {
                            images.after
                        })
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                    ),
                None => div().h(px(180.)).w_full().child("Preview…"),
            })
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                form_button("Before", self.effect_before).on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.effect_before = true;
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(form_button("After", !self.effect_before).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.effect_before = false;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(form_button("Reset", false).on_click(
                        cx.listener(|this, _, window, cx| this.reset_effect_form(window, cx)),
                    )),
            )
            .child(
                div()
                    .h(px(16.))
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child(if updating { "Updating preview…" } else { "" }),
            )
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
            FormKind::Color { target, hex, .. } => {
                let Some(color) = hex.color() else {
                    form.error = Some("Enter six hexadecimal digits, such as 76B7FF.".into());
                    cx.notify();
                    return;
                };
                let target = *target;
                self.dismiss_form(window, cx);
                match target {
                    ColorTarget::Swatch(swatch) if swatch == super::text_ui::TEXT_SWATCH => {
                        self.recolor_text(color, window, cx)
                    }
                    ColorTarget::Swatch(swatch) => {
                        self.set_color(swatch, color);
                        self.refresh_gradient(window, cx);
                    }
                    ColorTarget::GradientStop { revision, index } => {
                        self.set_gradient_stop_rgb(revision, index, color, window, cx)
                    }
                }
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
                self.commit_layer_opacity(id, opacity, window, cx);
            }
            FormKind::PdfImport {
                path,
                source,
                dpi,
                from,
                to,
                all_pages,
                mode,
                background,
                canvas,
                target,
                ..
            } => {
                let info = &source.info;
                let Ok(dpi) = dpi.text.parse::<u32>() else {
                    form.error = Some("Enter a resolution in DPI.".into());
                    cx.notify();
                    return;
                };
                let (first, last) = if *all_pages {
                    (1, info.pages)
                } else {
                    let (Ok(first), Ok(last)) =
                        (from.text.parse::<usize>(), to.text.parse::<usize>())
                    else {
                        form.error = Some("Enter a page range.".into());
                        cx.notify();
                        return;
                    };
                    (first, last)
                };
                let mode = if matches!(target, PdfTarget::Layers { .. }) {
                    PdfMode::Layers
                } else {
                    *mode
                };
                let options = PdfOptions {
                    path: path.clone(),
                    first,
                    last,
                    dpi,
                    mode,
                    background: *background,
                    canvas: if *target == PdfTarget::NewDocument {
                        *canvas
                    } else {
                        PdfCanvas::FirstPage
                    },
                };
                let sizes = match pdf::validate_options(info, &options) {
                    Ok(sizes) => sizes,
                    Err(error) => {
                        form.error = Some(error.to_string());
                        cx.notify();
                        return;
                    }
                };
                let target = *target;
                let source = source.clone();
                if let Err(error) = self.validate_pdf_target(target, &sizes) {
                    if let Some(form) = self.form.as_mut() {
                        form.error = Some(error.to_string());
                    }
                    cx.notify();
                    return;
                }
                self.dismiss_form(window, cx);
                self.begin_pdf_import(source, options, target, window, cx);
            }
            FormKind::Adjustment { .. } => {
                self.apply_adjustment(window, cx);
                self.dismiss_form(window, cx);
            }
            FormKind::Filter { .. } => {
                if self.apply_filter(window, cx) {
                    self.dismiss_form(window, cx);
                }
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
                    FormKind::PdfImport {
                        dpi,
                        from,
                        to,
                        all_pages,
                        active,
                        ..
                    } => {
                        if key == "tab" {
                            if *all_pages {
                                *all_pages = false;
                                *active = 1;
                                from.selected = true;
                            } else {
                                *active = (*active + 1) % 3;
                                match *active {
                                    0 => dpi.selected = true,
                                    1 => from.selected = true,
                                    _ => to.selected = true,
                                }
                            }
                        } else {
                            match *active {
                                0 => dpi.handle_key(event, FieldMode::Digits { max: 4 }, cx),
                                1 => from.handle_key(event, FieldMode::Digits { max: 4 }, cx),
                                _ => to.handle_key(event, FieldMode::Digits { max: 4 }, cx),
                            }
                        }
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
                    FormKind::Adjustment { .. } | FormKind::Filter { .. } => {}
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

    fn drag_adjust_slider(
        &mut self,
        slider: AdjustSlider,
        range: (i16, i16),
        at: (Bounds<Pixels>, Point<Pixels>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = slider_amount(at.0, at.1, range.0, range.1);
        match slider {
            AdjustSlider::Balance(index) => self.drag_balance_slider(index, value, window, cx),
            AdjustSlider::InputBlack
            | AdjustSlider::InputWhite
            | AdjustSlider::Gamma
            | AdjustSlider::OutputBlack
            | AdjustSlider::OutputWhite => self.drag_level_slider(slider, value, window, cx),
            AdjustSlider::Brightness
            | AdjustSlider::Contrast
            | AdjustSlider::Hue
            | AdjustSlider::Saturation
            | AdjustSlider::Lightness => {
                self.drag_simple_slider(slider, value, window, cx);
            }
        }
    }

    fn drag_filter_slider(
        &mut self,
        slider: FilterSlider,
        range: (i16, i16),
        at: (Bounds<Pixels>, Point<Pixels>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = slider_amount(at.0, at.1, range.0, range.1);
        let spec = {
            let Some(Form {
                kind:
                    FormKind::Filter {
                        kind,
                        radius,
                        amount,
                        offset_x,
                        offset_y,
                        opacity,
                        color,
                    },
                ..
            }) = &mut self.form
            else {
                return;
            };
            let current = match slider {
                FilterSlider::Radius => *radius,
                FilterSlider::Amount => *amount,
                FilterSlider::OffsetX => *offset_x,
                FilterSlider::OffsetY => *offset_y,
                FilterSlider::Opacity => *opacity,
            };
            if current == value {
                return;
            }
            match slider {
                FilterSlider::Radius => *radius = value,
                FilterSlider::Amount => *amount = value,
                FilterSlider::OffsetX => *offset_x = value,
                FilterSlider::OffsetY => *offset_y = value,
                FilterSlider::Opacity => *opacity = value,
            }
            filter_spec(
                *kind, *radius, *amount, *offset_x, *offset_y, *opacity, *color,
            )
        };
        self.set_filter(spec, window, cx);
    }

    fn drag_simple_slider(
        &mut self,
        slider: AdjustSlider,
        value: i16,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Form {
            kind:
                FormKind::Adjustment {
                    operation,
                    brightness,
                    contrast,
                    hue,
                    saturation,
                    lightness,
                    ..
                },
            ..
        }) = &self.form
        else {
            return;
        };
        let operation = *operation;
        let mut values = (*brightness, *contrast, *hue, *saturation, *lightness);
        match slider {
            AdjustSlider::Brightness => values.0 = value,
            AdjustSlider::Contrast => values.1 = value,
            AdjustSlider::Hue => values.2 = value,
            AdjustSlider::Saturation => values.3 = value,
            AdjustSlider::Lightness => values.4 = value,
            _ => return,
        }
        let adjustment = match operation {
            AdjustKind::BrightnessContrast => Adjustment::BrightnessContrast {
                brightness: values.0,
                contrast: values.1,
            },
            AdjustKind::HueSaturation => Adjustment::HueSaturation {
                hue: values.2,
                saturation: values.3,
                lightness: values.4,
            },
            _ => return,
        };
        if let Some(Form {
            kind:
                FormKind::Adjustment {
                    brightness,
                    contrast,
                    hue,
                    saturation,
                    lightness,
                    ..
                },
            ..
        }) = &mut self.form
        {
            *brightness = values.0;
            *contrast = values.1;
            *hue = values.2;
            *saturation = values.3;
            *lightness = values.4;
        }
        self.set_adjustment(adjustment, window, cx);
        cx.notify();
    }

    fn drag_balance_slider(
        &mut self,
        index: usize,
        value: i16,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Form {
            kind:
                FormKind::Adjustment {
                    balance_range,
                    balance,
                    ..
                },
            ..
        }) = &self.form
        else {
            return;
        };
        let balance = balance.set_shift(*balance_range, index, value);
        if let Some(Form {
            kind: FormKind::Adjustment {
                balance: stored, ..
            },
            ..
        }) = &mut self.form
        {
            *stored = balance;
        }
        self.set_adjustment(Adjustment::ColorBalance(balance), window, cx);
        cx.notify();
    }

    fn drag_level_slider(
        &mut self,
        slider: AdjustSlider,
        value: i16,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Form {
            kind:
                FormKind::Adjustment {
                    map_channel,
                    levels,
                    ..
                },
            ..
        }) = &self.form
        else {
            return;
        };
        let channel = *map_channel;
        let mut stops = levels.stops(channel);
        match slider {
            AdjustSlider::InputBlack => stops = stops.set_input_black(value.clamp(0, 255) as u8),
            AdjustSlider::InputWhite => stops = stops.set_input_white(value.clamp(0, 255) as u8),
            AdjustSlider::Gamma => {
                stops = stops.set_gamma(u16::try_from(value).unwrap_or(GAMMA_ONE));
            }
            AdjustSlider::OutputBlack => stops = stops.set_output_black(value.clamp(0, 255) as u8),
            AdjustSlider::OutputWhite => stops = stops.set_output_white(value.clamp(0, 255) as u8),
            _ => return,
        }
        let levels = levels.set_stops(channel, stops);
        if let Some(Form {
            kind: FormKind::Adjustment { levels: stored, .. },
            ..
        }) = &mut self.form
        {
            *stored = levels;
        }
        self.set_adjustment(Adjustment::Levels(levels), window, cx);
        cx.notify();
    }

    fn set_balance_range(&mut self, range: BalanceRange, cx: &mut Context<Self>) {
        if let Some(Form {
            kind: FormKind::Adjustment { balance_range, .. },
            ..
        }) = &mut self.form
        {
            *balance_range = range;
        }
        cx.notify();
    }

    fn toggle_luminosity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Form {
            kind: FormKind::Adjustment { balance, .. },
            ..
        }) = &self.form
        else {
            return;
        };
        let balance = balance.set_preserve_luminosity(!balance.preserve_luminosity);
        if let Some(Form {
            kind: FormKind::Adjustment {
                balance: stored, ..
            },
            ..
        }) = &mut self.form
        {
            *stored = balance;
        }
        self.set_adjustment(Adjustment::ColorBalance(balance), window, cx);
        cx.notify();
    }

    fn set_map_channel(&mut self, channel: MapChannel, cx: &mut Context<Self>) {
        if let Some(Form {
            kind:
                FormKind::Adjustment {
                    map_channel,
                    curve_drag,
                    ..
                },
            ..
        }) = &mut self.form
        {
            *map_channel = channel;
            *curve_drag = None;
        }
        cx.notify();
    }

    fn edit_curve(
        &mut self,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        button: MouseButton,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Form {
            kind:
                FormKind::Adjustment {
                    map_channel,
                    curves,
                    ..
                },
            ..
        }) = &self.form
        else {
            return;
        };
        let channel = *map_channel;
        let curve = curves.curve(channel);
        let (x, y) = curve_coords(bounds, position);
        if let Some(index) = hit_curve_point(bounds, position, curve) {
            if button == MouseButton::Right {
                self.store_curve(channel, curve.remove_point(index), None, window, cx);
            } else {
                let at = curve.points()[index];
                let grab = (
                    i16::from(at.0) - i16::from(x),
                    i16::from(at.1) - i16::from(y),
                );
                self.store_curve(channel, curve, Some((index as u8, grab)), window, cx);
            }
            return;
        }
        if button != MouseButton::Left {
            return;
        }
        let Some(curve) = curve.try_add(x, y) else {
            return;
        };
        let index = curve
            .points()
            .iter()
            .position(|point| point.0 == x)
            .unwrap_or(0);
        self.store_curve(channel, curve, Some((index as u8, (0, 0))), window, cx);
    }

    fn drag_curve_point(
        &mut self,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Form {
            kind:
                FormKind::Adjustment {
                    map_channel,
                    curves,
                    curve_drag: Some((index, grab)),
                    ..
                },
            ..
        }) = &self.form
        else {
            return;
        };
        let channel = *map_channel;
        let index = usize::from(*index);
        let grab = *grab;
        let (x, y) = curve_coords(bounds, position);
        let x = (i16::from(x) + grab.0).clamp(0, 255) as u8;
        let y = (i16::from(y) + grab.1).clamp(0, 255) as u8;
        let curve = curves.curve(channel).move_point(index, x, y);
        self.store_curve(channel, curve, Some((index as u8, grab)), window, cx);
    }

    fn end_curve_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(Form {
            kind: FormKind::Adjustment { curve_drag, .. },
            ..
        }) = &mut self.form
        {
            *curve_drag = None;
        }
        cx.notify();
    }

    fn store_curve(
        &mut self,
        channel: MapChannel,
        curve: Curve,
        drag: Option<(u8, (i16, i16))>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Form {
            kind: FormKind::Adjustment { curves, .. },
            ..
        }) = &self.form
        else {
            return;
        };
        let curves = curves.set_curve(channel, curve);
        if let Some(Form {
            kind:
                FormKind::Adjustment {
                    curves: stored,
                    curve_drag,
                    ..
                },
            ..
        }) = &mut self.form
        {
            *stored = curves;
            *curve_drag = drag;
        }
        self.set_adjustment(Adjustment::Curves(curves), window, cx);
        cx.notify();
    }

    pub(super) fn form_overlay(&self, cx: &Context<Self>) -> Option<Div> {
        let adjustment_preview = self.adjustment_preview.clone();
        let adjustment_histogram = self.adjustment_histogram;
        let filter_preview = self.filter_preview.clone();
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
                FormKind::PdfImport {
                    path,
                    source,
                    dpi,
                    from,
                    to,
                    all_pages,
                    mode,
                    background,
                    canvas,
                    target,
                    active,
                } => {
                    let info = &source.info;
                    let pages = if info.pages == 1 {
                        "1 page".to_owned()
                    } else {
                        format!("{} pages", info.pages)
                    };
                    let preview = (|| -> Result<_, String> {
                        let dpi = dpi.text.parse::<u32>().map_err(|_| "Enter a resolution in DPI.".to_owned())?;
                        let (first, last) = if *all_pages { (1, info.pages) }
                            else {
                                let first = from.text.parse::<usize>().map_err(|_| "Enter a page range.".to_owned())?;
                                let last = to.text.parse::<usize>().map_err(|_| "Enter a page range.".to_owned())?;
                                (first, last)
                            };
                        let options = PdfOptions {
                            path: path.clone(), first, last, dpi,
                            mode: if *target == PdfTarget::NewDocument { *mode } else { PdfMode::Layers },
                            background: *background,
                            canvas: if *target == PdfTarget::NewDocument { *canvas } else { PdfCanvas::FirstPage },
                        };
                        let sizes = pdf::validate_options(info, &options).map_err(|error| error.to_string())?;
                        Ok((options, sizes))
                    })();
                    let target_canvas = match target {
                        PdfTarget::NewDocument => preview.as_ref().ok().and_then(|(options, sizes)| {
                            (options.mode == PdfMode::Layers).then(|| pdf::layered_canvas_size(sizes, options.canvas).ok()).flatten()
                        }),
                        PdfTarget::Layers { tab_id, .. } => self.active_tab().filter(|tab| tab.id == *tab_id).map(|tab| tab.document.content.size()),
                    };
                    let estimate = preview.as_ref().map_or_else(
                        |error| error.clone(),
                        |(options, sizes)| {
                            if *target == PdfTarget::NewDocument && let Some((width, height)) = target_canvas {
                                format!("Canvas: {width} × {height} px at {} DPI", options.dpi)
                            } else {
                                format!("Page {} at {} DPI: {} × {} px", options.first, options.dpi, sizes[0].0, sizes[0].1)
                            }
                        },
                    );
                    let clipping = preview.as_ref().ok().and_then(|(_, sizes)| {
                        let (width, height) = target_canvas?;
                        let count = sizes.iter().filter(|size| size.0 > width || size.1 > height).count();
                        match count {
                            0 => None,
                            1 => Some("1 selected page extends outside the canvas. Its pixels are retained in its layer.".to_owned()),
                            _ => Some(format!("{count} selected pages extend outside the canvas. Their pixels are retained in the layers.")),
                        }
                    });
                    let range = div()
                        .flex()
                        .gap_2()
                        .child(form_button("All Pages", *all_pages).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(Form {
                                    kind:
                                        FormKind::PdfImport {
                                            all_pages, active, ..
                                        },
                                    ..
                                }) = &mut this.form
                                {
                                    *all_pages = true;
                                    *active = 0;
                                    cx.notify();
                                }
                            },
                        )))
                        .child(form_button("Page Range", !*all_pages).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(Form {
                                    kind:
                                        FormKind::PdfImport {
                                            all_pages,
                                            from,
                                            active,
                                            ..
                                        },
                                    ..
                                }) = &mut this.form
                                {
                                    *all_pages = false;
                                    *active = 1;
                                    from.selected = true;
                                    cx.notify();
                                }
                            },
                        )));
                    let fields = if *all_pages {
                        div()
                    } else {
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                div().flex_1().child("From").child(
                                    from.view(*active == 1).id("pdf-from").mt_1().on_click(
                                        cx.listener(|this, _, _, cx| {
                                            if let Some(Form {
                                                kind:
                                                    FormKind::PdfImport { from, active, .. },
                                                ..
                                            }) = &mut this.form
                                            {
                                                *active = 1;
                                                from.selected = true;
                                                cx.notify();
                                            }
                                        }),
                                    ),
                                ),
                            )
                            .child(
                                div().flex_1().child("To").child(
                                    to.view(*active == 2).id("pdf-to").mt_1().on_click(
                                        cx.listener(|this, _, _, cx| {
                                            if let Some(Form {
                                                kind: FormKind::PdfImport { to, active, .. },
                                                ..
                                            }) = &mut this.form
                                            {
                                                *active = 2;
                                                to.selected = true;
                                                cx.notify();
                                            }
                                        }),
                                    ),
                                ),
                            )
                    };
                    let modes = div().flex().gap_2().child(
                        form_button("As Layers", *mode == PdfMode::Layers).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(Form {
                                    kind: FormKind::PdfImport { mode, .. },
                                    ..
                                }) = &mut this.form
                                {
                                    *mode = PdfMode::Layers;
                                    cx.notify();
                                }
                            },
                        )),
                    );
                    let modes = if *target == PdfTarget::NewDocument {
                        modes.child(
                            form_button("As Images", *mode == PdfMode::Separate).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(Form {
                                        kind: FormKind::PdfImport { mode, .. },
                                        ..
                                    }) = &mut this.form
                                    {
                                        *mode = PdfMode::Separate;
                                        cx.notify();
                                    }
                                }),
                            ),
                        )
                    } else {
                        modes
                    };
                    let backgrounds = div().flex().gap_2().children([
                        ("White", PdfBackground::White), ("Transparent", PdfBackground::Transparent),
                    ].map(|(label, choice)| form_button(label, *background == choice).on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(Form { kind: FormKind::PdfImport { background, .. }, .. }) = &mut this.form {
                            *background = choice; cx.notify();
                        }
                    }))));
                    let canvases = div().flex().gap_2().children([
                        ("First Page", PdfCanvas::FirstPage), ("Fit All Pages", PdfCanvas::FitAllPages),
                    ].map(|(label, choice)| form_button(label, *canvas == choice).on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(Form { kind: FormKind::PdfImport { canvas, .. }, .. }) = &mut this.form {
                            *canvas = choice; cx.notify();
                        }
                    }))));
                    let body = div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(format!("{}  ·  {pages}", file_name(path))),
                        )
                        .child(div().child("Resolution (DPI)").child(
                            dpi.view(*active == 0).id("pdf-dpi").mt_1().on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(Form {
                                        kind: FormKind::PdfImport { dpi, active, .. },
                                        ..
                                    }) = &mut this.form
                                    {
                                        *active = 0;
                                        dpi.selected = true;
                                        cx.notify();
                                    }
                                },
                            )),
                        ))
                        .child(
                            div()
                                .child("Pages")
                                .child(range.mt_1())
                                .child(fields.mt_2()),
                        )
                        .when(*target == PdfTarget::NewDocument, |body| {
                            body.child(div().child("Open pages as").child(modes.mt_1()))
                        })
                        .child(div().child("Page background").child(backgrounds.mt_1()))
                        .when(*target == PdfTarget::NewDocument && *mode == PdfMode::Layers && info.pages > 1, |body| {
                            body.child(div().child("Canvas size").child(canvases.mt_1()))
                        })
                        .child(div().text_xs().text_color(rgb(MUTED)).child(estimate))
                        .when_some(clipping, |body, warning| body.child(div().text_xs().text_color(rgb(0xffd699)).child(warning)))
                        .when(matches!(target, PdfTarget::Layers { .. }), |body| {
                            body.child(div().text_xs().text_color(rgb(MUTED)).child("Pages are centered on the existing canvas; its size stays unchanged."))
                        });
                    ("Import PDF", "Import", body)
                }
                FormKind::Color {
                    target, hex, hsv, ..
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
                    let title = match *target {
                        ColorTarget::Swatch(0) => "Foreground Color",
                        ColorTarget::Swatch(super::text_ui::TEXT_SWATCH) => "Text Color",
                        ColorTarget::Swatch(_) => "Background Color",
                        ColorTarget::GradientStop { .. } => "Gradient Stop Color",
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
                FormKind::Adjustment {
                    operation,
                    brightness,
                    contrast,
                    hue,
                    saturation,
                    lightness,
                    balance_range,
                    balance,
                    map_channel,
                    levels,
                    curves,
                    ..
                } => {
                    let title = match operation {
                        AdjustKind::BrightnessContrast => "Brightness & Contrast",
                        AdjustKind::HueSaturation => "Hue & Saturation",
                        AdjustKind::Desaturate => "Desaturate",
                        AdjustKind::Invert => "Invert",
                        AdjustKind::ColorBalance => "Color Balance",
                        AdjustKind::Levels => "Levels",
                        AdjustKind::Curves => "Curves",
                    };
                    let mut body = div().flex().flex_col().gap_3()
                        .child(self.effect_preview_panel(adjustment_preview, self.adjustment_generation.pending(), cx));
                    body = match operation {
                        AdjustKind::BrightnessContrast => body
                            .child(adjust_slider(
                                "Brightness",
                                brightness.to_string(),
                                *brightness,
                                -100,
                                100,
                                AdjustSlider::Brightness,
                                cx,
                            ))
                            .child(adjust_slider(
                                "Contrast",
                                contrast.to_string(),
                                *contrast,
                                -100,
                                100,
                                AdjustSlider::Contrast,
                                cx,
                            )),
                        AdjustKind::HueSaturation => body
                            .child(adjust_slider(
                                "Hue",
                                hue.to_string(),
                                *hue,
                                -180,
                                180,
                                AdjustSlider::Hue,
                                cx,
                            ))
                            .child(adjust_slider(
                                "Saturation",
                                saturation.to_string(),
                                *saturation,
                                -100,
                                100,
                                AdjustSlider::Saturation,
                                cx,
                            ))
                            .child(adjust_slider(
                                "Lightness",
                                lightness.to_string(),
                                *lightness,
                                -100,
                                100,
                                AdjustSlider::Lightness,
                                cx,
                            )),
                        AdjustKind::Desaturate => {
                            body.child("Removes color from the active layer and keeps its alpha.")
                        }
                        AdjustKind::Invert => {
                            body.child("Inverts the active layer's colors and keeps its alpha.")
                        }
                        AdjustKind::ColorBalance => {
                            color_balance_controls(body, *balance, *balance_range, cx)
                        }
                        AdjustKind::Levels => {
                            levels_controls(body, *levels, *map_channel, adjustment_histogram, cx)
                        }
                        AdjustKind::Curves => {
                            curves_controls(body, *curves, *map_channel, adjustment_histogram, cx)
                        }
                    };
                    (title, "Apply", body)
                }
                FormKind::Filter {
                    kind,
                    radius,
                    amount,
                    offset_x,
                    offset_y,
                    opacity,
                    color,
                } => {
                    let title = match kind {
                        FilterKind::Blur => "Blur",
                        FilterKind::Sharpen => "Sharpen",
                        FilterKind::Shadow => "Drop Shadow",
                    };
                    let body = div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(self.effect_preview_panel(filter_preview, self.filter_generation.pending(), cx))
                        .child(filter_controls(
                            *kind, *radius, *amount, *offset_x, *offset_y, *opacity, cx,
                        ))
                        .when(*kind == FilterKind::Shadow, |body| body.child(
                            div().flex().items_center().gap_2()
                                .child(div().size(px(16.)).border_1().border_color(rgb(BORDER)).rounded_sm().bg(rgba(u32::from_be_bytes(color.0))))
                                .child(div().flex_1().child("Shadow color"))
                                .child(form_button("Use Foreground", false).on_click(cx.listener(|this, _, window, cx| this.use_shadow_foreground(window, cx))))
                        ));
                    (title, "Apply", body)
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

#[derive(Clone, Copy)]
enum AdjustSlider {
    Brightness,
    Contrast,
    Hue,
    Saturation,
    Lightness,
    Balance(usize),
    InputBlack,
    InputWhite,
    Gamma,
    OutputBlack,
    OutputWhite,
}

fn adjust_slider(
    label: &'static str,
    readout: String,
    value: i16,
    min: i16,
    max: i16,
    slider: AdjustSlider,
    cx: &Context<Workspace>,
) -> Div {
    let fraction = f32::from(value - min) / f32::from(max - min).max(1.);
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let (layout, down, moved) = (bounds.clone(), bounds.clone(), bounds);
    let (down_slider, move_slider) = (slider, slider);
    let (down_min, down_max, move_min, move_max) = (min, max, min, max);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().flex().justify_between().child(label).child(readout))
        .child(
            div()
                .id(label)
                .h(px(22.))
                .w_full()
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.drag_adjust_slider(
                            down_slider,
                            (down_min, down_max),
                            (down.get(), event.position),
                            window,
                            cx,
                        );
                    }),
                )
                .on_mouse_move(
                    cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                        if event.dragging() {
                            this.drag_adjust_slider(
                                move_slider,
                                (move_min, move_max),
                                (moved.get(), event.position),
                                window,
                                cx,
                            );
                        }
                    }),
                )
                .child(
                    canvas(
                        move |bounds, _, _| layout.set(bounds),
                        move |bounds, _, window, _| paint_adjust_slider(bounds, fraction, window),
                    )
                    .size_full(),
                ),
        )
}

fn paint_adjust_slider(bounds: Bounds<Pixels>, fraction: f32, window: &mut Window) {
    let track = Bounds::centered_at(bounds.center(), size(bounds.size.width, px(6.)));
    window.paint_quad(fill(track, rgb(CHROME)));
    let knob = Bounds::centered_at(
        point(
            bounds.left() + bounds.size.width * fraction.clamp(0., 1.),
            bounds.center().y,
        ),
        size(px(12.), px(12.)),
    );
    window.paint_quad(fill(knob, rgb(ACCENT)));
}

#[derive(Clone, Copy)]
enum FilterSlider {
    Radius,
    Amount,
    OffsetX,
    OffsetY,
    Opacity,
}

fn filter_spec(
    kind: FilterKind,
    radius: i16,
    amount: i16,
    offset_x: i16,
    offset_y: i16,
    opacity: i16,
    color: image::Rgba<u8>,
) -> FilterSpec {
    let radius = u32::from(u16::try_from(radius).unwrap_or(0));
    let amount = u8::try_from(amount).unwrap_or(0);
    let opacity = u8::try_from(opacity).unwrap_or(0);
    match kind {
        FilterKind::Blur => FilterSpec::Blur { radius },
        FilterKind::Sharpen => FilterSpec::Sharpen { radius, amount },
        FilterKind::Shadow => FilterSpec::Shadow {
            radius,
            offset: (i32::from(offset_x), i32::from(offset_y)),
            opacity,
            color,
        },
    }
}

fn filter_controls(
    kind: FilterKind,
    radius: i16,
    amount: i16,
    offset_x: i16,
    offset_y: i16,
    opacity: i16,
    cx: &Context<Workspace>,
) -> Div {
    let limit = i16::try_from(filters::MAX_RADIUS).unwrap_or(32);
    let body = div().flex().flex_col().gap_3().child(filter_slider(
        "Radius",
        radius.to_string(),
        radius,
        0,
        limit,
        FilterSlider::Radius,
        cx,
    ));
    match kind {
        FilterKind::Blur => body.child("Softens the active layer inside the selection."),
        FilterKind::Sharpen => body
            .child(filter_slider(
                "Amount",
                format!("{amount}%"),
                amount,
                0,
                100,
                FilterSlider::Amount,
                cx,
            ))
            .child("Sharpens the active layer inside the selection."),
        FilterKind::Shadow => body
            .child(filter_slider(
                "Offset X",
                offset_x.to_string(),
                offset_x,
                -64,
                64,
                FilterSlider::OffsetX,
                cx,
            ))
            .child(filter_slider(
                "Offset Y",
                offset_y.to_string(),
                offset_y,
                -64,
                64,
                FilterSlider::OffsetY,
                cx,
            ))
            .child(filter_slider(
                "Opacity",
                format!("{opacity}%"),
                opacity,
                0,
                100,
                FilterSlider::Opacity,
                cx,
            ))
            .child("Adds a layer under the active layer, using the shadow color shown below."),
    }
}

fn filter_slider(
    label: &'static str,
    readout: String,
    value: i16,
    min: i16,
    max: i16,
    slider: FilterSlider,
    cx: &Context<Workspace>,
) -> Div {
    let fraction = f32::from(value - min) / f32::from(max - min).max(1.);
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let (layout, down, moved) = (bounds.clone(), bounds.clone(), bounds);
    let (down_slider, move_slider) = (slider, slider);
    let (down_min, down_max, move_min, move_max) = (min, max, min, max);
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().flex().justify_between().child(label).child(readout))
        .child(
            div()
                .id(label)
                .h(px(22.))
                .w_full()
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.drag_filter_slider(
                            down_slider,
                            (down_min, down_max),
                            (down.get(), event.position),
                            window,
                            cx,
                        );
                    }),
                )
                .on_mouse_move(
                    cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                        if event.dragging() {
                            this.drag_filter_slider(
                                move_slider,
                                (move_min, move_max),
                                (moved.get(), event.position),
                                window,
                                cx,
                            );
                        }
                    }),
                )
                .child(
                    canvas(
                        move |bounds, _, _| layout.set(bounds),
                        move |bounds, _, window, _| paint_adjust_slider(bounds, fraction, window),
                    )
                    .size_full(),
                ),
        )
}

fn slider_amount(bounds: Bounds<Pixels>, position: Point<Pixels>, min: i16, max: i16) -> i16 {
    let width = f32::from(bounds.size.width).max(1.);
    let t = ((f32::from(position.x) - f32::from(bounds.left())) / width).clamp(0., 1.);
    let value = f32::from(min) + t * f32::from(max - min);
    value.round().clamp(f32::from(min), f32::from(max)) as i16
}

fn color_balance_controls(
    body: Div,
    balance: ColorBalance,
    range: BalanceRange,
    cx: &Context<Workspace>,
) -> Div {
    let shift = balance.shift(range);
    body.child(
        div()
            .flex()
            .gap_2()
            .child(balance_range_button(
                "Shadows",
                BalanceRange::Shadows,
                range,
                cx,
            ))
            .child(balance_range_button(
                "Midtones",
                BalanceRange::Midtones,
                range,
                cx,
            ))
            .child(balance_range_button(
                "Highlights",
                BalanceRange::Highlights,
                range,
                cx,
            )),
    )
    .child(adjust_slider(
        "Cyan–Red",
        shift[0].to_string(),
        shift[0],
        -100,
        100,
        AdjustSlider::Balance(0),
        cx,
    ))
    .child(adjust_slider(
        "Magenta–Green",
        shift[1].to_string(),
        shift[1],
        -100,
        100,
        AdjustSlider::Balance(1),
        cx,
    ))
    .child(adjust_slider(
        "Yellow–Blue",
        shift[2].to_string(),
        shift[2],
        -100,
        100,
        AdjustSlider::Balance(2),
        cx,
    ))
    .child(
        form_button("Preserve luminosity", balance.preserve_luminosity)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_luminosity(window, cx))),
    )
}

fn balance_range_button(
    label: &'static str,
    range: BalanceRange,
    current: BalanceRange,
    cx: &Context<Workspace>,
) -> impl IntoElement {
    form_button(label, current == range).on_click(cx.listener(move |this, _, _, cx| {
        this.set_balance_range(range, cx);
    }))
}

fn levels_controls(
    body: Div,
    levels: Levels,
    channel: MapChannel,
    histogram: Option<Histogram>,
    cx: &Context<Workspace>,
) -> Div {
    let stops = levels.stops(channel);
    let gamma = format!("{:.2}", f32::from(stops.gamma) / f32::from(GAMMA_ONE));
    body.child(histogram_caption(histogram))
        .child(channel_row(channel, cx))
        .child(histogram_plot(histogram, channel))
        .child(adjust_slider(
            "Input black",
            stops.input_black.to_string(),
            i16::from(stops.input_black),
            0,
            i16::from(stops.input_white.saturating_sub(1)),
            AdjustSlider::InputBlack,
            cx,
        ))
        .child(adjust_slider(
            "Gamma",
            gamma,
            i16::try_from(stops.gamma).unwrap_or(GAMMA_ONE as i16),
            GAMMA_MIN as i16,
            GAMMA_MAX as i16,
            AdjustSlider::Gamma,
            cx,
        ))
        .child(adjust_slider(
            "Input white",
            stops.input_white.to_string(),
            i16::from(stops.input_white),
            i16::from(stops.input_black.saturating_add(1)),
            255,
            AdjustSlider::InputWhite,
            cx,
        ))
        .child(adjust_slider(
            "Output black",
            stops.output_black.to_string(),
            i16::from(stops.output_black),
            0,
            255,
            AdjustSlider::OutputBlack,
            cx,
        ))
        .child(adjust_slider(
            "Output white",
            stops.output_white.to_string(),
            i16::from(stops.output_white),
            0,
            255,
            AdjustSlider::OutputWhite,
            cx,
        ))
}

fn curves_controls(
    body: Div,
    curves: Curves,
    channel: MapChannel,
    histogram: Option<Histogram>,
    cx: &Context<Workspace>,
) -> Div {
    body.child(histogram_caption(histogram))
        .child(channel_row(channel, cx))
        .child(curve_plot(curves.curve(channel), histogram, channel, cx))
}

fn channel_row(channel: MapChannel, cx: &Context<Workspace>) -> Div {
    div()
        .flex()
        .gap_2()
        .child(map_channel_button("Value", MapChannel::Value, channel, cx))
        .child(map_channel_button("Red", MapChannel::Red, channel, cx))
        .child(map_channel_button("Green", MapChannel::Green, channel, cx))
        .child(map_channel_button("Blue", MapChannel::Blue, channel, cx))
}

fn map_channel_button(
    label: &'static str,
    channel: MapChannel,
    current: MapChannel,
    cx: &Context<Workspace>,
) -> impl IntoElement {
    form_button(label, current == channel).on_click(cx.listener(move |this, _, _, cx| {
        this.set_map_channel(channel, cx);
    }))
}

fn histogram_caption(histogram: Option<Histogram>) -> &'static str {
    match histogram {
        None => "Histogram…",
        Some(histogram) if histogram.is_empty() => "No opaque pixels in the selection.",
        Some(_) => "Histogram",
    }
}

fn histogram_plot(histogram: Option<Histogram>, channel: MapChannel) -> Div {
    let bins = histogram.map(|histogram| histogram.bins(channel));
    div().h(px(120.)).w_full().rounded_md().child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| match &bins {
                Some(bins) => paint_histogram(bounds, bins, window),
                None => window.paint_quad(fill(bounds, rgb(CANVAS))),
            },
        )
        .size_full(),
    )
}

fn curve_plot(
    curve: Curve,
    histogram: Option<Histogram>,
    channel: MapChannel,
    cx: &Context<Workspace>,
) -> impl IntoElement {
    let bins = histogram.map(|histogram| histogram.bins(channel));
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let (layout, down, right, moved, released, released_out) = (
        bounds.clone(),
        bounds.clone(),
        bounds.clone(),
        bounds.clone(),
        bounds.clone(),
        bounds,
    );
    div()
        .id("curve")
        .h(px(180.))
        .w_full()
        .rounded_md()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.edit_curve(down.get(), event.position, MouseButton::Left, window, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.edit_curve(right.get(), event.position, MouseButton::Right, window, cx);
            }),
        )
        .on_mouse_move(
            cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    this.drag_curve_point(moved.get(), event.position, window, cx);
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                this.drag_curve_point(released.get(), event.position, window, cx);
                this.end_curve_drag(cx);
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                this.drag_curve_point(released_out.get(), event.position, window, cx);
                this.end_curve_drag(cx);
            }),
        )
        .child(
            canvas(
                move |bounds, _, _| layout.set(bounds),
                move |bounds, _, window, _| paint_curve(bounds, curve, bins, window),
            )
            .size_full(),
        )
}

fn paint_histogram(bounds: Bounds<Pixels>, bins: &[u32; 256], window: &mut Window) {
    window.paint_quad(fill(bounds, rgb(CANVAS)));
    let max = bins.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return;
    }
    let step = f32::from(bounds.size.width) / 256.;
    let height = f32::from(bounds.size.height);
    for (index, count) in bins.iter().copied().enumerate() {
        if count == 0 {
            continue;
        }
        let bar = height * (count as f32 / max as f32);
        let left = f32::from(bounds.left()) + step * index as f32;
        window.paint_quad(fill(
            Bounds::from_corners(
                point(px(left), bounds.bottom() - px(bar)),
                point(px(left + step.max(1.)), bounds.bottom()),
            ),
            rgba(0xacc6ff88),
        ));
    }
}

fn paint_curve(
    bounds: Bounds<Pixels>,
    curve: Curve,
    bins: Option<[u32; 256]>,
    window: &mut Window,
) {
    match &bins {
        Some(bins) => paint_histogram(bounds, bins, window),
        None => window.paint_quad(fill(bounds, rgb(CANVAS))),
    }
    let mut path = PathBuilder::stroke(px(1.5));
    let mut started = false;
    for point in curve.points() {
        let at = curve_point_px(bounds, *point);
        if started {
            path.line_to(at);
        } else {
            path.move_to(at);
            started = true;
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, rgb(0xffffff));
    }
    for point in curve.points() {
        window.paint_quad(fill(
            Bounds::centered_at(curve_point_px(bounds, *point), size(px(8.), px(8.))),
            rgb(ACCENT),
        ));
    }
}

fn curve_point_px(bounds: Bounds<Pixels>, point_xy: (u8, u8)) -> Point<Pixels> {
    point(
        bounds.left() + bounds.size.width * (f32::from(point_xy.0) / 255.),
        bounds.top() + bounds.size.height * (1. - f32::from(point_xy.1) / 255.),
    )
}

fn curve_coords(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (u8, u8) {
    let width = f32::from(bounds.size.width).max(1.);
    let height = f32::from(bounds.size.height).max(1.);
    let x = ((f32::from(position.x) - f32::from(bounds.left())) / width).clamp(0., 1.);
    let y = ((f32::from(position.y) - f32::from(bounds.top())) / height).clamp(0., 1.);
    (
        (x * 255.).round().clamp(0., 255.) as u8,
        ((1. - y) * 255.).round().clamp(0., 255.) as u8,
    )
}

fn hit_curve_point(bounds: Bounds<Pixels>, position: Point<Pixels>, curve: Curve) -> Option<usize> {
    curve.points().iter().position(|point| {
        let center = curve_point_px(bounds, *point);
        let dx = f32::from(position.x) - f32::from(center.x);
        let dy = f32::from(position.y) - f32::from(center.y);
        dx * dx + dy * dy <= 64.
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_adjustment_controls_round_trip_all_channels_and_reset_to_defaults() {
        let levels = Levels::default()
            .set_stops(
                MapChannel::Value,
                crate::adjustments::LevelStops::IDENTITY.set_input_black(12),
            )
            .set_stops(
                MapChannel::Red,
                crate::adjustments::LevelStops::IDENTITY.set_gamma(150),
            )
            .set_stops(
                MapChannel::Green,
                crate::adjustments::LevelStops::IDENTITY.set_output_white(210),
            )
            .set_stops(
                MapChannel::Blue,
                crate::adjustments::LevelStops::IDENTITY.set_input_white(230),
            );
        let curves = Curves::default()
            .set_curve(
                MapChannel::Red,
                Curve::identity().try_add(100, 160).unwrap(),
            )
            .set_curve(
                MapChannel::Blue,
                Curve::identity().try_add(150, 120).unwrap(),
            );
        for value in [
            Adjustment::BrightnessContrast {
                brightness: -30,
                contrast: 24,
            },
            Adjustment::HueSaturation {
                hue: 175,
                saturation: -41,
                lightness: 23,
            },
            Adjustment::Desaturate,
            Adjustment::Invert,
            Adjustment::ColorBalance(ColorBalance {
                shadows: [-10, 20, 30],
                midtones: [40, -50, 60],
                highlights: [-70, 80, 90],
                preserve_luminosity: false,
            }),
            Adjustment::Levels(levels),
            Adjustment::Curves(curves),
        ] {
            let form = FormKind::adjustment_parameters(value);
            assert_eq!(form.adjustment_value(), Some(value));
            let kind = AdjustKind::of(value);
            assert_eq!(
                FormKind::adjustment_defaults(kind).adjustment_value(),
                Some(kind.defaults())
            );
        }
    }

    #[test]
    fn remembered_filter_controls_keep_shadow_color_and_signed_offsets() {
        for value in [
            FilterSpec::Blur { radius: 32 },
            FilterSpec::Sharpen {
                radius: 3,
                amount: 89,
            },
            FilterSpec::Shadow {
                radius: 15,
                offset: (-64, 64),
                opacity: 38,
                color: image::Rgba([20, 40, 60, 80]),
            },
        ] {
            assert_eq!(
                FormKind::filter_parameters(value).filter_value(),
                Some(value)
            );
        }
        let foreground = image::Rgba([12, 34, 56, 255]);
        assert_eq!(
            FormKind::filter_defaults(FilterKind::Shadow, foreground).filter_value(),
            Some(FilterSpec::Shadow {
                radius: 4,
                offset: (8, 8),
                opacity: 75,
                color: foreground,
            })
        );
    }

    #[test]
    fn font_query_fields_keep_unicode_cursor_boundaries_and_the_full_font_name_limit() {
        let mut field = Field::new("café".into());
        field.selected = false;
        field.replace(" 上");
        assert_eq!(field.text(), "café 上");
        assert_eq!(field.cursor, field.text.chars().count());
        field.cursor = 4;
        field.replace("!");
        assert_eq!(field.text(), "café! 上");
        field.select_all();
        field.insert(
            &"x".repeat(crate::text::MAX_FAMILY_CHARS),
            FieldMode::FontQuery,
        );
        assert_eq!(field.len(), crate::text::MAX_FAMILY_CHARS);
        field.insert("x", FieldMode::FontQuery);
        assert_eq!(field.len(), crate::text::MAX_FAMILY_CHARS);
        field.select_all();
        field.insert("invalid\nquery", FieldMode::FontQuery);
        assert_eq!(field.len(), crate::text::MAX_FAMILY_CHARS);
    }
}
