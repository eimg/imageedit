use gpui::{
    ClipboardItem, Context, CursorStyle, Div, Keystroke, MouseButton, Window, div, prelude::*, px,
    rgb,
};

use super::Workspace;
use super::paint_ui::BrushField;
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
    TextLetterSpacing,
    Tolerance,
    BrushSize,
    BrushSpacing,
    BrushHardness,
    BrushOpacity,
    PathStrokeSize,
    ScaleWidth,
    ScaleHeight,
    RotateAngle,
    MoveX,
    MoveY,
    GridSpacing,
    SelectionX,
    SelectionY,
    SelectionRatioWidth,
    SelectionRatioHeight,
    CropRatioWidth,
    CropRatioHeight,
    BrushFlow,
    SelectionFeatherRadius,
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
            Self::TextLetterSpacing => 9,
            Self::Tolerance => 10,
            Self::BrushSize => 11,
            Self::BrushSpacing => 12,
            Self::BrushHardness => 13,
            Self::BrushOpacity => 14,
            Self::PathStrokeSize => 15,
            Self::ScaleWidth => 16,
            Self::ScaleHeight => 17,
            Self::RotateAngle => 18,
            Self::MoveX => 19,
            Self::MoveY => 20,
            Self::GridSpacing => 21,
            Self::SelectionX => 22,
            Self::SelectionY => 23,
            Self::SelectionRatioWidth => 24,
            Self::SelectionRatioHeight => 25,
            Self::CropRatioWidth => 26,
            Self::CropRatioHeight => 27,
            Self::BrushFlow => 28,
            Self::SelectionFeatherRadius => 29,
        }
    }

    fn is_crop(self) -> bool {
        matches!(
            self,
            Self::CropX | Self::CropY | Self::CropWidth | Self::CropHeight
        )
    }

    pub(super) fn is_scale(self) -> bool {
        matches!(self, Self::ScaleWidth | Self::ScaleHeight)
    }

    pub(super) fn is_move(self) -> bool {
        matches!(self, Self::MoveX | Self::MoveY)
    }

    fn parse(self, text: &str) -> Option<i64> {
        if self == Self::RotateAngle {
            let value = text.parse::<f64>().ok()?;
            (value.is_finite() && (-360.0..=360.0).contains(&value))
                .then(|| (value * 10.).round() as i64)
        } else {
            parse_numeric(text).filter(|&value| {
                (!self.is_scale()
                    || (0..=i64::from(crate::document::MAX_NEW_EDGE)).contains(&value))
                    && (!self.is_move() || i32::try_from(value).is_ok())
            })
        }
    }

    fn format(self, value: i64) -> String {
        if self == Self::RotateAngle {
            format!("{:.1}", value as f64 / 10.)
        } else {
            value.to_string()
        }
    }

    pub(super) fn is_selection(self) -> bool {
        matches!(
            self,
            Self::SelectionRadius
                | Self::SelectionWidth
                | Self::SelectionHeight
                | Self::SelectionX
                | Self::SelectionY
        )
    }

    fn accepts_character(self, ch: char) -> bool {
        ch.is_ascii_digit()
            || ((matches!(self, Self::TextLetterSpacing | Self::RotateAngle) || self.is_move())
                && ch == '-')
            || (self == Self::RotateAngle && ch == '.')
    }

    fn is_tool_option(self) -> bool {
        matches!(
            self,
            Self::TextLetterSpacing
                | Self::Tolerance
                | Self::BrushSize
                | Self::BrushSpacing
                | Self::BrushHardness
                | Self::BrushOpacity
                | Self::BrushFlow
                | Self::PathStrokeSize
                | Self::RotateAngle
                | Self::GridSpacing
                | Self::SelectionRatioWidth
                | Self::SelectionRatioHeight
                | Self::CropRatioWidth
                | Self::CropRatioHeight
                | Self::SelectionFeatherRadius
        )
    }

    fn constrain(self, value: i64) -> i64 {
        let (min, max) = match self {
            Self::Tolerance => (0, 255),
            Self::GridSpacing => (1, 4096),
            Self::SelectionFeatherRadius => (0, 128),
            Self::SelectionRatioWidth
            | Self::SelectionRatioHeight
            | Self::CropRatioWidth
            | Self::CropRatioHeight => (1, 16384),
            Self::RotateAngle => (-3600, 3600),
            Self::BrushSize | Self::PathStrokeSize => (1, 500),
            Self::BrushSpacing => (1, 100),
            Self::BrushHardness | Self::BrushOpacity | Self::BrushFlow => (0, 100),
            Self::TextLetterSpacing => (
                crate::text::MIN_LETTER_SPACING as i64,
                crate::text::MAX_LETTER_SPACING as i64,
            ),
            _ => (0, i64::from(u32::MAX)),
        };
        value.clamp(min, max)
    }
}

pub(super) struct NumericEdit {
    pub(super) field: NumericField,
    text: String,
    cursor: usize,
    selected: bool,
    original: i64,
    original_crop: Option<CropRect>,
}

pub(super) enum NumericFollowup {
    Key(Keystroke),
    Focus(NumericField),
    Effect {
        reopen: bool,
    },
    LayerLock {
        tab: usize,
        layer: crate::document::LayerId,
        command: super::layers::LayerCommand,
    },
}

pub(super) struct PendingNumeric {
    tab: usize,
    generation: u64,
    followup: NumericFollowup,
}

impl PendingNumeric {
    fn accepts_completion(
        &self,
        tab: &super::Tab,
        id: usize,
        generation: u64,
        published: bool,
    ) -> bool {
        self.tab == id
            && tab.id == id
            && self.generation == generation
            && !tab.busy()
            && generation.checked_add(u64::from(published)) == Some(tab.history.generation())
    }
}

impl NumericEdit {
    fn new(field: NumericField, value: i64, crop: Option<CropRect>) -> Self {
        let text = field.format(value);
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
        let decimal = self.field == NumericField::RotateAngle;
        let digits: String = text
            .chars()
            .filter(|&ch| self.field.accepts_character(ch))
            .collect();
        if digits.is_empty() {
            return;
        }
        let mut next = if self.selected {
            String::new()
        } else {
            self.text.clone()
        };
        let at = if self.selected {
            0
        } else {
            self.byte_at(self.cursor)
        };
        next.insert_str(at, &digits);
        let unsigned = next.strip_prefix('-').unwrap_or(&next);
        if unsigned.len() > 10
            || !unsigned
                .chars()
                .all(|ch| ch.is_ascii_digit() || (decimal && ch == '.'))
            || unsigned.matches('.').count() > 1
        {
            return;
        }
        self.text = next;
        self.cursor = at + digits.len();
        self.selected = false;
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

fn parse_numeric(text: &str) -> Option<i64> {
    (!text.is_empty())
        .then(|| text.parse::<i64>().ok())
        .flatten()
}

impl Workspace {
    pub(super) fn numeric_value(&self, field: NumericField) -> Option<i64> {
        let rect = self.selection_shape_rect();
        let crop = self.crop;
        match field {
            NumericField::SelectionX => rect.map(|rect| rect.x),
            NumericField::SelectionY => rect.map(|rect| rect.y),
            NumericField::SelectionRatioWidth => Some(i64::from(self.selection_ratio_value.width)),
            NumericField::SelectionRatioHeight => {
                Some(i64::from(self.selection_ratio_value.height))
            }
            NumericField::CropRatioWidth => Some(i64::from(self.crop_ratio_value.width)),
            NumericField::CropRatioHeight => Some(i64::from(self.crop_ratio_value.height)),
            NumericField::SelectionRadius => Some(i64::from(self.active_selection_radius())),
            NumericField::SelectionWidth => rect.map(|rect| i64::from(rect.width)),
            NumericField::SelectionHeight => rect.map(|rect| i64::from(rect.height)),
            NumericField::CropX => crop.map(|crop| i64::from(crop.x)),
            NumericField::CropY => crop.map(|crop| i64::from(crop.y)),
            NumericField::CropWidth => crop.map(|crop| i64::from(crop.width)),
            NumericField::CropHeight => crop.map(|crop| i64::from(crop.height)),
            NumericField::TextSize => Some(self.text_size().round() as i64),
            NumericField::TextLineSpacing => {
                Some((self.text_line_spacing() * 100.0).round() as i64)
            }
            NumericField::TextLetterSpacing => Some(self.text_letter_spacing().round() as i64),
            NumericField::Tolerance => Some(i64::from(self.tolerance)),
            NumericField::BrushSize => Some(i64::from(self.brush_value(BrushField::Size))),
            NumericField::BrushSpacing => Some(i64::from(self.brush_value(BrushField::Spacing))),
            NumericField::BrushHardness => Some(i64::from(self.brush_value(BrushField::Hardness))),
            NumericField::BrushOpacity => Some(i64::from(self.brush_value(BrushField::Opacity))),
            NumericField::BrushFlow => Some(i64::from(self.brush_value(BrushField::Flow))),
            NumericField::PathStrokeSize => Some(i64::from(self.brushes.outline.diameter)),
            NumericField::ScaleWidth => self.scale_size().map(|size| i64::from(size.0)),
            NumericField::ScaleHeight => self.scale_size().map(|size| i64::from(size.1)),
            NumericField::RotateAngle => self
                .warp_status()?
                .degrees
                .map(|degrees| (degrees * 10.).round() as i64),
            NumericField::MoveX => self.move_offset().map(|offset| i64::from(offset.0)),
            NumericField::MoveY => self.move_offset().map(|offset| i64::from(offset.1)),
            NumericField::GridSpacing => Some(i64::from(self.grid_spacing)),
            NumericField::SelectionFeatherRadius => Some(i64::from(self.selection_feather_radius)),
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
        if field == NumericField::RotateAngle
            && self.warp_status().is_none_or(|status| status.applying)
        {
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
            if self.commit_numeric_with_followup(NumericFollowup::Focus(field), window, cx) {
                return;
            }
        }
        let Some(value) = self.numeric_value(field) else {
            return;
        };
        let original_crop = if field.is_crop() { self.crop } else { None };
        if field.is_scale() && !self.begin_scale_numeric(window, cx) {
            return;
        }
        if field.is_move() && !self.begin_move_numeric(window, cx) {
            return;
        }
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
                        && text.chars().any(|ch| edit.field.accepts_character(ch))
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
        let Some(value) = field.parse(&text) else {
            self.restore_numeric_values(field, original, original_crop, window, cx);
            cx.notify();
            return;
        };
        if field.is_tool_option() {
            self.apply_tool_numeric(field, value, window, cx);
            return;
        }
        if field.is_scale() {
            if let Ok(value) = u32::try_from(value) {
                if value == 0 {
                    self.restore_scale_size(cx);
                } else {
                    self.preview_scale_size(field == NumericField::ScaleWidth, value, cx);
                }
            }
            return;
        }
        if field.is_move() {
            self.preview_move_position(field == NumericField::MoveX, value as i32, cx);
            return;
        }
        let value = value.clamp(0, i64::from(u32::MAX)) as u32;
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
                let rect = self.selection_shape_rect();
                let max = rect.map_or_else(
                    || {
                        self.active_tab().map_or(0, |tab| {
                            let (width, height) = tab.document.content.size();
                            width.max(height)
                        })
                    },
                    |rect| rect.width.min(rect.height) / 2,
                );
                self.selection_radius = value.min(max);
                if let Some(selection) =
                    rect.and_then(|rect| self.selection_for_rect(rect, Some(self.selection_radius)))
                {
                    self.selection_preview = Some(selection);
                }
            }
            NumericField::SelectionX | NumericField::SelectionY => {
                let Some(mut rect) = self.selection_shape_rect() else {
                    return;
                };
                let Some(tab) = self.active_tab() else {
                    return;
                };
                if field == NumericField::SelectionX {
                    rect.x = i64::from(value.min(tab.document.width().saturating_sub(rect.width)));
                } else {
                    rect.y =
                        i64::from(value.min(tab.document.height().saturating_sub(rect.height)));
                }
                self.selection_preview = self.selection_for_rect(rect, None);
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
                let limits = (
                    width
                        .saturating_sub(u32::try_from(rect.x).unwrap_or(0))
                        .max(1),
                    height
                        .saturating_sub(u32::try_from(rect.y).unwrap_or(0))
                        .max(1),
                );
                if let Some(ratio) = self.effective_selection_ratio() {
                    (rect.width, rect.height) =
                        ratio.fit(field == NumericField::SelectionWidth, value, limits);
                } else if field == NumericField::SelectionWidth {
                    rect.width = value.min(limits.0);
                } else {
                    rect.height = value.min(limits.1);
                }
                if let Some(selection) = self.selection_for_rect(rect, None) {
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
                let limits = (
                    width.saturating_sub(crop.x).max(1),
                    height.saturating_sub(crop.y).max(1),
                );
                if let Some(ratio) = self.crop_ratio {
                    (crop.width, crop.height) =
                        ratio.fit(field == NumericField::CropWidth, value, limits);
                } else if field == NumericField::CropWidth {
                    crop.width = value.min(limits.0);
                } else {
                    crop.height = value.min(limits.1);
                }
                self.crop = Some(crop);
            }
            _ => unreachable!("tool options were handled before selection and crop fields"),
        }
        cx.notify();
    }

    pub(super) fn commit_numeric_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.numeric_edit.take() else {
            return;
        };
        if edit.field.parse(&edit.text).is_none() {
            self.restore_numeric_edit(&edit, window, cx);
            if edit.field.is_scale() {
                self.cancel_scale(window, cx);
            }
            if edit.field.is_move() {
                self.cancel_move(window, cx);
            }
        } else if edit.field.is_scale() {
            self.finish_scale(window, cx);
        } else if edit.field.is_move() {
            self.finish_move(window, cx);
        } else if edit.field.is_selection()
            && let Some(selection) = self.selection_preview.take()
        {
            self.commit_selection(selection, window, cx);
        }
        cx.notify();
    }

    pub(super) fn commit_numeric_with_followup(
        &mut self,
        followup: NumericFollowup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let before = self
            .active_tab()
            .filter(|tab| !tab.busy())
            .map(|tab| (tab.id, tab.history.generation()));
        self.commit_numeric_edit(window, cx);
        let Some((id, generation)) = before else {
            return false;
        };
        if self.active_tab().is_some_and(|tab| {
            tab.id == id && tab.editing && tab.history.generation() == generation
        }) {
            self.numeric_followup = Some(PendingNumeric {
                tab: id,
                generation,
                followup,
            });
            return true;
        }
        false
    }

    pub(super) fn finish_numeric_followup(
        &mut self,
        id: usize,
        generation: u64,
        published: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .numeric_followup
            .as_ref()
            .is_none_or(|pending| pending.tab != id)
        {
            return;
        }
        let pending = self.numeric_followup.take().unwrap();
        let Some(tab) = self
            .active_tab()
            .filter(|tab| pending.accepts_completion(tab, id, generation, published))
        else {
            return;
        };
        let settled_generation = tab.history.generation();
        let tool = self.tool;
        let entity = cx.weak_entity();
        // Menu availability and key contexts must render the settled edit first.
        // Dispatch outside the entity update so keystroke interceptors can borrow it.
        window.on_next_frame(move |window, cx| {
            let key = entity
                .update(cx, |this, cx| {
                    if this.form.is_some()
                        || this.dialog_open
                        || this.numeric_edit.is_some()
                        || this.tool != tool
                        || this.active_tab().is_none_or(|tab| {
                            tab.id != id
                                || tab.busy()
                                || tab.history.generation() != settled_generation
                        })
                    {
                        return None;
                    }
                    match pending.followup {
                        NumericFollowup::Key(key) => {
                            this.focus.focus(window);
                            Some(key)
                        }
                        NumericFollowup::Focus(field) => {
                            this.begin_numeric_edit(field, window, cx);
                            None
                        }
                        NumericFollowup::Effect { reopen } => {
                            this.run_last_effect(reopen, window, cx);
                            None
                        }
                        NumericFollowup::LayerLock {
                            tab,
                            layer,
                            command,
                        } => {
                            this.toggle_layer_lock(tab, layer, command, window, cx);
                            None
                        }
                    }
                })
                .ok()
                .flatten();
            if let Some(key) = key {
                window.dispatch_keystroke(key, cx);
            }
        });
    }

    pub(super) fn cancel_numeric_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.numeric_edit.take() else {
            return;
        };
        self.restore_numeric_edit(&edit, window, cx);
        if edit.field.is_scale() {
            self.cancel_scale(window, cx);
        }
        if edit.field.is_move() {
            self.cancel_move(window, cx);
        }
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
        original: i64,
        original_crop: Option<CropRect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if field.is_tool_option() {
            self.apply_tool_numeric(field, original, window, cx);
            return;
        }
        if field.is_scale() {
            self.restore_scale_size(cx);
            return;
        }
        if field.is_move() {
            self.restore_move_position(cx);
            return;
        }
        match field {
            NumericField::TextSize => self.set_text_size(original as f32, window, cx),
            NumericField::TextLineSpacing => {
                self.set_text_line_spacing(original as f32 / 100.0, window, cx)
            }
            NumericField::SelectionRadius => {
                self.selection_radius = original as u32;
                self.selection_preview = None;
            }
            NumericField::SelectionWidth
            | NumericField::SelectionHeight
            | NumericField::SelectionX
            | NumericField::SelectionY => {
                self.selection_preview = None;
            }
            NumericField::CropX
            | NumericField::CropY
            | NumericField::CropWidth
            | NumericField::CropHeight => self.crop = original_crop,
            _ => unreachable!("tool options were restored before selection and crop fields"),
        }
    }

    fn apply_tool_numeric(
        &mut self,
        field: NumericField,
        value: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = field.constrain(value);
        match field {
            NumericField::Tolerance => self.tolerance = value as u8,
            NumericField::GridSpacing => self.grid_spacing = value as u32,
            NumericField::SelectionFeatherRadius => self.selection_feather_radius = value as u32,
            NumericField::SelectionRatioWidth | NumericField::SelectionRatioHeight => {
                if field == NumericField::SelectionRatioWidth {
                    self.selection_ratio_value.width = value as u32;
                } else {
                    self.selection_ratio_value.height = value as u32;
                }
                self.selection_ratio = Some(self.selection_ratio_value);
            }
            NumericField::CropRatioWidth | NumericField::CropRatioHeight => {
                if field == NumericField::CropRatioWidth {
                    self.crop_ratio_value.width = value as u32;
                } else {
                    self.crop_ratio_value.height = value as u32;
                }
                self.crop_ratio = Some(self.crop_ratio_value);
            }
            NumericField::BrushSize => self.set_brush_value(BrushField::Size, value as u32, cx),
            NumericField::BrushSpacing => {
                self.set_brush_value(BrushField::Spacing, value as u32, cx)
            }
            NumericField::BrushHardness => {
                self.set_brush_value(BrushField::Hardness, value as u32, cx)
            }
            NumericField::BrushOpacity => {
                self.set_brush_value(BrushField::Opacity, value as u32, cx)
            }
            NumericField::BrushFlow => self.set_brush_value(BrushField::Flow, value as u32, cx),
            NumericField::PathStrokeSize => self.brushes.outline.diameter = value as u32,
            NumericField::RotateAngle => self.set_warp_angle(value as f64 / 10., window, cx),
            NumericField::TextLetterSpacing => {
                self.set_text_letter_spacing(value as f32, window, cx)
            }
            _ => unreachable!("only tool settings use this numeric setter"),
        }
        cx.notify();
    }

    pub(super) fn numeric_stepper(
        &self,
        field: NumericField,
        down: gpui::Stateful<Div>,
        up: gpui::Stateful<Div>,
        cx: &Context<Self>,
    ) -> Div {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(down)
            .child(self.numeric_chip(field, None, cx))
            .child(up)
    }

    fn selection_for_rect(&self, rect: PixelRect, radius: Option<u32>) -> Option<Selection> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        let current = self.selection_preview.as_ref().or(Some(&tab.selection))?;
        match current {
            Selection::Rect(_) | Selection::RoundedRect(_) if radius.is_some() => {
                Some(Selection::rounded_rect_quality(
                    rect,
                    radius?,
                    width,
                    height,
                    if matches!(current, Selection::RoundedRect(_)) {
                        current.is_antialiased()
                    } else {
                        self.selection_antialias
                    },
                ))
            }
            Selection::Rect(_) | Selection::RoundedRect(_) | Selection::Ellipse(_) => Some(
                super::selection::placed_selection(current.clone(), rect, width, height),
            ),
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
            return div().text_size(px(12.)).child(field.format(value));
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
    fn creation_feather_radius_is_a_bounded_setting_and_empty_text_keeps_its_original() {
        let field = NumericField::SelectionFeatherRadius;
        assert!(field.is_tool_option());
        assert!(!field.is_selection());
        assert_eq!(field.constrain(-1), 0);
        assert_eq!(field.constrain(129), 128);
        let mut edit = NumericEdit::new(field, 8, None);
        edit.insert_digits("128");
        assert_eq!(field.parse(&edit.text), Some(128));
        edit.selected = true;
        edit.backspace();
        assert_eq!(field.parse(&edit.text), None);
        assert_eq!(edit.original, 8);
        assert!(!field.accepts_character('-'));
        assert!(!field.accepts_character('.'));
    }

    #[test]
    fn move_coordinates_accept_signed_pixel_positions_and_reject_overflow() {
        for field in [NumericField::MoveX, NumericField::MoveY] {
            let mut edit = NumericEdit::new(field, 12, None);
            edit.insert_digits("-2147483648");
            assert_eq!(field.parse(&edit.text), Some(i64::from(i32::MIN)));
            assert_eq!(field.parse("2147483647"), Some(i64::from(i32::MAX)));
            for invalid in ["-2147483649", "2147483648", "-", "", "2.5"] {
                assert_eq!(field.parse(invalid), None);
            }
            assert_eq!(edit.original, 12);
            assert!(field.accepts_character('-'));
        }
    }

    #[test]
    fn angle_edits_keep_decimal_degrees_and_reject_incomplete_or_invalid_input() {
        let field = NumericField::RotateAngle;
        let mut edit = NumericEdit::new(field, -125, None);
        assert_eq!(edit.text, "-12.5");
        edit.insert_digits("-45.6");
        assert_eq!(field.parse(&edit.text), Some(-456));
        edit.insert_digits(".");
        assert_eq!(edit.text, "-45.6");
        edit.selected = true;
        edit.insert_digits("-");
        assert_eq!(field.parse(&edit.text), None);
        edit.insert_digits(".5");
        assert_eq!(field.parse(&edit.text), Some(-5));
        assert_eq!(edit.original, -125);
        for invalid in ["", "-", ".", "NaN", "inf", "360.1", "-361"] {
            assert_eq!(field.parse(invalid), None);
        }
        assert_eq!(field.parse("12.34"), Some(123));
        assert_eq!(field.parse("360"), Some(3600));
        assert_eq!(field.format(123), "12.3");
        assert!(field.accepts_character('-'));
        assert!(field.accepts_character('.'));
        assert!(!NumericField::ScaleWidth.accepts_character('.'));
        assert!(!NumericField::ScaleWidth.accepts_character('-'));
    }

    #[test]
    fn scale_fields_reject_oversized_values_instead_of_committing_a_stale_preview() {
        for field in [NumericField::ScaleWidth, NumericField::ScaleHeight] {
            assert_eq!(field.parse("0"), Some(0));
            assert_eq!(field.parse("16384"), Some(16384));
            for invalid in ["", "-1", "16385", "4294967296", "1.5"] {
                assert_eq!(field.parse(invalid), None);
            }
        }
    }

    #[test]
    fn numeric_followup_waits_for_its_selection_commit_and_rejects_stale_context() {
        let mut tab = super::super::tests::opacity_tab();
        let id = tab.id;
        let generation = tab.history.generation();
        let key = Keystroke::parse("ctrl-s").unwrap();
        let pending = PendingNumeric {
            tab: id,
            generation,
            followup: NumericFollowup::Key(key.clone()),
        };
        let mut state = tab.state();
        state.selection = Selection::rectangle((0, 0), (0, 0), 2, 2);
        let selection = state.selection.clone();
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            crate::preview::Dirty::Unpainted,
        )
        .unwrap();
        tab.editing = true;
        assert!(!pending.accepts_completion(&tab, id, generation, true));
        assert!(
            tab.publish_change(
                generation,
                super::super::Change::Selection,
                Some((state.content, state.active_layer, state.selection, patch)),
                1
            )
            .0
        );
        assert_eq!(tab.selection, selection);
        assert!(!tab.history.is_dirty());
        assert!(pending.accepts_completion(&tab, id, generation, true));
        assert!(!pending.accepts_completion(&tab, id, generation, false));
        assert!(!pending.accepts_completion(&tab, id, generation + 1, true));
        tab.id += 1;
        assert!(!pending.accepts_completion(&tab, id, generation, true));
        assert!(matches!(pending.followup, NumericFollowup::Key(queued) if queued == key));
    }

    #[test]
    fn unchanged_numeric_commit_can_still_focus_the_next_field() {
        let tab = super::super::tests::opacity_tab();
        let pending = PendingNumeric {
            tab: tab.id,
            generation: tab.history.generation(),
            followup: NumericFollowup::Focus(NumericField::SelectionHeight),
        };
        assert!(pending.accepts_completion(&tab, tab.id, tab.history.generation(), false));
        assert!(matches!(
            pending.followup,
            NumericFollowup::Focus(NumericField::SelectionHeight)
        ));
    }

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

    #[test]
    fn signed_letter_spacing_keeps_a_partial_sign_and_normal_cursor_deletion() {
        let mut edit = NumericEdit::new(NumericField::TextLetterSpacing, -3, None);
        edit.insert_digits("-");
        assert_eq!(parse_numeric(&edit.text), None);
        assert_eq!(edit.original, -3);
        edit.insert_digits("12");
        assert_eq!(parse_numeric(&edit.text), Some(-12));
        edit.move_left();
        edit.insert_digits("-"); // A sign inside a number is rejected.
        assert_eq!(edit.text, "-12");
        edit.backspace();
        assert_eq!(parse_numeric(&edit.text), Some(-2));
        edit.selected = true;
        edit.delete();
        assert_eq!(parse_numeric(&edit.text), None);
        assert_eq!(edit.original, -3);
    }

    #[test]
    fn typed_tool_values_obey_the_same_limits_as_the_step_buttons() {
        let mut edit = NumericEdit::new(NumericField::Tolerance, 32, None);
        edit.insert_digits("255");
        assert_eq!(parse_numeric(&edit.text), Some(255));
        edit.move_left();
        edit.backspace();
        assert_eq!(parse_numeric(&edit.text), Some(25));
        for (field, min, max) in [
            (NumericField::Tolerance, 0, 255),
            (NumericField::BrushSize, 1, 500),
            (NumericField::BrushSpacing, 1, 100),
            (NumericField::BrushHardness, 0, 100),
            (NumericField::BrushOpacity, 0, 100),
            (NumericField::BrushFlow, 0, 100),
            (NumericField::PathStrokeSize, 1, 500),
            (NumericField::TextLetterSpacing, -64, 256),
            (NumericField::SelectionRatioWidth, 1, 16384),
            (NumericField::SelectionRatioHeight, 1, 16384),
            (NumericField::CropRatioWidth, 1, 16384),
            (NumericField::CropRatioHeight, 1, 16384),
        ] {
            assert_eq!(field.constrain(i64::MIN), min);
            assert_eq!(field.constrain(i64::MAX), max);
            assert_eq!(field.constrain((min + max) / 2), (min + max) / 2);
        }
    }
}
