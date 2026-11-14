use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

use gpui::{
    ClipboardItem, Context, CursorStyle, Div, KeyDownEvent, Keystroke, MouseButton, Window, div,
    prelude::*, px, rgb,
};

use super::Workspace;
use super::dialogs::{Field, FieldMode};
use super::layers::MovePaint;
use super::numeric::NumericField;
use super::settings_ui::ToolSection;
use super::view::{
    ACCENT, BORDER, CANVAS, HOVER, MUTED, SELECTED, TEXT, choice_button, info_note, option_action,
    option_row, section_label, segmented, step_button, toggle_button,
};
use crate::commands::{
    CopyImage, CutImage, FinishText, PasteImage, RasterizeText, Redo, SelectAll, TextCopy, TextCut,
    TextPaste, TextSelectAll, Undo,
};
use crate::document::{self, LayerId};
use crate::preview::Dirty;
use crate::text::{
    self, CaretAffinity, CaretMap, FontFace, MAX_LETTER_SPACING, MAX_LINE_SPACING, MAX_PIXEL_SIZE,
    MAX_TEXT_CHARS, MIN_LETTER_SPACING, MIN_LINE_SPACING, MIN_PIXEL_SIZE, Spacing, TextAlign,
    TextError, TextLayout, TextSpec,
};

fn text_command_key(key: &Keystroke, windows: bool) -> bool {
    key.modifiers.control || key.modifiers.platform || (windows && key.modifiers.alt)
}

fn font_query_navigation_key(key: &Keystroke) -> bool {
    (key.modifiers.alt || key.modifiers.control || key.modifiers.platform)
        && matches!(
            key.key.as_str(),
            "left" | "right" | "up" | "down" | "home" | "end" | "backspace" | "delete"
        )
}

fn typed_text(key: &Keystroke, windows: bool) -> Option<&str> {
    if text_command_key(key, windows) {
        return None;
    }
    // GPUI's Windows backend reports named Space without key_char.
    key.key_char
        .as_deref()
        .or_else(|| (key.key == "space").then_some(" "))
}

/// The color dialog uses this swatch index for the text color.
pub(super) const TEXT_SWATCH: usize = 2;

// One caret phase. The caret is solid for a phase after each edit or move.
const CARET_BLINK_MS: u128 = 530;

const GENERIC_FAMILIES: [(&str, &str); 3] = [
    ("Sans Serif", "sans-serif"),
    ("Serif", "serif"),
    ("Monospace", "monospace"),
];

pub(super) struct FontSearch {
    field: Field,
    selected: usize,
    undo: Vec<Field>,
    redo: Vec<Field>,
}

impl Default for FontSearch {
    fn default() -> Self {
        Self {
            field: Field::new(String::new()),
            selected: 0,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl FontSearch {
    fn edit(&mut self, edit: impl FnOnce(&mut Field)) -> bool {
        let previous = self.field.clone();
        edit(&mut self.field);
        if previous.text() == self.field.text() {
            return false;
        }
        if self.undo.len() == 100 {
            self.undo.remove(0);
        }
        self.undo.push(previous);
        self.redo.clear();
        self.selected = 0;
        true
    }

    fn step_history(&mut self, redo: bool) -> bool {
        let target = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        if let Some(target) = target {
            let previous = std::mem::replace(&mut self.field, target);
            if redo {
                self.undo.push(previous);
            } else {
                self.redo.push(previous);
            }
            self.selected = 0;
            return true;
        }
        false
    }
}

fn matches_font(label: &str, family: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    label.to_lowercase().contains(&query) || family.to_lowercase().contains(&query)
}

/// The style the next new text starts with. Editing a draft updates it too.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct TextDefaults {
    family: String,
    face: FontFace,
    size: f32,
    spacing: Spacing,
    layout: TextLayout,
}

impl Default for TextDefaults {
    fn default() -> Self {
        Self {
            family: "sans-serif".to_owned(),
            face: FontFace::default(),
            size: 32.0,
            spacing: Spacing::default(),
            layout: TextLayout::default(),
        }
    }
}

impl TextDefaults {
    pub(super) fn valid(&self) -> bool {
        !self.family.is_empty()
            && self.family.chars().count() <= crate::text::MAX_FAMILY_CHARS
            && !self.family.chars().any(char::is_control)
            && self.size.is_finite()
            && (MIN_PIXEL_SIZE..=MAX_PIXEL_SIZE).contains(&self.size)
            && self.spacing.allowed()
            && self.layout.allowed()
    }
}

pub(super) type TextContinuation =
    Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// A caret and selection rectangles in canvas pixels.
#[derive(Clone)]
pub(super) struct TextCaret {
    pub x: f32,
    pub top: f32,
    pub height: f32,
    pub selection: Vec<(f32, f32, f32, f32)>,
    // The box of the text being edited, so a click that starts new text is obvious.
    pub frame: (f32, f32, f32, f32),
    pub visible: bool,
}

#[derive(Clone)]
pub(super) struct TextDraft {
    tab: usize,
    generation: u64,
    above: LayerId,
    opacity: u8,
    substitution: Option<String>,
    layer: Option<LayerId>,
    anchor: (i32, i32),
    source: String,
    cursor: usize,
    cursor_affinity: CaretAffinity,
    selection_anchor: Option<usize>,
    family: String,
    face: FontFace,
    size: f32,
    color: image::Rgba<u8>,
    spacing: Spacing,
    layout: TextLayout,
    rendered: Arc<text::RenderedText>,
    carets: Arc<CaretMap>,
    // A click that opened this draft places the caret once the layout is known.
    pending_hit: Option<(f32, f32)>,
    pending_extend: bool,
    pending_drag_hit: Option<(f32, f32)>,
    dragging: bool,
    baseline: Arc<text::RenderedText>,
    baseline_anchor: (i32, i32),
    undo: Vec<TextEdit>,
    redo: Vec<TextEdit>,
}

#[derive(Clone)]
struct TextEdit {
    source: String,
    cursor: usize,
    cursor_affinity: CaretAffinity,
    selection_anchor: Option<usize>,
    family: String,
    face: FontFace,
    size: f32,
    color: image::Rgba<u8>,
    spacing: Spacing,
    layout: TextLayout,
    anchor: (i32, i32),
}

struct TextPreviewInput {
    rendered: Arc<text::RenderedText>,
    carets: Arc<CaretMap>,
    spec: TextSpec,
    opacity: u8,
    layer: Option<LayerId>,
    above: LayerId,
    anchor: (i32, i32),
}

#[derive(Clone, PartialEq)]
pub(super) struct RecoveryText {
    spec: TextSpec,
    anchor: (i32, i32),
    layer: Option<LayerId>,
    above: LayerId,
}

impl TextDraft {
    pub(super) fn recovery_signature(&self) -> RecoveryText {
        RecoveryText {
            spec: self.spec(),
            anchor: self.anchor,
            layer: self.layer,
            above: self.above,
        }
    }

    pub(super) fn recovery_tab(&self) -> usize {
        self.tab
    }

    pub(super) fn recovery_content(
        &self,
        content: &mut document::DocumentContent,
        active: &mut LayerId,
    ) -> Option<serde_json::Value> {
        let dirty = self.apply(content, active);
        if dirty != Dirty::None || !self.changed() {
            return None;
        }
        Some(
            serde_json::json!({"source": self.source, "family": self.family, "size": self.size,
            "bold": self.face.bold, "italic": self.face.italic, "color": self.color.0,
            "letter_spacing": self.spacing.letter, "line_spacing": self.spacing.line,
            "layout": self.layout, "anchor": self.anchor, "layer": self.layer.map(|id| id.0), "above": self.above.0}),
        )
    }

    fn selected_range(&self) -> Option<std::ops::Range<usize>> {
        let anchor = self.selection_anchor?;
        (anchor != self.cursor).then(|| anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    fn move_to(&mut self, byte: usize, extend: bool) {
        if byte > self.source.len() || !self.source.is_char_boundary(byte) {
            return;
        }
        if extend {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
        self.cursor = byte;
        self.cursor_affinity = CaretAffinity::Downstream;
    }

    fn move_to_position(&mut self, position: (usize, CaretAffinity), extend: bool) {
        if position.0 <= self.source.len() && self.source.is_char_boundary(position.0) {
            self.move_to(position.0, extend);
            self.cursor_affinity = position.1;
        }
    }

    fn navigation_position(
        &self,
        key: &str,
        word: bool,
        command: bool,
        extend: bool,
    ) -> (usize, CaretAffinity) {
        use CaretAffinity::{Downstream, Upstream};
        if !extend
            && !word
            && !command
            && let Some(range) = self.selected_range()
        {
            if key == "left" {
                return (range.start, Downstream);
            }
            if key == "right" {
                return (range.end, Upstream);
            }
        }
        if (word && key == "home") || (command && key == "up") {
            return (0, Downstream);
        }
        if (word && key == "end") || (command && key == "down") {
            return (self.source.len(), Upstream);
        }
        let layout_valid = self.carets.matches(&self.spec());
        if key == "home" || (command && key == "left") {
            if self.layout.width.is_some() && layout_valid {
                return self
                    .carets
                    .line_edge_position(self.cursor, self.cursor_affinity, false)
                    .unwrap_or((self.cursor, self.cursor_affinity));
            }
            return (
                self.source[..self.cursor]
                    .rfind('\n')
                    .map_or(0, |at| at + 1),
                Downstream,
            );
        }
        if key == "end" || (command && key == "right") {
            if self.layout.width.is_some() && layout_valid {
                return self
                    .carets
                    .line_edge_position(self.cursor, self.cursor_affinity, true)
                    .unwrap_or((self.cursor, self.cursor_affinity));
            }
            return (
                self.source[self.cursor..]
                    .find('\n')
                    .map_or(self.source.len(), |at| self.cursor + at),
                Upstream,
            );
        }
        if matches!(key, "up" | "down") {
            return layout_valid
                .then(|| {
                    self.carets
                        .vertical_position(self.cursor, self.cursor_affinity, key == "down")
                })
                .flatten()
                .unwrap_or((self.cursor, self.cursor_affinity));
        }
        // Crossing a shared wrap boundary changes visual side before consuming
        // another grapheme. Word navigation retains its existing byte semantics.
        if !word && layout_valid {
            let before = self.carets.at_affinity(self.cursor, Upstream);
            let after = self.carets.at_affinity(self.cursor, Downstream);
            if before.zip(after).is_some_and(|(a, b)| a.line != b.line) {
                if key == "left" && self.cursor_affinity == Downstream {
                    return (self.cursor, Upstream);
                }
                if key == "right" && self.cursor_affinity == Upstream {
                    return (self.cursor, Downstream);
                }
            }
        }
        let forward = key == "right";
        let byte = if word {
            word_boundary(&self.source, self.cursor, forward)
        } else {
            grapheme_boundary(&self.source, self.cursor, forward)
        };
        (byte, if forward { Upstream } else { Downstream })
    }

    fn delete_selection(&mut self) -> bool {
        self.stop_pointer();
        let Some(range) = self.selected_range() else {
            self.selection_anchor = None;
            return false;
        };
        self.cursor = range.start;
        self.cursor_affinity = CaretAffinity::Downstream;
        self.source.replace_range(range, "");
        self.selection_anchor = None;
        true
    }

    fn delete(&mut self, backward: bool, word: bool) -> bool {
        if self.delete_selection() {
            return true;
        }
        let target = if word {
            word_boundary(&self.source, self.cursor, !backward)
        } else {
            grapheme_boundary(&self.source, self.cursor, !backward)
        };
        if target == self.cursor {
            return false;
        }
        let range = target.min(self.cursor)..target.max(self.cursor);
        self.cursor = range.start;
        self.cursor_affinity = CaretAffinity::Downstream;
        self.source.replace_range(range, "");
        true
    }

    fn replace_selection(&mut self, text: &str) {
        self.delete_selection();
        self.source.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.cursor_affinity = CaretAffinity::Downstream;
    }

    fn stop_pointer(&mut self) {
        self.dragging = false;
        self.pending_hit = None;
        self.pending_drag_hit = None;
    }

    pub(super) fn end_pointer_drag(&mut self) {
        self.dragging = false;
        self.pending_drag_hit = None;
    }

    fn resolve_pending_pointer(&mut self) {
        if !self.carets.matches(&self.spec()) {
            return;
        }
        if let Some((x, y)) = self.pending_hit.take()
            && let Some(byte) = self.carets.hit_position(x, y)
        {
            self.move_to_position(byte, self.pending_extend);
            if let Some((x, y)) = self.pending_drag_hit.take()
                && let Some(byte) = self.carets.hit_position(x, y)
            {
                self.move_to_position(byte, true);
            }
        }
    }

    fn spec(&self) -> TextSpec {
        TextSpec {
            source: self.source.clone(),
            family: self.family.clone(),
            face: self.face,
            size: self.size,
            color: self.color,
            spacing: self.spacing,
            layout: self.layout,
        }
    }

    fn preview_input(&self) -> TextPreviewInput {
        TextPreviewInput {
            rendered: self.rendered.clone(),
            carets: self.carets.clone(),
            spec: self.spec(),
            opacity: self.opacity,
            layer: self.layer,
            above: self.above,
            anchor: self.anchor,
        }
    }

    fn state(&self) -> TextEdit {
        TextEdit {
            source: self.source.clone(),
            cursor: self.cursor,
            cursor_affinity: self.cursor_affinity,
            selection_anchor: self.selection_anchor,
            family: self.family.clone(),
            face: self.face,
            size: self.size,
            color: self.color,
            spacing: self.spacing,
            layout: self.layout,
            anchor: self.anchor,
        }
    }

    fn restore(&mut self, state: TextEdit) {
        self.source = state.source;
        self.cursor = state.cursor;
        self.cursor_affinity = state.cursor_affinity;
        self.selection_anchor = state.selection_anchor;
        self.family = state.family;
        self.face = state.face;
        self.size = state.size;
        self.color = state.color;
        self.spacing = state.spacing;
        self.layout = state.layout;
        self.anchor = state.anchor;
        self.stop_pointer();
    }

    fn record(&mut self, previous: TextEdit) {
        self.redo.clear();
        if self.undo.len() == 100 {
            self.undo.remove(0);
        }
        self.undo.push(previous);
    }

    fn step_history(&mut self, redo: bool) -> bool {
        let target = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        let Some(target) = target else {
            return false;
        };
        let current = self.state();
        if redo {
            self.undo.push(current);
        } else {
            self.redo.push(current);
        }
        self.restore(target);
        true
    }

    pub(super) fn changed(&self) -> bool {
        if self.layer.is_none() {
            return !self.source.is_empty();
        }
        self.anchor != self.baseline_anchor || !self.baseline.matches(&self.spec())
    }

    fn apply(&self, content: &mut document::DocumentContent, active: &mut LayerId) -> Dirty {
        let rendered = if self.rendered.matches(&self.spec()) {
            self.rendered.as_ref().clone()
        } else {
            match text::render_spec(&self.spec()) {
                Ok(rendered) => rendered,
                Err(_) => return Dirty::None,
            }
        };
        if let Some(id) = self.layer {
            content.set_text(id, self.anchor, rendered)
        } else if let Some(id) = content.insert_text_layer(self.above, self.anchor, rendered) {
            *active = id;
            Dirty::Canvas
        } else {
            Dirty::None
        }
    }

    /// The layout box of the text in canvas pixels: left, top, right, bottom.
    /// Empty text still gets a box one line tall so its frame is visible.
    fn bounds(&self) -> (f32, f32, f32, f32) {
        let (ax, ay) = (self.anchor.0 as f32, self.anchor.1 as f32);
        if let (Some(width), Some(height)) = (self.layout.width, self.layout.height) {
            return (ax, ay, ax + width as f32, ay + height as f32);
        }
        let offset = self.rendered.style.offset_for(self.anchor);
        let (width, height) = self.rendered.image.dimensions();
        let mut left = (offset.0 as f32).min(ax);
        let mut top = (offset.1 as f32).min(ay);
        let mut right = offset.0 as f32 + width as f32;
        let mut bottom = offset.1 as f32 + height as f32;
        for stop in &self.carets.stops {
            left = left.min(ax + stop.x);
            right = right.max(ax + stop.x);
            top = top.min(ay + stop.top);
            bottom = bottom.max(ay + stop.top + self.carets.line_height);
        }
        bottom = bottom.max(ay + self.size * self.spacing.line);
        if self.source.is_empty() {
            right = right.max(ax + self.size * 0.5);
        }
        (left, top, right, bottom)
    }

    // A click within `pad` canvas pixels of the box still edits this text.
    fn contains(&self, pixel: (i64, i64), pad: f32) -> bool {
        let (left, top, right, bottom) = self.bounds();
        let (x, y) = (pixel.0 as f32 + 0.5, pixel.1 as f32 + 0.5);
        x >= left - pad && x <= right + pad && y >= top - pad && y <= bottom + pad
    }

    #[cfg(test)]
    fn hit(&self, pixel: (i64, i64)) -> Option<usize> {
        self.hit_position(pixel).map(|position| position.0)
    }

    fn hit_position(&self, pixel: (i64, i64)) -> Option<(usize, CaretAffinity)> {
        // Preview layout runs in the background. Its byte offsets may refer to
        // the previous source, including the middle of a new UTF-8 character.
        if !self.carets.matches(&self.spec()) {
            return None;
        }
        self.carets.hit_position(
            pixel.0 as f32 + 0.5 - self.anchor.0 as f32,
            pixel.1 as f32 + 0.5 - self.anchor.1 as f32,
        )
    }
}

impl Workspace {
    pub(super) fn extend_text_selection(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(pixel) = self.unclamped_pointer_pixel(position, scale) else {
            return;
        };
        let Some(draft) = self.text_draft.as_mut().filter(|draft| draft.dragging) else {
            return;
        };
        if draft.pending_hit.is_some() {
            draft.pending_drag_hit = Some((
                pixel.0 as f32 + 0.5 - draft.anchor.0 as f32,
                pixel.1 as f32 + 0.5 - draft.anchor.1 as f32,
            ));
        } else if let Some(byte) = draft.hit_position(pixel) {
            draft.move_to_position(byte, true);
        }
        self.caret_shown_at = std::time::Instant::now();
        cx.notify();
    }

    pub(super) fn finish_text_selection(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        self.extend_text_selection(position, scale, cx);
        if let Some(draft) = self.text_draft.as_mut() {
            draft.dragging = false;
        }
    }

    pub(super) fn bind_text_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        let drafting = self.text_draft.is_some();
        let rasterize = self
            .active_tab()
            .is_some_and(|tab| !tab.busy() && tab.document.content.layer_is_text(tab.active_layer));
        root.when(drafting, |root| {
            root.on_action(cx.listener(|this, _: &FinishText, window, cx| {
                this.finish_text(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TextCopy, _, cx| {
                this.copy_text(cx);
            }))
            .on_action(cx.listener(|this, _: &TextCut, window, cx| {
                this.cut_text(window, cx);
            }))
            .on_action(cx.listener(|this, _: &TextPaste, window, cx| {
                this.paste_text(window, cx);
            }))
            .on_action(cx.listener(|this, _: &TextSelectAll, _, cx| {
                this.select_all_text(cx);
            }))
            .on_action(
                cx.listener(|this, _: &Undo, window, cx| this.step_text_history(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Redo, window, cx| this.step_text_history(true, window, cx)),
            )
        })
        .when(rasterize, |root| {
            root.on_action(cx.listener(|this, _: &RasterizeText, window, cx| {
                this.rasterize_text_layer(window, cx);
            }))
        })
    }

    pub(super) fn text_options(&self, panel: Div, cx: &Context<Self>) -> Div {
        let draft = self.text_draft.as_ref();
        let family = draft.map_or(self.preferences.text_defaults.family.clone(), |draft| {
            draft.family.clone()
        });
        let face = draft.map_or(self.preferences.text_defaults.face, |draft| draft.face);
        let color = draft.map_or_else(
            || self.color_memory.swatches[0],
            |draft| {
                let [red, green, blue, _] = draft.color.0;
                u32::from_be_bytes([0, red, green, blue])
            },
        );
        let substituted = draft.and_then(|draft| draft.substitution.clone());
        let layout = self.text_layout();
        let can_rasterize = self
            .active_tab()
            .is_some_and(|tab| !tab.busy() && tab.document.content.layer_is_text(tab.active_layer));
        panel
            .when_some(substituted, |panel, message| {
                panel.child(info_note(message))
            })
            .child(section_label("COLOR"))
            .child(
                div()
                    .id("text-color")
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(CANVAS))
                    .cursor_pointer()
                    .hover(|button| button.bg(rgb(HOVER)))
                    .child(
                        div()
                            .size(px(16.))
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(color)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(TEXT))
                            .child(format!("#{color:06X}")),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        let swatch = if this.text_draft.is_some() {
                            TEXT_SWATCH
                        } else {
                            0
                        };
                        this.color_form(swatch, window, cx);
                    })),
            )
            .child(section_label("FONT"))
            .child(self.font_picker(&family, cx))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        toggle_button("Bold", face.bold)
                            .flex_1()
                            .font_weight(gpui::FontWeight::BOLD)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_text_face(true, window, cx)
                            })),
                    )
                    .child(
                        toggle_button("Italic", face.italic)
                            .flex_1()
                            .italic()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_text_face(false, window, cx)
                            })),
                    ),
            )
            .child(option_row(
                "Size",
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(step_button("text-size-down", "−").on_click(cx.listener(
                        |this, _, window, cx| {
                            let size = this.text_size() - 1.0;
                            this.set_text_size(size, window, cx);
                        },
                    )))
                    .child(self.numeric_chip(NumericField::TextSize, None, cx))
                    .child(step_button("text-size-up", "+").on_click(cx.listener(
                        |this, _, window, cx| {
                            let size = this.text_size() + 1.0;
                            this.set_text_size(size, window, cx);
                        },
                    ))),
            ))
            .child(
                self.tool_option_section(
                    ToolSection::TextSpacing,
                    "SPACING",
                    format!(
                        "{} px · {}%",
                        self.numeric_value(NumericField::TextLetterSpacing)
                            .unwrap_or(0),
                        self.numeric_value(NumericField::TextLineSpacing)
                            .unwrap_or(100)
                    ),
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(option_row(
                            "Letter px",
                            self.numeric_stepper(
                                NumericField::TextLetterSpacing,
                                step_button("text-letter-down", "−").on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.step_text_spacing(-1.0, 0.0, window, cx)
                                    },
                                )),
                                step_button("text-letter-up", "+").on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.step_text_spacing(1.0, 0.0, window, cx)
                                    },
                                )),
                                cx,
                            ),
                        ))
                        .child(option_row(
                            "Line %",
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(step_button("text-line-down", "−").on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.step_text_spacing(0.0, -0.1, window, cx)
                                    },
                                )))
                                .child(self.numeric_chip(NumericField::TextLineSpacing, None, cx))
                                .child(step_button("text-line-up", "+").on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.step_text_spacing(0.0, 0.1, window, cx)
                                    },
                                ))),
                        )),
                    cx,
                ),
            )
            .child(
                self.tool_option_section(
                    ToolSection::TextLayout,
                    "LAYOUT",
                    layout.width.map_or_else(
                        || "Auto".to_owned(),
                        |width| format!("{} × {}", width, layout.height.unwrap_or(0)),
                    ),
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(toggle_button("Fixed box", layout.width.is_some()).on_click(
                            cx.listener(|this, _, window, cx| {
                                let mut layout = this.text_layout();
                                if layout.width.is_some() {
                                    layout.width = None;
                                    layout.height = None;
                                } else {
                                    layout.width = Some(256);
                                    layout.height = Some(128);
                                }
                                this.set_text_layout(layout, window, cx);
                            }),
                        ))
                        .when(layout.width.is_some(), |panel| {
                            panel
                                .child(option_row(
                                    "Width",
                                    self.numeric_chip(NumericField::TextBoxWidth, None, cx),
                                ))
                                .child(option_row(
                                    "Height",
                                    self.numeric_chip(NumericField::TextBoxHeight, None, cx),
                                ))
                                .child(segmented(
                                    [
                                        (TextAlign::Left, "Left"),
                                        (TextAlign::Center, "Center"),
                                        (TextAlign::Right, "Right"),
                                    ]
                                    .into_iter()
                                    .map(|(align, label)| {
                                        choice_button(label, layout.align == align).on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                let mut layout = this.text_layout();
                                                layout.align = align;
                                                this.set_text_layout(layout, window, cx);
                                            }),
                                        )
                                    }),
                                ))
                        }),
                    cx,
                ),
            )
            .when(
                draft.is_some_and(|draft| {
                    draft.layout.height.is_some_and(|height| {
                        draft
                            .carets
                            .stops
                            .last()
                            .is_some_and(|stop| stop.top + draft.carets.line_height > height as f32)
                    })
                }),
                |panel| {
                    panel.child(info_note(
                        "Text extends below the box. Increase its height to show more.",
                    ))
                },
            )
            .child(section_label("LAYER"))
            .child(
                option_action("Rasterize Text", false)
                    .w_full()
                    .when(!can_rasterize, |button| {
                        button.opacity(0.45).cursor(CursorStyle::Arrow)
                    })
                    .when(can_rasterize, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.rasterize_text_layer(window, cx);
                        }))
                    }),
            )
    }

    fn font_entries(&self) -> Vec<(String, String, String)> {
        let mut entries: Vec<_> = GENERIC_FAMILIES
            .iter()
            .enumerate()
            .map(|(index, (label, name))| {
                let preview = self.font_families.as_ref().map_or_else(
                    || (*name).to_owned(),
                    |families| families.generic_previews[index].clone(),
                );
                ((*label).to_owned(), (*name).to_owned(), preview)
            })
            .collect();
        if let Some(families) = &self.font_families {
            entries.extend(
                families
                    .names
                    .iter()
                    .map(|name| (name.clone(), name.clone(), name.clone())),
            );
        }
        entries.retain(|(label, name, _)| matches_font(label, name, self.font_search.field.text()));
        entries
    }

    fn open_font_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_numeric_edit(window, cx);
        self.font_search = FontSearch::default();
        self.font_list_open = true;
        self.load_font_families(window, cx);
        self.reset_font_match();
        self.font_search_focus.focus(window);
        cx.notify();
    }

    pub(super) fn close_font_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.font_list_open {
            self.font_list_open = false;
            self.focus.focus(window);
            cx.notify();
        }
    }

    fn reset_font_match(&mut self) {
        let family = self
            .text_draft
            .as_ref()
            .map_or(self.preferences.text_defaults.family.as_str(), |draft| {
                draft.family.as_str()
            });
        self.font_search.selected = self
            .font_entries()
            .iter()
            .position(|(_, name, _)| name.eq_ignore_ascii_case(family))
            .unwrap_or(0);
        self.font_scroll.scroll_to_item(self.font_search.selected);
    }

    fn font_search_clipboard(&mut self, key: &str, cx: &mut Context<Self>) {
        if self
            .font_search
            .edit(|field| field.clipboard(key, FieldMode::FontQuery, cx))
        {
            self.font_scroll.scroll_to_top_of_item(0);
        }
        self.font_input_scroll
            .scroll_to_item(self.font_search.field.caret_item());
        cx.notify();
    }

    fn font_search_history(&mut self, redo: bool, cx: &mut Context<Self>) {
        if self.font_search.step_history(redo) {
            self.font_scroll.scroll_to_top_of_item(0);
            self.font_input_scroll
                .scroll_to_item(self.font_search.field.caret_item());
        }
        cx.notify();
    }

    pub(super) fn font_search_key(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // These bindings also exist for canvas text. Keep them in the query
        // even though the single-line Field does not implement word movement.
        if font_query_navigation_key(key) {
            return true;
        }
        if cfg!(target_os = "windows") && key.modifiers.alt {
            self.close_font_picker(window, cx);
            return false;
        }
        if key.modifiers.secondary() {
            match key.key.as_str() {
                "a" | "c" | "x" | "v" => self.font_search_clipboard(&key.key, cx),
                "z" => self.font_search_history(key.modifiers.shift, cx),
                "y" => self.font_search_history(true, cx),
                _ => {
                    self.close_font_picker(window, cx);
                    return false;
                }
            }
            return true;
        }
        if key.modifiers.control || key.modifiers.platform {
            self.close_font_picker(window, cx);
            return false;
        }
        match key.key.as_str() {
            "escape" | "tab" => self.close_font_picker(window, cx),
            "enter" => {
                if let Some((_, name, _)) =
                    self.font_entries().get(self.font_search.selected).cloned()
                {
                    self.close_font_picker(window, cx);
                    self.set_text_family(name, window, cx);
                }
            }
            "up" | "down" => {
                let last = self.font_entries().len().saturating_sub(1);
                self.font_search.selected = if key.key == "down" {
                    (self.font_search.selected + 1).min(last)
                } else {
                    self.font_search.selected.saturating_sub(1)
                };
                self.font_scroll.scroll_to_item(self.font_search.selected);
                cx.notify();
            }
            _ => {
                if self
                    .font_search
                    .edit(|field| field.handle_keystroke(key, FieldMode::FontQuery, cx))
                {
                    self.font_scroll.scroll_to_top_of_item(0);
                }
                self.font_input_scroll
                    .scroll_to_item(self.font_search.field.caret_item());
                cx.notify();
            }
        }
        true
    }

    fn font_picker(&self, family: &str, cx: &Context<Self>) -> Div {
        let label = GENERIC_FAMILIES
            .iter()
            .find(|(_, name)| *name == family)
            .map_or_else(|| family.to_owned(), |(label, _)| (*label).to_owned());
        let open = self.font_list_open;
        let button = div()
            .id("text-font")
            .h(px(28.))
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(if open { ACCENT } else { BORDER }))
            .bg(rgb(CANVAS))
            .cursor_pointer()
            .when(open, |button| {
                button
                    .track_focus(&self.font_search_focus)
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.font_search.field.select_all();
                            this.font_input_scroll
                                .scroll_to_item(this.font_search.field.caret_item());
                            this.font_search_focus.focus(window);
                            cx.notify();
                        }),
                    )
                    .on_action(cx.listener(|this, _: &CopyImage, _, cx| {
                        this.font_search_clipboard("c", cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &CutImage, _, cx| {
                        this.font_search_clipboard("x", cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &PasteImage, _, cx| {
                        this.font_search_clipboard("v", cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                        this.font_search_clipboard("a", cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &Undo, _, cx| {
                        this.font_search_history(false, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &Redo, _, cx| {
                        this.font_search_history(true, cx);
                        cx.stop_propagation();
                    }))
            })
            .hover(|button| button.bg(rgb(HOVER)))
            .when_else(
                open,
                |button| {
                    button.child(
                        self.font_search
                            .field
                            .view(true)
                            .id("font-query-input")
                            .h(px(26.))
                            .px_0()
                            .border_0()
                            .flex_1()
                            .min_w_0()
                            .overflow_x_scroll()
                            .track_scroll(&self.font_input_scroll)
                            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                            .whitespace_nowrap()
                            .text_size(px(12.))
                            .when(self.font_search.field.text().is_empty(), |field| {
                                field.child(div().text_color(rgb(MUTED)).child("Filter fonts…"))
                            }),
                    )
                },
                |button| {
                    button.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_ellipsis()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(label),
                    )
                },
            )
            .child(
                div()
                    .id("font-picker-toggle")
                    .cursor_pointer()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        if this.font_list_open {
                            this.close_font_picker(window, cx);
                        } else {
                            this.open_font_picker(window, cx);
                        }
                    }))
                    .child(if open { "▲" } else { "▼" }),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                if this.font_list_open {
                    this.font_search_focus.focus(window);
                } else {
                    this.open_font_picker(window, cx);
                }
            }));
        let mut picker = div().flex().flex_col().gap_1().child(button);
        if !open {
            return picker;
        }
        let entries = self.font_entries();
        let no_matches = entries.is_empty();
        let loading = self.font_families.is_none();
        let current = family.to_owned();
        picker =
            picker.child(
                div()
                    .id("text-font-list")
                    .h(px(220.))
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.font_scroll)
                    // GPUI scrolls this element before this bubble listener runs.
                    // Consume the event here so the enclosing inspector stays put,
                    // including when this list reaches either end.
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .flex()
                    .flex_col()
                    .p_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(CANVAS))
                    .children(entries.into_iter().enumerate().map(
                        |(index, (label, name, preview))| {
                            let chosen = name.eq_ignore_ascii_case(&current);
                            let highlighted = index == self.font_search.selected;
                            div()
                                .id(("text-font-entry", index))
                                .h(px(32.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .rounded_sm()
                                .text_size(px(12.))
                                .text_ellipsis()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_color(rgb(if chosen {
                                    ACCENT
                                } else if highlighted {
                                    TEXT
                                } else {
                                    MUTED
                                }))
                                .when(highlighted, |entry| entry.bg(rgb(SELECTED)))
                                .cursor_pointer()
                                .hover(|entry| entry.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                                .child(
                                    div()
                                        .w(px(35.))
                                        .flex_shrink_0()
                                        .overflow_hidden()
                                        .font_family(preview)
                                        .text_size(px(20.))
                                        .child("Aa"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(label),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.close_font_picker(window, cx);
                                    this.set_text_family(name.clone(), window, cx);
                                }))
                        },
                    ))
                    .when(loading, |list| {
                        list.child(
                            div()
                                .flex_shrink_0()
                                .px_2()
                                .text_size(px(11.))
                                .text_color(rgb(MUTED))
                                .child("Loading installed fonts…"),
                        )
                    })
                    .when(no_matches && !loading, |list| {
                        list.child(
                            div()
                                .px_2()
                                .text_size(px(12.))
                                .text_color(rgb(MUTED))
                                .child("No matching fonts"),
                        )
                    }),
            );
        picker
    }

    fn load_font_families(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.font_families.is_some() || self.font_loading {
            return;
        }
        self.font_loading = true;
        let job = cx.background_executor().spawn(async { text::families() });
        cx.spawn_in(window, async move |workspace, cx| {
            let families = job.await;
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.font_loading = false;
                workspace.font_families = Some(Arc::new(families));
                if workspace.font_list_open && workspace.font_search.field.text().trim().is_empty()
                {
                    workspace.reset_font_match();
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn restore_text_recovery(&mut self, id: usize, value: serde_json::Value) -> bool {
        #[derive(serde::Deserialize)]
        struct Draft {
            source: String,
            family: String,
            size: f32,
            bold: bool,
            italic: bool,
            color: [u8; 4],
            letter_spacing: f32,
            line_spacing: f32,
            layout: TextLayout,
            anchor: (i32, i32),
            layer: Option<u64>,
            #[serde(default)]
            above: u64,
        }
        let Ok(saved) = serde_json::from_value::<Draft>(value) else {
            return false;
        };
        let spacing = Spacing {
            letter: saved.letter_spacing,
            line: saved.line_spacing,
        };
        if saved.source.chars().count() > MAX_TEXT_CHARS
            || saved.family.chars().count() > text::MAX_FAMILY_CHARS
            || !saved.size.is_finite()
            || !(MIN_PIXEL_SIZE..=MAX_PIXEL_SIZE).contains(&saved.size)
            || !spacing.allowed()
            || !saved.layout.allowed()
        {
            return false;
        }
        let Some(index) = self.tab_index(id) else {
            return false;
        };
        let tab = &self.tabs[index];
        let layer = saved.layer.map(LayerId);
        let (baseline, opacity, baseline_anchor) = if let Some(layer) = layer {
            let Some(layer) = tab.document.content.layer(layer) else {
                return false;
            };
            let Some(style) = layer.text.clone() else {
                return false;
            };
            let anchor = style.anchor(layer.offset);
            (
                text::RenderedText {
                    image: layer.pixels.clone(),
                    style,
                },
                layer.opacity,
                anchor,
            )
        } else {
            (
                text::RenderedText {
                    image: Arc::new(image::RgbaImage::new(1, 1)),
                    style: text::TextStyle {
                        source: String::new(),
                        family: saved.family.clone(),
                        face: FontFace {
                            bold: saved.bold,
                            italic: saved.italic,
                        },
                        size: saved.size,
                        color: image::Rgba(saved.color),
                        spacing,
                        layout: saved.layout,
                        resolved_family: saved.family.clone(),
                        origin: (0, 0),
                    },
                },
                255,
                saved.anchor,
            )
        };
        let baseline = Arc::new(baseline);
        self.tabs[index].pending_text = Some(TextDraft {
            tab: id,
            generation: tab.history.generation(),
            above: tab
                .document
                .content
                .layer(LayerId(saved.above))
                .map_or(tab.active_layer, |layer| layer.id),
            opacity,
            substitution: None,
            layer,
            anchor: saved.anchor,
            cursor: saved.source.len(),
            cursor_affinity: CaretAffinity::Downstream,
            source: saved.source,
            selection_anchor: None,
            family: saved.family,
            face: FontFace {
                bold: saved.bold,
                italic: saved.italic,
            },
            size: saved.size,
            color: image::Rgba(saved.color),
            spacing,
            layout: saved.layout,
            rendered: baseline.clone(),
            baseline,
            baseline_anchor,
            carets: Arc::new(CaretMap::default()),
            pending_hit: None,
            pending_extend: false,
            pending_drag_hit: None,
            dragging: false,
            undo: Vec::new(),
            redo: Vec::new(),
        });
        true
    }

    pub(super) fn text_preview_layer(&self) -> Option<Option<LayerId>> {
        self.text_draft.as_ref().map(|draft| draft.layer)
    }

    pub(super) fn text_layout(&self) -> TextLayout {
        self.text_draft
            .as_ref()
            .map_or(self.preferences.text_defaults.layout, |draft| draft.layout)
    }

    fn set_text_layout(&mut self, layout: TextLayout, window: &mut Window, cx: &mut Context<Self>) {
        if !layout.allowed() {
            return;
        }
        self.preferences.text_defaults.layout = layout;
        self.change_text(window, cx, |draft| {
            if draft.layout == layout {
                return false;
            }
            draft.layout = layout;
            true
        });
        cx.notify();
    }

    pub(super) fn set_text_box_edge(
        &mut self,
        width: bool,
        value: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut layout = self.text_layout();
        if layout.width.is_none() || !(1..=document::MAX_NEW_EDGE).contains(&value) {
            return;
        }
        if width {
            layout.width = Some(value);
        } else {
            layout.height = Some(value);
        }
        self.set_text_layout(layout, window, cx);
    }

    pub(super) fn text_size(&self) -> f32 {
        self.text_draft
            .as_ref()
            .map_or(self.preferences.text_defaults.size, |draft| draft.size)
    }

    pub(super) fn text_line_spacing(&self) -> f32 {
        self.text_draft
            .as_ref()
            .map_or(self.preferences.text_defaults.spacing.line, |draft| {
                draft.spacing.line
            })
    }

    pub(super) fn text_letter_spacing(&self) -> f32 {
        self.text_draft
            .as_ref()
            .map_or(self.preferences.text_defaults.spacing.letter, |draft| {
                draft.spacing.letter
            })
    }

    pub(super) fn set_text_letter_spacing(
        &mut self,
        letter: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let letter = letter.round().clamp(MIN_LETTER_SPACING, MAX_LETTER_SPACING);
        let line = self.text_line_spacing();
        self.set_text_spacing(Spacing { letter, line }, window, cx);
    }

    pub(super) fn set_text_size(&mut self, size: f32, window: &mut Window, cx: &mut Context<Self>) {
        let size = size.round().clamp(MIN_PIXEL_SIZE, MAX_PIXEL_SIZE);
        self.preferences.text_defaults.size = size;
        self.change_text(window, cx, |draft| {
            if draft.size == size {
                return false;
            }
            draft.size = size;
            true
        });
    }

    pub(super) fn set_text_line_spacing(
        &mut self,
        line: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line = ((line * 100.0).round() / 100.0).clamp(MIN_LINE_SPACING, MAX_LINE_SPACING);
        let letter = self
            .text_draft
            .as_ref()
            .map_or(self.preferences.text_defaults.spacing.letter, |draft| {
                draft.spacing.letter
            });
        self.set_text_spacing(Spacing { letter, line }, window, cx);
    }

    fn step_text_spacing(
        &mut self,
        letter: f32,
        line: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .text_draft
            .as_ref()
            .map_or(self.preferences.text_defaults.spacing, |draft| {
                draft.spacing
            });
        let next = Spacing {
            letter: (current.letter + letter)
                .round()
                .clamp(MIN_LETTER_SPACING, MAX_LETTER_SPACING),
            line: (((current.line + line) * 100.0).round() / 100.0)
                .clamp(MIN_LINE_SPACING, MAX_LINE_SPACING),
        };
        self.set_text_spacing(next, window, cx);
    }

    fn set_text_spacing(&mut self, spacing: Spacing, window: &mut Window, cx: &mut Context<Self>) {
        self.preferences.text_defaults.spacing = spacing;
        self.change_text(window, cx, |draft| {
            if draft.spacing == spacing {
                return false;
            }
            draft.spacing = spacing;
            true
        });
    }

    fn set_text_family(&mut self, family: String, window: &mut Window, cx: &mut Context<Self>) {
        let family = text::normalize_family(&family);
        self.preferences.text_defaults.family = family.clone();
        self.change_text(window, cx, |draft| {
            if draft.family == family {
                return false;
            }
            draft.family = family;
            true
        });
    }

    fn toggle_text_face(&mut self, bold: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut face = self
            .text_draft
            .as_ref()
            .map_or(self.preferences.text_defaults.face, |draft| draft.face);
        if bold {
            face.bold = !face.bold;
        } else {
            face.italic = !face.italic;
        }
        self.preferences.text_defaults.face = face;
        self.change_text(window, cx, |draft| {
            draft.face = face;
            true
        });
    }

    pub(super) fn recolor_text(&mut self, color: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(color) = self.active_tab().map(|tab| {
            tab.document
                .content
                .mode()
                .constrain(document::solid_color(color))
        }) else {
            return;
        };
        self.change_text(window, cx, |draft| {
            if draft.color == color {
                return false;
            }
            draft.color = color;
            true
        });
    }

    pub(super) fn text_color_value(&self) -> Option<u32> {
        self.text_draft.as_ref().map(|draft| {
            let [red, green, blue, _] = draft.color.0;
            u32::from_be_bytes([0, red, green, blue])
        })
    }

    /// The caret and any selected text, in canvas pixels, while typing.
    pub(super) fn text_caret(&self) -> Option<TextCaret> {
        let draft = self.text_draft.as_ref()?;
        if self.active_tab().is_none_or(|tab| tab.id != draft.tab) {
            return None;
        }
        let (ax, ay) = (draft.anchor.0 as f32, draft.anchor.1 as f32);
        let line_height = if draft.carets.line_height > 0.0 {
            draft.carets.line_height
        } else {
            draft.size * draft.spacing.line
        };
        let stop = draft
            .carets
            .at_affinity(draft.cursor, draft.cursor_affinity);
        let (x, top) = stop.map_or((0.0, 0.0), |stop| (stop.x, stop.top));
        let selection = if draft.carets.matches(&draft.spec()) {
            draft
                .selected_range()
                .map(|range| {
                    draft
                        .carets
                        .selection_rects(range)
                        .into_iter()
                        .filter_map(|(x, y, w, h)| {
                            let (left, top) = (x.max(0.), y.max(0.));
                            let right = draft
                                .layout
                                .width
                                .map_or(x + w, |width| (x + w).min(width as f32));
                            let bottom = draft
                                .layout
                                .height
                                .map_or(y + h, |height| (y + h).min(height as f32));
                            (right > left && bottom > top).then_some((
                                ax + left,
                                ay + top,
                                right - left,
                                bottom - top,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let phase = self.caret_shown_at.elapsed().as_millis() / CARET_BLINK_MS;
        Some(TextCaret {
            x: ax + x,
            top: ay + top,
            height: draft.layout.height.map_or(line_height, |height| {
                line_height.min((height as f32 - top).max(0.))
            }),
            selection,
            frame: draft.bounds(),
            visible: phase.is_multiple_of(2)
                && x >= 0.
                && top >= 0.
                && draft.layout.width.is_none_or(|width| x <= width as f32)
                && draft.layout.height.is_none_or(|height| top < height as f32),
        })
    }

    pub(super) fn place_text(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        scale: f32,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pixel) = self.unclamped_pointer_pixel(position, scale) else {
            return;
        };
        // Clicks this many screen points from a text box still edit that text.
        const SLOP: f32 = 12.0;
        let pad = self
            .active_tab()
            .and_then(|tab| {
                let geometry =
                    super::canvas_tools::PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
                let per_pixel = f32::from(geometry.bounds.size.width) / tab.document.width() as f32;
                (per_pixel > 0.0).then(|| SLOP / per_pixel)
            })
            .unwrap_or(SLOP)
            .max(2.0);
        self.place_text_at(pixel, pad, extend, window, cx);
        if let Some(draft) = self.text_draft.as_mut() {
            draft.dragging = true;
        }
    }

    fn place_text_at(
        &mut self,
        pixel: (i64, i64),
        pad: f32,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        self.close_font_picker(window, cx);
        // A click inside the text being typed moves the caret.
        if let Some(draft) = self.text_draft.as_mut()
            && draft.contains(pixel, pad)
        {
            if let Some(byte) = draft.hit_position(pixel) {
                draft.stop_pointer();
                draft.move_to_position(byte, extend);
            } else {
                draft.pending_hit = Some((
                    pixel.0 as f32 + 0.5 - draft.anchor.0 as f32,
                    pixel.1 as f32 + 0.5 - draft.anchor.1 as f32,
                ));
                draft.pending_extend = extend;
                draft.pending_drag_hit = None;
            }
            self.caret_shown_at = std::time::Instant::now();
            cx.notify();
            return;
        }
        // Anywhere else finishes that text first.
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| {
                this.place_text_at(pixel, pad, extend, window, cx)
            });
            return;
        }
        if let Some((id, offset, style, pixels, opacity)) = self.text_layer_at(pixel, pad) {
            if let Some(index) = self.active {
                self.tabs[index].active_layer = id;
            }
            let anchor = style.anchor(offset);
            self.open_draft(
                Some(id),
                anchor,
                text::RenderedText {
                    image: pixels,
                    style,
                },
                opacity,
                Some((
                    pixel.0 as f32 + 0.5 - anchor.0 as f32,
                    pixel.1 as f32 + 0.5 - anchor.1 as f32,
                )),
                window,
                cx,
            );
            return;
        }
        let anchor = (clamp_i32(pixel.0), clamp_i32(pixel.1));
        let color = self
            .active_tab()
            .map(|tab| {
                tab.document
                    .content
                    .mode()
                    .constrain(document::solid_color(self.color_memory.swatches[0]))
            })
            .unwrap_or(document::solid_color(self.color_memory.swatches[0]));
        let defaults = self.preferences.text_defaults.clone();
        self.open_draft(
            None,
            anchor,
            text::RenderedText {
                image: Arc::new(image::RgbaImage::new(1, 1)),
                style: text::TextStyle {
                    source: String::new(),
                    family: defaults.family.clone(),
                    face: defaults.face,
                    size: defaults.size,
                    color,
                    spacing: defaults.spacing,
                    layout: defaults.layout,
                    resolved_family: defaults.family,
                    origin: (0, 0),
                },
            },
            255,
            None,
            window,
            cx,
        );
    }

    // The topmost visible text layer under the pointer.
    #[allow(clippy::type_complexity)]
    fn text_layer_at(
        &self,
        pixel: (i64, i64),
        pad: f32,
    ) -> Option<(
        LayerId,
        (i32, i32),
        text::TextStyle,
        Arc<image::RgbaImage>,
        u8,
    )> {
        let tab = self.active_tab()?;
        // Distance from the pointer to each visible text layer's box, topmost first.
        // A click inside a box wins; otherwise the nearest box within `pad`.
        let (x, y) = (pixel.0 as f32 + 0.5, pixel.1 as f32 + 0.5);
        let layer = tab
            .document
            .content
            .layers()
            .iter()
            .rev()
            .filter(|layer| layer.visible && layer.text.is_some())
            .map(|layer| {
                let (width, height) = layer.pixels.dimensions();
                let (left, top) = (layer.offset.0 as f32, layer.offset.1 as f32);
                let dx = (left - x).max(x - (left + width as f32)).max(0.0);
                let dy = (top - y).max(y - (top + height as f32)).max(0.0);
                (layer, dx.max(dy))
            })
            .filter(|(_, distance)| *distance <= pad)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(layer, _)| layer)?;
        Some((
            layer.id,
            layer.offset,
            layer.text.clone()?,
            layer.pixels.clone(),
            layer.opacity,
        ))
    }

    pub(super) fn type_text(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.text_draft.is_none() {
            return;
        }
        let key = &event.keystroke;
        let word = if cfg!(target_os = "macos") {
            key.modifiers.alt && !key.modifiers.platform && !key.modifiers.control
        } else {
            key.modifiers.control && !key.modifiers.alt && !key.modifiers.platform
        };
        let command_navigation = cfg!(target_os = "macos")
            && key.modifiers.platform
            && !key.modifiers.control
            && !key.modifiers.alt
            && matches!(key.key.as_str(), "left" | "right" | "up" | "down");
        let editing_key = matches!(
            key.key.as_str(),
            "left" | "right" | "up" | "down" | "home" | "end" | "backspace" | "delete"
        );
        if text_command_key(key, cfg!(target_os = "windows"))
            && !command_navigation
            && !(word && editing_key)
        {
            return;
        }
        match key.key.as_str() {
            "backspace" | "delete" => self.change_text(window, cx, |draft| {
                draft.delete(key.key == "backspace", word)
            }),
            "left" | "right" | "home" | "end" | "up" | "down" => {
                self.move_caret(key.modifiers.shift, cx, |draft| {
                    draft.navigation_position(
                        &key.key,
                        word,
                        command_navigation,
                        key.modifiers.shift,
                    )
                });
            }
            "enter" => self.insert_text("\n", window, cx),
            "escape" | "tab" => {}
            _ => {
                if let Some(text) = typed_text(key, cfg!(target_os = "windows")) {
                    self.insert_text(text, window, cx);
                }
            }
        }
    }

    fn move_caret(
        &mut self,
        extend: bool,
        cx: &mut Context<Self>,
        target: impl FnOnce(&TextDraft) -> (usize, CaretAffinity),
    ) {
        self.caret_shown_at = std::time::Instant::now();
        if let Some(draft) = self.text_draft.as_mut() {
            let byte = target(draft);
            draft.stop_pointer();
            draft.move_to_position(byte, extend);
        }
        cx.notify();
    }

    /// Ends the text session. A modified draft is written to its layer as one
    /// undo step; an unchanged or empty one just closes. Returns true when the
    /// commit job started, so the tab is busy until it lands.
    pub(super) fn finish_text(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(draft) = self.text_draft.as_ref() else {
            return false;
        };
        self.preferences.text_defaults = TextDefaults {
            family: draft.family.clone(),
            face: draft.face,
            size: draft.size,
            spacing: draft.spacing,
            layout: draft.layout,
        };
        let Some(tab) = self.active_tab() else {
            self.cancel_text(window, cx);
            return false;
        };
        if tab.busy() {
            return false;
        }
        let current = tab.id == draft.tab && tab.history.generation() == draft.generation;
        if !current || !draft.changed() || (draft.layer.is_none() && draft.source.trim().is_empty())
        {
            self.cancel_text(window, cx);
            return false;
        }
        let layer_hidden = draft.layer.is_some_and(|id| {
            tab.document
                .content
                .layer(id)
                .is_none_or(|layer| !layer.visible)
        });
        if layer_hidden {
            self.cancel_text(window, cx);
            self.notice("The text layer is hidden, so the edit was not kept.", cx);
            return false;
        }
        let Some(draft) = self.text_draft.take() else {
            return false;
        };
        if let Some(index) = self.active {
            self.tabs[index].pending_text = Some(draft.clone());
        }
        let id = draft.tab;
        // The preview stays on the canvas until the committed pixels replace it.
        self.text_settled = self.text_overlay.is_some().then_some(id);
        self.edit_content(
            move |content, active, _| draft.apply(content, active),
            window,
            cx,
        );
        let started = self.active_tab().is_some_and(|tab| tab.busy());
        // Any preview still rendering belongs to the closed draft.
        self.text_generation = self.text_generation.wrapping_add(1);
        self.text_dirty = false;
        self.text_error = None;
        if !started {
            self.drop_settled_text(window, cx);
            self.recover_pending_text(window, cx);
        }
        cx.notify();
        started
    }

    pub(super) fn recover_pending_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.active else {
            return;
        };
        if self.tabs[index].busy() || self.text_draft.is_some() {
            return;
        }
        let Some(mut draft) = self.tabs[index].pending_text.take() else {
            return;
        };
        draft.generation = self.tabs[index].history.generation();
        self.preferences.tool = crate::commands::Tool::Text;
        self.text_draft = Some(draft);
        self.caret_shown_at = std::time::Instant::now();
        self.start_caret_blink(window, cx);
        self.request_text_preview(window, cx);
    }

    /// Runs `then` after the text commit that `finish_text` just started.
    pub(super) fn defer_after_text(
        &mut self,
        then: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    ) {
        if let Some(id) = self.active_tab().map(|tab| tab.id) {
            self.text_continuation = Some((id, Box::new(then)));
        }
    }

    pub(super) fn drop_settled_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_settled.take().is_some()
            && self.text_draft.is_none()
            && let Some(overlay) = self.text_overlay.take()
        {
            super::drop_images(overlay.into_images(), window, cx);
        }
    }

    /// Closes the draft without writing it.
    pub(super) fn cancel_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_draft.is_none() && (self.text_overlay.is_none() || self.text_settled.is_some())
        {
            return;
        }
        self.text_generation = self.text_generation.wrapping_add(1);
        self.text_dirty = false;
        self.text_error = None;
        self.text_draft = None;
        if self.text_settled.is_none()
            && let Some(overlay) = self.text_overlay.take()
        {
            super::drop_images(overlay.into_images(), window, cx);
        }
        cx.notify();
    }

    pub(super) fn rasterize_text_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finish_text(window, cx) {
            self.defer_after_text(|this, window, cx| this.rasterize_text_layer(window, cx));
            return;
        }
        self.cancel_text(window, cx);
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let Some(id) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        if !self
            .active_tab()
            .is_some_and(|tab| tab.document.content.layer_is_text(id))
        {
            self.notice("The active layer is not text.", cx);
            return;
        }
        self.edit_content(
            move |content, _, _| {
                if content.rasterize_text(id) {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn refuse_text_pixels(&mut self, image_wide: bool, cx: &mut Context<Self>) -> bool {
        let blocked = self.active_tab().is_some_and(|tab| {
            if image_wide {
                tab.document.content.has_text_layer()
            } else {
                tab.document.content.layer_is_text(tab.active_layer)
            }
        });
        if blocked {
            self.notice("Rasterize the text layer before using pixel tools.", cx);
        }
        blocked
    }

    pub(super) fn has_pending_text(&self) -> bool {
        self.text_draft.as_ref().is_some_and(TextDraft::changed)
    }

    fn step_text_history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .text_draft
            .as_mut()
            .is_some_and(|draft| draft.step_history(redo))
        {
            self.request_text_preview(window, cx);
            return;
        }
        // Past the typing history, Undo and Redo act on the document.
        if self.finish_text(window, cx) {
            return;
        }
        let change = if redo {
            super::Change::Redo
        } else {
            super::Change::Undo
        };
        self.step_history(change, window, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn open_draft(
        &mut self,
        layer: Option<LayerId>,
        anchor: (i32, i32),
        rendered: text::RenderedText,
        opacity: u8,
        pending_hit: Option<(f32, f32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let rendered = Arc::new(rendered);
        self.text_draft = Some(TextDraft {
            tab: tab.id,
            generation: tab.history.generation(),
            above: tab.active_layer,
            opacity,
            substitution: None,
            layer,
            anchor,
            source: rendered.style.source.clone(),
            cursor: rendered.style.source.len(),
            cursor_affinity: CaretAffinity::Downstream,
            selection_anchor: None,
            family: rendered.style.family.clone(),
            face: rendered.style.face,
            size: rendered.style.size,
            color: rendered.style.color,
            spacing: rendered.style.spacing,
            layout: rendered.style.layout,
            baseline: rendered.clone(),
            baseline_anchor: anchor,
            rendered,
            carets: Arc::new(CaretMap::default()),
            pending_hit,
            pending_extend: false,
            pending_drag_hit: None,
            dragging: false,
            undo: Vec::new(),
            redo: Vec::new(),
        });
        self.caret_shown_at = std::time::Instant::now();
        self.start_caret_blink(window, cx);
        self.request_text_preview(window, cx);
    }

    // Repaints at each caret phase change while a draft is open.
    fn start_caret_blink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.caret_blinking {
            return;
        }
        self.caret_blinking = true;
        cx.spawn_in(window, async move |workspace, cx| {
            loop {
                let wait = workspace
                    .update(cx, |workspace, _| {
                        let elapsed = workspace.caret_shown_at.elapsed().as_millis();
                        (CARET_BLINK_MS - elapsed % CARET_BLINK_MS) as u64
                    })
                    .unwrap_or(CARET_BLINK_MS as u64);
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(wait + 1))
                    .await;
                let open = workspace
                    .update(cx, |workspace, cx| {
                        if workspace.text_draft.is_none() {
                            workspace.caret_blinking = false;
                            return false;
                        }
                        if workspace.preferences.tool == crate::commands::Tool::Text {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !open {
                    break;
                }
            }
        })
        .detach();
    }

    fn change_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        mutate: impl FnOnce(&mut TextDraft) -> bool,
    ) {
        self.caret_shown_at = std::time::Instant::now();
        let changed = self.text_draft.as_mut().is_some_and(|draft| {
            let previous = draft.state();
            if !mutate(draft) {
                return false;
            }
            draft.stop_pointer();
            draft.record(previous);
            true
        });
        if changed {
            self.request_text_preview(window, cx);
        }
        cx.notify();
    }

    fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let cleaned = clean_text(text);
        if cleaned.is_empty() {
            return;
        }
        let Some(draft) = self.text_draft.as_ref() else {
            return;
        };
        let existing = draft.source.chars().count()
            - draft
                .selected_range()
                .map_or(0, |range| draft.source[range].chars().count());
        if existing + cleaned.chars().count() > MAX_TEXT_CHARS {
            self.notice(TextError::TooLong.message(), cx);
            return;
        }
        self.change_text(window, cx, |draft| {
            draft.replace_selection(&cleaned);
            true
        });
    }

    pub(super) fn copy_text(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.text_draft.as_ref() else {
            return;
        };
        if let Some(range) = draft.selected_range() {
            cx.write_to_clipboard(ClipboardItem::new_string(draft.source[range].to_owned()));
        }
    }

    pub(super) fn cut_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_text(cx);
        if self
            .text_draft
            .as_ref()
            .is_some_and(|draft| draft.selected_range().is_some())
        {
            self.change_text(window, cx, |draft| draft.delete_selection());
        }
    }

    pub(super) fn paste_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.insert_text(&text, window, cx);
    }

    pub(super) fn select_all_text(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.text_draft.as_mut()
            && !draft.source.is_empty()
        {
            draft.stop_pointer();
            draft.selection_anchor = Some(0);
            draft.cursor = draft.source.len();
            draft.cursor_affinity = CaretAffinity::Downstream;
            cx.notify();
        }
    }

    fn request_text_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.text_generation = self.text_generation.wrapping_add(1);
        self.text_dirty = true;
        self.text_error = None;
        self.pump_text_preview(window, cx);
        cx.notify();
    }

    fn pump_text_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_busy || !self.text_dirty {
            return;
        }
        let Some(draft) = self.text_draft.as_ref().map(TextDraft::preview_input) else {
            return;
        };
        let base = self
            .text_overlay
            .is_none()
            .then(|| {
                self.active_document()
                    .map(|document| document.content.clone())
            })
            .flatten();
        self.text_dirty = false;
        self.text_busy = true;
        let token = self.text_generation;
        let job = cx.background_executor().spawn(async move {
            let rendered = if draft.rendered.matches(&draft.spec) {
                draft.rendered
            } else {
                Arc::new(text::render_spec(&draft.spec)?)
            };
            let carets = if draft.carets.matches(&draft.spec) {
                draft.carets
            } else {
                Arc::new(text::caret_map(&draft.spec)?)
            };
            let substitution = text::substitution(&rendered.style);
            let shown = super::layers::with_layer_opacity(&rendered.image, draft.opacity);
            let sprite = document::preview_render(&shown);
            let (below, above) = base
                .as_ref()
                .map(|content| {
                    if let Some(layer) = draft.layer {
                        content.preview_around(layer)
                    } else {
                        content.preview_at_insertion(draft.above)
                    }
                })
                .unwrap_or((None, None));
            let overlay = MovePaint {
                below: below.as_ref().map(document::preview_render),
                above: above.as_ref().map(document::preview_render),
                sprite,
                offset: rendered.style.offset_for(draft.anchor),
                size: rendered.image.dimensions(),
            };
            Ok::<_, TextError>((rendered, carets, substitution, overlay))
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.text_busy = false;
                if workspace.text_generation != token || workspace.text_draft.is_none() {
                    if let Ok((_, _, _, overlay)) = result {
                        super::drop_images(overlay.into_images(), window, cx);
                    }
                } else {
                    match result {
                        Ok((rendered, carets, substitution, overlay)) => {
                            let draft = workspace.text_draft.as_mut().unwrap();
                            draft.rendered = rendered;
                            draft.carets = carets;
                            draft.substitution = substitution;
                            draft.resolve_pending_pointer();
                            if let Some(existing) = workspace.text_overlay.as_mut() {
                                let previous =
                                    std::mem::replace(&mut existing.sprite, overlay.sprite);
                                existing.offset = overlay.offset;
                                existing.size = overlay.size;
                                super::drop_images(
                                    [previous]
                                        .into_iter()
                                        .chain(overlay.below)
                                        .chain(overlay.above),
                                    window,
                                    cx,
                                );
                            } else {
                                workspace.text_overlay = Some(overlay);
                            }
                        }
                        Err(error) => {
                            workspace.text_error = Some(error);
                            workspace.notice(error.message(), cx);
                        }
                    }
                    cx.notify();
                }
                workspace.pump_text_preview(window, cx);
            });
        })
        .detach();
    }
}

fn grapheme_boundary(text: &str, cursor: usize, forward: bool) -> usize {
    if forward {
        text[cursor..]
            .grapheme_indices(true)
            .nth(1)
            .map_or(text.len(), |(at, _)| cursor + at)
    } else {
        text[..cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(at, _)| at)
    }
}

fn word_boundary(text: &str, cursor: usize, forward: bool) -> usize {
    if forward {
        text.unicode_word_indices()
            .map(|(at, word)| at + word.len())
            .find(|&at| at > cursor)
            .unwrap_or(text.len())
    } else {
        text.unicode_word_indices()
            .map(|(at, _)| at)
            .rfind(|&at| at < cursor)
            .unwrap_or(0)
    }
}

fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|ch| *ch == '\n' || *ch == '\t' || !ch.is_control())
        .collect()
}

fn clamp_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_space_and_menu_keys_do_not_depend_on_windows_character_data() {
        let space = Keystroke::parse("space").unwrap();
        assert_eq!(space.key_char, None);
        assert_eq!(typed_text(&space, true), Some(" "));
        assert_eq!(typed_text(&space, false), Some(" "));
        let mut option_space = Keystroke::parse("alt-space").unwrap();
        option_space.key_char = Some("\u{a0}".into());
        assert_eq!(typed_text(&option_space, false), Some("\u{a0}"));
        let mut menu = Keystroke::parse("alt-f").unwrap();
        menu.key_char = Some("f".into());
        assert_eq!(typed_text(&menu, true), None);
        assert!(text_command_key(&menu, true));
        // Preserve macOS Option-produced characters.
        menu.key_char = Some("ƒ".into());
        assert_eq!(typed_text(&menu, false), Some("ƒ"));
        let mut shortcut = Keystroke::parse("ctrl-s").unwrap();
        shortcut.key_char = Some("s".into());
        assert_eq!(typed_text(&shortcut, true), None);
    }

    #[test]
    fn font_filter_matches_partial_names_aliases_and_unicode_without_case() {
        for (label, name, query) in [
            ("Arial Rounded", "Arial Rounded", "  ARIAL "),
            ("Sans Serif", "sans-serif", "sans-"),
            ("Café Sans", "Café Sans", "CAFÉ"),
            ("Myanmar မြန်မာ", "Myanmar မြန်မာ", "မြန်မာ"),
            ("Any Font", "Any Font", " "),
        ] {
            assert!(matches_font(label, name, query));
        }
        assert!(!matches_font("Arial", "Arial", "Helvetica"));
        let families = ["Arial", "Arial Rounded", "Helvetica"];
        let filtered: Vec<_> = families
            .into_iter()
            .filter(|name| matches_font(name, name, "arial"))
            .collect();
        assert_eq!(filtered, ["Arial", "Arial Rounded"]);
    }

    #[test]
    fn font_query_history_restores_the_query_and_resets_the_highlight() {
        let mut search = FontSearch {
            selected: 5,
            ..Default::default()
        };
        search.edit(|field| *field = Field::new("Arial".into()));
        assert_eq!(search.selected, 0);
        search.edit(|field| *field = Field::new("Café".into()));
        search.step_history(false);
        assert_eq!(search.field.text(), "Arial");
        search.step_history(true);
        assert_eq!(search.field.text(), "Café");
        search.step_history(false);
        search.edit(|field| *field = Field::new("Courier".into()));
        search.step_history(true);
        assert_eq!(search.field.text(), "Courier");
        assert!(search.redo.is_empty());
    }

    fn draft(layer: Option<LayerId>) -> TextDraft {
        let rendered = Arc::new(text::RenderedText {
            image: Arc::new(image::RgbaImage::new(1, 1)),
            style: text::TextStyle {
                source: "café 上".into(),
                family: "sans-serif".into(),
                face: FontFace::default(),
                size: 24.0,
                color: image::Rgba([0, 0, 0, 255]),
                spacing: Spacing::default(),
                layout: TextLayout::default(),
                resolved_family: "sans-serif".into(),
                origin: (0, 0),
            },
        });
        TextDraft {
            tab: 1,
            generation: 0,
            above: LayerId(0),
            opacity: 255,
            substitution: None,
            layer,
            anchor: (2, 3),
            source: rendered.style.source.clone(),
            cursor: rendered.style.source.len(),
            cursor_affinity: CaretAffinity::Downstream,
            selection_anchor: None,
            family: "sans-serif".into(),
            face: FontFace::default(),
            size: 24.0,
            color: rendered.style.color,
            spacing: Spacing::default(),
            layout: TextLayout::default(),
            baseline: rendered.clone(),
            baseline_anchor: (2, 3),
            rendered,
            carets: Arc::new(CaretMap::default()),
            pending_hit: None,
            pending_extend: false,
            pending_drag_hit: None,
            dragging: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    #[test]
    fn text_undo_redo_preserves_unicode_style_and_placement_without_touching_pixels() {
        let mut draft = draft(Some(LayerId(1)));
        let pixels = draft.rendered.image.clone();
        assert!(!draft.changed());
        let initial = draft.state();
        draft.source.push('!');
        draft.cursor = draft.source.len();
        draft.family = "serif".into();
        draft.anchor = (10, 20);
        draft.spacing.letter = 3.0;
        draft.face = FontFace {
            bold: true,
            italic: true,
        };
        draft.record(initial);
        assert!(draft.changed());
        assert!(draft.step_history(false));
        assert_eq!(draft.source, "café 上");
        assert!(draft.source.is_char_boundary(draft.cursor));
        assert_eq!(draft.anchor, (2, 3));
        assert_eq!(draft.family, "sans-serif");
        assert_eq!(draft.spacing, Spacing::default());
        assert_eq!(draft.face, FontFace::default());
        assert!(!draft.changed());
        assert!(draft.step_history(true));
        assert_eq!(draft.source, "café 上!");
        assert_eq!(draft.anchor, (10, 20));
        assert_eq!(draft.family, "serif");
        assert_eq!(draft.spacing.letter, 3.0);
        assert_eq!(
            draft.face,
            FontFace {
                bold: true,
                italic: true
            }
        );
        assert!(Arc::ptr_eq(&pixels, &draft.rendered.image));
        assert!(draft.step_history(false));
        let previous = draft.state();
        draft.source.push('?');
        draft.record(previous);
        assert!(!draft.step_history(true));
        for _ in 0..150 {
            let previous = draft.state();
            draft.size += 1.0;
            draft.record(previous);
        }
        assert_eq!(draft.undo.len(), 100);
    }

    #[test]
    fn pending_text_detects_source_spacing_and_anchor_changes_but_not_selection() {
        let mut existing = draft(Some(LayerId(1)));
        let signature = existing.recovery_signature();
        existing.selection_anchor = Some(0);
        existing.cursor = existing.source.len();
        existing.cursor = 0;
        assert!(!existing.changed());
        assert!(existing.recovery_signature() == signature);
        existing.anchor.0 += 1;
        assert!(existing.changed());
        assert!(existing.recovery_signature() != signature);
        existing.anchor.0 -= 1;
        existing.spacing.line = 2.0;
        assert!(existing.changed());
        assert!(existing.recovery_signature() != signature);
        let mut new = draft(None);
        assert!(new.changed());
        new.source.clear();
        assert!(!new.changed());
        new.source.push('上');
        assert!(new.changed());
    }

    #[test]
    fn font_query_retains_modified_navigation_without_reaching_canvas_text() {
        for key in [
            "alt-left",
            "alt-shift-right",
            "cmd-left",
            "cmd-shift-down",
            "ctrl-backspace",
            "ctrl-home",
        ] {
            assert!(font_query_navigation_key(&Keystroke::parse(key).unwrap()));
        }
        for key in ["cmd-s", "ctrl-c", "alt-o", "a"] {
            assert!(!font_query_navigation_key(&Keystroke::parse(key).unwrap()));
        }
    }

    #[test]
    fn partial_selection_replaces_only_its_range_and_history_restores_that_selection() {
        let mut draft = draft(Some(LayerId(1)));
        let pixels = draft.rendered.image.clone();
        draft.move_to(3, false);
        draft.move_to(draft.source.len(), true);
        assert_eq!(&draft.source[draft.selected_range().unwrap()], "é 上");
        let previous = draft.state();
        draft.replace_selection("new");
        draft.record(previous);
        assert_eq!(draft.source, "cafnew");
        assert_eq!(draft.cursor, 6);
        assert_eq!(draft.selected_range(), None);
        assert!(draft.step_history(false));
        assert_eq!(draft.source, "café 上");
        assert_eq!(draft.selected_range(), Some(3..9));
        assert!(draft.step_history(true));
        assert_eq!(draft.source, "cafnew");
        assert!(Arc::ptr_eq(&pixels, &draft.rendered.image));
        draft.move_to(1, false);
        draft.move_to(0, true);
        assert!(draft.delete(false, false));
        assert_eq!(draft.source, "afnew");
    }

    #[test]
    fn grapheme_and_word_deletion_preserve_unicode_clusters() {
        let mut draft = draft(None);
        draft.source = "a\u{301} 👩‍💻 café 上".into();
        draft.cursor = "a\u{301} 👩‍💻".len();
        assert!(draft.delete(true, false));
        assert_eq!(draft.source, "a\u{301}  café 上");
        draft.move_to(0, false);
        assert!(draft.delete(false, false));
        assert_eq!(draft.source, "  café 上");
        assert_eq!(word_boundary(&draft.source, 0, true), "  café".len());
        draft.move_to(draft.source.len(), false);
        assert!(draft.delete(true, true));
        assert_eq!(draft.source, "  café ");
        assert!(draft.delete(true, true));
        assert_eq!(draft.source, "  ");
        assert_eq!(grapheme_boundary("a\u{301}b", 0, true), "a\u{301}".len());
        assert_eq!(grapheme_boundary("a\u{301}b", "a\u{301}".len(), false), 0);
    }

    #[test]
    fn wrapped_caret_keeps_pointer_navigation_selection_and_history_affinity() {
        use CaretAffinity::{Downstream, Upstream};
        let mut draft = draft(None);
        draft.source = "MMMMMMMMMMMMMMMMMMMM".into();
        draft.layout = TextLayout {
            width: Some(150),
            height: Some(200),
            align: TextAlign::Left,
        };
        draft.carets = Arc::new(text::caret_map(&draft.spec()).unwrap());
        let pair = draft
            .carets
            .stops
            .windows(2)
            .find(|p| p[0].byte == p[1].byte && p[0].line != p[1].line)
            .unwrap();
        let (end, start) = (pair[0], pair[1]);
        let position = draft
            .carets
            .hit_position(end.x, end.top + draft.carets.line_height / 2.)
            .unwrap();
        assert_eq!(position, (end.byte, Upstream));
        draft.move_to_position(position, false);
        assert_eq!(
            draft
                .carets
                .at_affinity(draft.cursor, draft.cursor_affinity)
                .unwrap(),
            end
        );
        assert_eq!(
            draft.navigation_position("end", false, false, false),
            position
        );
        let home = draft.navigation_position("home", false, false, false);
        assert_eq!(
            draft.carets.at_affinity(home.0, home.1).unwrap().line,
            end.line
        );
        let down = draft.navigation_position("down", false, false, false);
        assert_eq!(
            draft.carets.at_affinity(down.0, down.1).unwrap().line,
            start.line
        );
        draft.move_to_position(
            draft.navigation_position("right", false, false, false),
            false,
        );
        assert_eq!(
            (draft.cursor, draft.cursor_affinity),
            (start.byte, Downstream)
        );
        draft.move_to_position(
            draft.navigation_position("left", false, false, false),
            false,
        );
        assert_eq!((draft.cursor, draft.cursor_affinity), position);
        let previous = draft.state();
        draft.replace_selection("X");
        draft.record(previous);
        assert!(draft.step_history(false));
        assert_eq!((draft.cursor, draft.cursor_affinity), position);
        draft.move_to_position(draft.navigation_position("home", false, false, true), true);
        assert_eq!(draft.selected_range(), Some(home.0..end.byte));
        // Pending layout hits must preserve the clicked side too, including after release.
        draft.pending_hit = Some((end.x, end.top + draft.carets.line_height / 2.));
        draft.pending_extend = false;
        draft.resolve_pending_pointer();
        assert_eq!((draft.cursor, draft.cursor_affinity), position);
    }

    #[test]
    fn delayed_pointer_selection_resolves_after_release_and_edits_cancel_old_hits() {
        let mut draft = draft(None);
        draft.carets = Arc::new(text::caret_map(&draft.spec()).unwrap());
        let start = draft.carets.at(3).unwrap();
        let end = draft.carets.at(draft.source.len()).unwrap();
        draft.pending_hit = Some((start.x, start.top));
        draft.pending_drag_hit = Some((end.x, end.top));
        draft.dragging = false;
        draft.resolve_pending_pointer();
        assert_eq!(draft.selected_range(), Some(3..9));
        draft.pending_hit = Some((0., 0.));
        draft.replace_selection("X");
        let cursor = draft.cursor;
        draft.resolve_pending_pointer();
        assert_eq!(draft.source, "cafX");
        assert_eq!(draft.cursor, cursor);
        assert!(draft.pending_hit.is_none());
    }

    #[test]
    fn a_click_inside_the_text_box_hits_a_caret_stop() {
        let mut draft = draft(None);
        draft.source = "ab\ncd".into();
        draft.carets = Arc::new(text::caret_map(&draft.spec()).expect("the sample text lays out"));
        assert!(draft.contains((2, 3), 0.0));
        assert!(!draft.contains((2000, 3000), 4.0));
        // A click just past the box edits it only within the slop.
        let (_, _, right, _) = draft.bounds();
        let beside = (right as i64 + 6, 5);
        assert!(!draft.contains(beside, 2.0));
        assert!(draft.contains(beside, 8.0));
        // The second line starts after the newline.
        let line = draft.carets.line_height;
        let byte = draft.hit((2, 3 + line as i64 + 1)).unwrap();
        assert_eq!(byte, 3);
    }

    #[test]
    fn clicking_before_the_preview_catches_up_cannot_reuse_stale_utf8_offsets() {
        let mut draft = draft(None);
        draft.source = "abcdef".into();
        draft.carets = Arc::new(text::caret_map(&draft.spec()).unwrap());
        draft.source = "上".into();
        draft.cursor = draft.source.len();
        assert_eq!(draft.hit((20, 4)), None);
        draft.carets = Arc::new(text::caret_map(&draft.spec()).unwrap());
        let byte = draft.hit((20, 4)).unwrap();
        assert!(draft.source.is_char_boundary(byte));
    }

    #[test]
    fn failed_commits_keep_the_draft_unsaved_until_it_can_be_recovered() {
        let document = document::Document::blank("Image".into(), 2, 2, image::Rgba([0, 0, 0, 0]));
        let mut tab = super::super::Tab {
            id: 1,
            stroke_anchor: None,
            active_layer: document.content.top_layer_id(),
            document,
            zoom: crate::commands::Zoom::Fit,
            pan: gpui::Point::default(),
            history: super::super::history::History::new(true),
            editing: false,
            saving: None,
            exporting: false,
            notice: None,
            building_level: None,
            selection: crate::selection::Selection::Unrestricted,
            pending_text: Some(draft(None)),
            pending_opacity: None,
            pending_guides: None,
        };
        assert!(tab.settle_text_commit(false));
        assert!(tab.dirty());
        assert_eq!(tab.pending_text.as_ref().unwrap().source, "café 上");
        assert!(tab.notice.is_some());
        assert!(!tab.settle_text_commit(true));
        assert!(tab.pending_text.is_none());
        assert!(!tab.dirty());
    }

    #[test]
    fn failed_text_rendering_and_full_layer_stacks_leave_the_document_intact() {
        let mut draft = draft(None);
        let mut content = document::DocumentContent::single_layer(
            "Background".into(),
            image::RgbaImage::new(2, 2),
        );
        let mut active = content.top_layer_id();
        let pixels = content.layer(active).unwrap().pixels.clone();
        draft.source = "A".repeat(MAX_TEXT_CHARS);
        draft.size = MAX_PIXEL_SIZE;
        assert_eq!(draft.apply(&mut content, &mut active), Dirty::None);
        assert!(draft.changed());
        assert_eq!(content.layers().len(), 1);
        assert!(Arc::ptr_eq(&pixels, &content.layer(active).unwrap().pixels));

        while content.can_add_layer() {
            content
                .add_layer(active, "Layer".into(), image::Rgba([0, 0, 0, 0]))
                .unwrap();
        }
        draft.source = draft.rendered.style.source.clone();
        draft.size = draft.rendered.style.size;
        assert_eq!(draft.apply(&mut content, &mut active), Dirty::None);
        assert!(draft.changed());
        assert!(Arc::ptr_eq(&pixels, &content.layer(active).unwrap().pixels));
    }
}
