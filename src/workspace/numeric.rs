use gpui::{
    ClipboardItem, Context, CursorStyle, Div, Keystroke, MouseButton, Window, div, prelude::*, px,
    rgb,
};

use super::Workspace;
use super::view::{ACCENT, BORDER, CANVAS, MUTED};
use crate::document::CropRect;
use crate::preview::PixelRect;
use crate::selection::Selection;
use crate::text::{MAX_LINE_SPACING, MAX_PIXEL_SIZE, MIN_LINE_SPACING, MIN_PIXEL_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NumericField {
    SelectionRadius,
    SelectionWidth,
    SelectionHeight,
    CropX,
    CropY,
    CropWidth,
    CropHeight,
    TextSize,
    TextLineSpacing,
}

impl NumericField {
    fn id(self) -> usize {
        match self {
            Self::SelectionRadius => 0,
            Self::SelectionWidth => 1,
            Self::SelectionHeight => 2,
            Self::CropX => 3,
            Self::CropY => 4,
            Self::CropWidth => 5,
            Self::CropHeight => 6,
            Self::TextSize => 7,
            Self::TextLineSpacing => 8,
        }
    }

    fn is_crop(self) -> bool {
        matches!(
            self,
            Self::CropX | Self::CropY | Self::CropWidth | Self::CropHeight
        )
    }

    pub(super) fn is_selection(self) -> bool {
        matches!(
            self,
            Self::SelectionRadius | Self::SelectionWidth | Self::SelectionHeight
        )
    }
}

pub(super) struct NumericEdit {
    pub(super) field: NumericField,
    text: String,
    cursor: usize,
    selected: bool,
    original: u32,
    original_crop: Option<CropRect>,
}

impl NumericEdit {
    fn new(field: NumericField, value: u32, crop: Option<CropRect>) -> Self {
        let text = value.to_string();
        Self {
            field,
            cursor: text.chars().count(),
            text,
            selected: true,
            original: value,
            original_crop: crop,
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

    fn insert_digits(&mut self, text: &str) {
        let digits: String = text.chars().filter(|ch| ch.is_ascii_digit()).collect();
        if digits.is_empty() {
            return;
        }
        let existing = if self.selected { 0 } else { self.len() };
        if existing + digits.chars().count() > 10 {
            return;
        }
        if self.selected {
            self.text.clear();
            self.cursor = 0;
            self.selected = false;
        }
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, &digits);
        self.cursor += digits.chars().count();
    }

    fn backspace(&mut self) {
        if self.selected {
            self.text.clear();
            self.cursor = 0;
            self.selected = false;
        } else if self.cursor > 0 {
            self.cursor -= 1;
            let start = self.byte_at(self.cursor);
            let end = self.byte_at(self.cursor + 1);
            self.text.replace_range(start..end, "");
        }
    }

    fn delete(&mut self) {
        if self.selected {
            self.text.clear();
            self.cursor = 0;
            self.selected = false;
        } else if self.cursor < self.len() {
            let start = self.byte_at(self.cursor);
            let end = self.byte_at(self.cursor + 1);
            self.text.replace_range(start..end, "");
        }
    }

    fn move_left(&mut self) {
        self.cursor = if self.selected {
            0
        } else {
            self.cursor.saturating_sub(1)
        };
        self.selected = false;
    }

    fn move_right(&mut self) {
        self.cursor = if self.selected {
            self.len()
        } else {
            (self.cursor + 1).min(self.len())
        };
        self.selected = false;
    }

    pub(super) fn is_crop(&self) -> bool {
        self.field.is_crop()
    }
}

fn parse_numeric(text: &str) -> Option<u32> {
    (!text.is_empty())
        .then(|| text.parse::<u32>().ok())
        .flatten()
}

impl Workspace {
    pub(super) fn numeric_value(&self, field: NumericField) -> Option<u32> {
        let rect = self.selection_shape_rect();
        let crop = self.crop;
        match field {
            NumericField::SelectionRadius => Some(self.selection_radius),
            NumericField::SelectionWidth => rect.map(|rect| rect.width),
            NumericField::SelectionHeight => rect.map(|rect| rect.height),
            NumericField::CropX => crop.map(|crop| crop.x),
            NumericField::CropY => crop.map(|crop| crop.y),
            NumericField::CropWidth => crop.map(|crop| crop.width),
            NumericField::CropHeight => crop.map(|crop| crop.height),
            NumericField::TextSize => Some(self.text_size().round() as u32),
            NumericField::TextLineSpacing => {
                Some((self.text_line_spacing() * 100.0).round() as u32)
            }
        }
    }

    pub(super) fn begin_numeric_edit(
        &mut self,
        field: NumericField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        if let Some(edit) = self.numeric_edit.as_mut() {
            if edit.field == field {
                edit.selected = true;
                edit.cursor = edit.len();
                self.numeric_focus.focus(window);
                cx.notify();
                return;
            }
            self.commit_numeric_edit(window, cx);
        }
        let Some(value) = self.numeric_value(field) else {
            return;
        };
        let original_crop = if field.is_crop() { self.crop } else { None };
        self.numeric_edit = Some(NumericEdit::new(field, value, original_crop));
        self.numeric_focus.focus(window);
        cx.notify();
    }

    pub(super) fn numeric_key(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.numeric_edit.is_none() {
            return false;
        }
        if key.key == "escape" {
            self.cancel_numeric_edit(window, cx);
            return true;
        }
        if key.key == "enter" {
            self.commit_numeric_edit(window, cx);
            self.focus.focus(window);
            return true;
        }
        if key.modifiers.secondary() {
            match key.key.as_str() {
                "a" => {
                    if let Some(edit) = self.numeric_edit.as_mut() {
                        edit.selected = true;
                        edit.cursor = edit.len();
                    }
                    cx.notify();
                }
                "c" => {
                    if let Some(edit) = self.numeric_edit.as_ref() {
                        cx.write_to_clipboard(ClipboardItem::new_string(edit.text.clone()));
                    }
                    cx.notify();
                }
                "x" => {
                    if let Some(edit) = self.numeric_edit.as_ref() {
                        cx.write_to_clipboard(ClipboardItem::new_string(edit.text.clone()));
                    }
                    if let Some(edit) = self.numeric_edit.as_mut() {
                        edit.text.clear();
                        edit.cursor = 0;
                        edit.selected = false;
                    }
                    self.apply_numeric_fields(window, cx);
                }
                "v" => {
                    let text = cx
                        .read_from_clipboard()
                        .and_then(|item| item.text())
                        .unwrap_or_default();
                    if let Some(edit) = self.numeric_edit.as_mut() {
                        edit.insert_digits(&text);
                    }
                    self.apply_numeric_fields(window, cx);
                }
                _ => return false,
            }
            return true;
        }
        if key.modifiers.alt || key.modifiers.control || key.modifiers.platform {
            return false;
        }
        let mut changed = false;
        {
            let Some(edit) = self.numeric_edit.as_mut() else {
                return false;
            };
            match key.key.as_str() {
                "backspace" => {
                    edit.backspace();
                    changed = true;
                }
                "delete" => {
                    edit.delete();
                    changed = true;
                }
                "left" => edit.move_left(),
                "right" => edit.move_right(),
                "home" => {
                    edit.cursor = 0;
                    edit.selected = false;
                }
                "end" => {
                    edit.cursor = edit.len();
                    edit.selected = false;
                }
                _ => {
                    if let Some(text) = key.key_char.as_deref()
                        && text.chars().any(|ch| ch.is_ascii_digit())
                    {
                        edit.insert_digits(text);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.apply_numeric_fields(window, cx);
        } else {
            cx.notify();
        }
        true
    }

    pub(super) fn apply_numeric_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((field, text, original, original_crop)) = self.numeric_edit.as_ref().map(|edit| {
            (
                edit.field,
                edit.text.clone(),
                edit.original,
                edit.original_crop,
            )
        }) else {
            return;
        };
        let Some(value) = parse_numeric(&text) else {
            self.restore_numeric_values(field, original, original_crop, window, cx);
            cx.notify();
            return;
        };
        match field {
            NumericField::TextSize => {
                // Sizes below the minimum are mid-typing; wait for more digits.
                if (MIN_PIXEL_SIZE as u32..=MAX_PIXEL_SIZE as u32).contains(&value) {
                    self.set_text_size(value as f32, window, cx);
                }
            }
            NumericField::TextLineSpacing => {
                let line = value as f32 / 100.0;
                if (MIN_LINE_SPACING..=MAX_LINE_SPACING).contains(&line) {
                    self.set_text_line_spacing(line, window, cx);
                }
            }
            NumericField::SelectionRadius => {
                let Some(rect) = self.selection_shape_rect() else {
                    return;
                };
                self.selection_radius = value.min(rect.width.min(rect.height) / 2);
                if let Some(selection) = self.selection_for_rect(rect) {
                    self.selection_preview = Some(selection);
                }
            }
            NumericField::SelectionWidth | NumericField::SelectionHeight => {
                if value == 0 {
                    self.restore_numeric_values(field, original, original_crop, window, cx);
                    cx.notify();
                    return;
                }
                let Some(mut rect) = self.selection_shape_rect() else {
                    return;
                };
                let Some(tab) = self.active_tab() else {
                    return;
                };
                let (width, height) = tab.document.content.size();
                if field == NumericField::SelectionWidth {
                    let left = u32::try_from(rect.x).unwrap_or(0);
                    rect.width = value.min(width.saturating_sub(left).max(1));
                } else {
                    let top = u32::try_from(rect.y).unwrap_or(0);
                    rect.height = value.min(height.saturating_sub(top).max(1));
                }
                if let Some(selection) = self.selection_for_rect(rect) {
                    self.selection_preview = Some(selection);
                }
            }
            NumericField::CropX | NumericField::CropY => {
                let Some(mut crop) = self.crop else {
                    return;
                };
                let Some(tab) = self.active_tab() else {
                    return;
                };
                let (width, height) = tab.document.content.size();
                if field == NumericField::CropX {
                    crop.x = value.min(width.saturating_sub(crop.width));
                } else {
                    crop.y = value.min(height.saturating_sub(crop.height));
                }
                self.crop = Some(crop);
            }
            NumericField::CropWidth | NumericField::CropHeight => {
                if value == 0 {
                    self.restore_numeric_values(field, original, original_crop, window, cx);
                    cx.notify();
                    return;
                }
                let Some(mut crop) = self.crop else {
                    return;
                };
                let Some(tab) = self.active_tab() else {
                    return;
                };
                let (width, height) = tab.document.content.size();
                if field == NumericField::CropWidth {
                    crop.width = value.min(width.saturating_sub(crop.x).max(1));
                } else {
                    crop.height = value.min(height.saturating_sub(crop.y).max(1));
                }
                self.crop = Some(crop);
            }
        }
        cx.notify();
    }

    pub(super) fn commit_numeric_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.numeric_edit.take() else {
            return;
        };
        if parse_numeric(&edit.text).is_none() {
            self.restore_numeric_edit(&edit, window, cx);
        } else if edit.field.is_selection()
            && let Some(selection) = self.selection_preview.take()
        {
            self.commit_selection(selection, window, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_numeric_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.numeric_edit.take() else {
            return;
        };
        self.restore_numeric_edit(&edit, window, cx);
        self.focus.focus(window);
        cx.notify();
    }

    fn restore_numeric_edit(
        &mut self,
        edit: &NumericEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.restore_numeric_values(edit.field, edit.original, edit.original_crop, window, cx);
    }

    fn restore_numeric_values(
        &mut self,
        field: NumericField,
        original: u32,
        original_crop: Option<CropRect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match field {
            NumericField::TextSize => self.set_text_size(original as f32, window, cx),
            NumericField::TextLineSpacing => {
                self.set_text_line_spacing(original as f32 / 100.0, window, cx)
            }
            NumericField::SelectionRadius => {
                self.selection_radius = original;
                self.selection_preview = None;
            }
            NumericField::SelectionWidth | NumericField::SelectionHeight => {
                self.selection_preview = None;
            }
            NumericField::CropX
            | NumericField::CropY
            | NumericField::CropWidth
            | NumericField::CropHeight => self.crop = original_crop,
        }
    }

    fn selection_for_rect(&self, rect: PixelRect) -> Option<Selection> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        let current = self.selection_preview.as_ref().or(Some(&tab.selection))?;
        match current {
            Selection::Ellipse(_) => Some(Selection::ellipse_rect(rect, width, height)),
            Selection::Rect(_) | Selection::RoundedRect(_) => Some(Selection::rounded_rect_rect(
                rect,
                self.selection_radius,
                width,
                height,
            )),
            _ => None,
        }
    }

    pub(super) fn numeric_chip(
        &self,
        field: NumericField,
        label: Option<&'static str>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<Div> {
        let active = self
            .numeric_edit
            .as_ref()
            .is_some_and(|edit| edit.field == field);
        let value = self.numeric_text(field);
        let chip = div()
            .id(("numeric-field", field.id()))
            .h(px(26.))
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .rounded_md()
            .bg(rgb(CANVAS))
            .border_1()
            .border_color(rgb(if active { ACCENT } else { BORDER }))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.begin_numeric_edit(field, window, cx);
                }),
            );
        if let Some(label) = label {
            chip.flex_1()
                .min_w_0()
                .justify_between()
                .child(
                    div()
                        .text_size(px(10.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(MUTED))
                        .child(label),
                )
                .child(value)
        } else {
            chip.min_w(px(48.)).justify_center().child(value)
        }
    }

    fn numeric_text(&self, field: NumericField) -> Div {
        let Some(edit) = self
            .numeric_edit
            .as_ref()
            .filter(|edit| edit.field == field)
        else {
            let value = self.numeric_value(field).unwrap_or(0);
            return div().text_size(px(12.)).child(value.to_string());
        };
        let text = div().flex().items_center().text_size(px(12.));
        if edit.selected && !edit.text.is_empty() {
            text.child(
                div()
                    .bg(rgb(ACCENT))
                    .text_color(rgb(CANVAS))
                    .child(edit.text.clone()),
            )
        } else {
            let at = edit.byte_at(edit.cursor);
            text.child(edit.text[..at].to_owned())
                .child(div().w(px(1.)).h(px(14.)).flex_shrink_0().bg(rgb(ACCENT)))
                .child(edit.text[at..].to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_deleted_values_do_not_create_a_zero() {
        let mut edit = NumericEdit::new(NumericField::SelectionWidth, 20, None);
        edit.selected = true;
        edit.backspace();
        assert!(edit.text.is_empty());
        edit.insert_digits("3");
        assert_eq!(edit.text, "3");
        edit.backspace();
        assert_eq!(edit.text, "");
        edit.insert_digits("12");
        assert_eq!(edit.text, "12");
    }

    #[test]
    fn empty_text_is_not_a_numeric_value() {
        assert_eq!(parse_numeric(""), None);
        assert_eq!(parse_numeric("0"), Some(0));
        assert_eq!(parse_numeric("0042"), Some(42));
        assert_eq!(parse_numeric("not a number"), None);
    }

    #[test]
    fn typed_digits_replace_a_selection_and_respect_the_cursor() {
        let mut edit = NumericEdit::new(NumericField::CropX, 12, None);
        edit.selected = false;
        edit.cursor = 1;
        edit.insert_digits("34");
        assert_eq!(edit.text, "1342");
        edit.selected = true;
        edit.insert_digits("7");
        assert_eq!(edit.text, "7");
    }
}
