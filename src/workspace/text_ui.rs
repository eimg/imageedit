use std::sync::Arc;

use gpui::{ClipboardItem, Context, Div, KeyDownEvent, SharedString, Window, div, prelude::*, rgb};

use super::Workspace;
use super::layers::MovePaint;
use super::view::{
    MUTED, action_row, choice_button, option_action, option_row, section_label, segmented,
    step_button, stepper,
};
use crate::commands::{
    ApplyText, CancelText, RasterizeText, Redo, TextCopy, TextCut, TextPaste, TextSelectAll, Undo,
};
use crate::document::{self, LayerId};
use crate::preview::Dirty;
use crate::text::{self, MAX_PIXEL_SIZE, MAX_TEXT_CHARS, MIN_PIXEL_SIZE, TextError};

const DEFAULT_TEXT_SIZE: f32 = 32.0;

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
    selected: bool,
    family: String,
    size: f32,
    color: image::Rgba<u8>,
    rendered: Arc<text::RenderedText>,
    baseline: Arc<text::RenderedText>,
    baseline_anchor: (i32, i32),
    undo: Vec<TextEdit>,
    redo: Vec<TextEdit>,
}

#[derive(Clone)]
struct TextEdit {
    source: String,
    cursor: usize,
    selected: bool,
    family: String,
    size: f32,
    color: image::Rgba<u8>,
    anchor: (i32, i32),
}

struct TextPreviewInput {
    rendered: Arc<text::RenderedText>,
    source: String,
    family: String,
    size: f32,
    color: image::Rgba<u8>,
    opacity: u8,
    layer: Option<LayerId>,
    above: LayerId,
    anchor: (i32, i32),
}

impl TextDraft {
    fn preview_input(&self) -> TextPreviewInput {
        TextPreviewInput {
            rendered: self.rendered.clone(),
            source: self.source.clone(),
            family: self.family.clone(),
            size: self.size,
            color: self.color,
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
            selected: self.selected,
            family: self.family.clone(),
            size: self.size,
            color: self.color,
            anchor: self.anchor,
        }
    }

    fn restore(&mut self, state: TextEdit) {
        self.source = state.source;
        self.cursor = state.cursor;
        self.selected = state.selected;
        self.family = state.family;
        self.size = state.size;
        self.color = state.color;
        self.anchor = state.anchor;
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

    fn changed(&self) -> bool {
        if self.layer.is_none() {
            return !self.source.is_empty();
        }
        self.anchor != self.baseline_anchor
            || !self
                .baseline
                .matches(&self.source, &self.family, self.size, self.color)
    }
}

impl Workspace {
    pub(super) fn bind_text_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        let drafting = self.text_draft.is_some();
        let rasterize = !drafting
            && self.active_tab().is_some_and(|tab| {
                !tab.busy() && tab.document.content.layer_is_text(tab.active_layer)
            });
        root.when(self.can_apply_text(), |root| {
            root.on_action(cx.listener(|this, _: &ApplyText, window, cx| {
                this.apply_text(window, cx);
            }))
        })
        .when(drafting, |root| {
            root.on_action(cx.listener(|this, _: &CancelText, window, cx| {
                this.cancel_text(window, cx);
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
        })
        .when(rasterize, |root| {
            root.on_action(cx.listener(|this, _: &RasterizeText, window, cx| {
                this.rasterize_text_layer(window, cx);
            }))
        })
        .when(
            self.text_draft
                .as_ref()
                .is_some_and(|draft| !draft.undo.is_empty()),
            |root| {
                root.on_action(cx.listener(|this, _: &Undo, window, cx| {
                    this.step_text_history(false, window, cx)
                }))
            },
        )
        .when(
            self.text_draft
                .as_ref()
                .is_some_and(|draft| !draft.redo.is_empty()),
            |root| {
                root.on_action(cx.listener(|this, _: &Redo, window, cx| {
                    this.step_text_history(true, window, cx)
                }))
            },
        )
    }

    pub(super) fn text_options(&self, panel: Div, cx: &Context<Self>) -> Div {
        let Some(draft) = &self.text_draft else {
            return panel
                .child("Click the canvas to place text. Typing stays in the text layer.")
                .child(
                    option_action("Rasterize Text", false)
                        .w_full()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.rasterize_text_layer(window, cx);
                        })),
                );
        };
        let family = draft.family.clone();
        let size = size_label(draft.size);
        let source: SharedString = if draft.source.is_empty() {
            "Type to enter text. Return starts a new line.".into()
        } else if draft.selected {
            format!("Selected: {}", draft.source.replace('\n', "↵")).into()
        } else {
            draft.source.replace('\n', "↵").into()
        };
        let substituted = draft.substitution.clone();
        let can_apply = self.can_apply_text();
        panel
            .child("Apply commits one undo step. Escape discards the draft.")
            .child(div().text_color(rgb(MUTED)).text_ellipsis().child(source))
            .when_some(substituted, |panel, message| panel.child(message))
            .child(section_label("FONT"))
            .child(segmented([
                family_button("Sans Serif", "sans-serif", &family, cx),
                family_button("Serif", "serif", &family, cx),
                family_button("Monospace", "monospace", &family, cx),
            ]))
            .child(option_row(
                "Size",
                stepper(
                    size,
                    step_button("text-size-down", "−").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.step_text_size(-1.0, window, cx);
                        },
                    )),
                    step_button("text-size-up", "+").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.step_text_size(1.0, window, cx);
                        },
                    )),
                ),
            ))
            .child(
                option_action("Use Foreground", false)
                    .w_full()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.recolor_text(window, cx);
                    })),
            )
            .child(section_label("LAYER"))
            .child(action_row([
                option_action("Apply Text", can_apply)
                    .when(can_apply, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.apply_text(window, cx);
                        }))
                    })
                    .when(!can_apply, |button| {
                        button.opacity(0.45).cursor(gpui::CursorStyle::Arrow)
                    }),
                option_action("Discard Text", false).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.cancel_text(window, cx);
                    },
                )),
            ]))
            .child(
                option_action("Rasterize Text", false)
                    .w_full()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.rasterize_text_layer(window, cx);
                    })),
            )
    }

    pub(super) fn place_text(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let Some(active) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        let existing = self.active_tab().and_then(|tab| {
            let layer = tab.document.content.layer(active)?;
            layer.text.clone().map(|style| {
                (
                    layer.offset,
                    style,
                    layer.visible,
                    layer.pixels.clone(),
                    layer.opacity,
                )
            })
        });
        if let Some((offset, style, visible, pixels, opacity)) = existing {
            if !visible {
                self.notice(
                    "The active layer is hidden. Show it before editing text.",
                    cx,
                );
                return;
            }
            if self
                .text_draft
                .as_ref()
                .is_some_and(|draft| draft.layer == Some(active))
            {
                return;
            }
            self.cancel_text(window, cx);
            let anchor = style.anchor(offset);
            self.open_draft(
                Some(active),
                anchor,
                text::RenderedText {
                    image: pixels,
                    style,
                },
                opacity,
                window,
                cx,
            );
            return;
        }
        let Some(pixel) = self.unclamped_pointer_pixel(position, scale) else {
            return;
        };
        let anchor = (clamp_i32(pixel.0), clamp_i32(pixel.1));
        if self
            .text_draft
            .as_ref()
            .is_some_and(|draft| draft.layer.is_none())
        {
            self.change_text(window, cx, |draft| {
                if draft.anchor == anchor {
                    return false;
                }
                draft.anchor = anchor;
                true
            });
            return;
        }
        let color = self
            .active_tab()
            .map(|tab| {
                tab.document
                    .content
                    .mode()
                    .constrain(document::solid_color(self.colors[0]))
            })
            .unwrap_or(document::solid_color(self.colors[0]));
        self.cancel_text(window, cx);
        self.open_draft(
            None,
            anchor,
            text::RenderedText {
                image: Arc::new(image::RgbaImage::new(1, 1)),
                style: text::TextStyle {
                    source: String::new(),
                    family: "sans-serif".to_owned(),
                    size: DEFAULT_TEXT_SIZE,
                    color,
                    resolved_family: "sans-serif".to_owned(),
                    origin: (0, 0),
                },
            },
            255,
            window,
            cx,
        );
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
        if key.modifiers.secondary() || key.modifiers.control || key.modifiers.platform {
            return;
        }
        match key.key.as_str() {
            "backspace" => self.change_text(window, cx, |draft| {
                if draft.selected {
                    draft.source.clear();
                    draft.cursor = 0;
                    draft.selected = false;
                    return true;
                }
                let Some(previous) = draft.source[..draft.cursor].chars().next_back() else {
                    return false;
                };
                let start = draft.cursor - previous.len_utf8();
                draft.source.replace_range(start..draft.cursor, "");
                draft.cursor = start;
                true
            }),
            "delete" => self.change_text(window, cx, |draft| {
                if draft.selected {
                    draft.source.clear();
                    draft.cursor = 0;
                    draft.selected = false;
                    return true;
                }
                let Some(next) = draft.source[draft.cursor..].chars().next() else {
                    return false;
                };
                let end = draft.cursor + next.len_utf8();
                draft.source.replace_range(draft.cursor..end, "");
                true
            }),
            "left" | "home" => self.change_text(window, cx, |draft| {
                draft.cursor = if draft.selected || key.key == "home" {
                    0
                } else {
                    draft.source[..draft.cursor]
                        .chars()
                        .next_back()
                        .map_or(draft.cursor, |ch| draft.cursor - ch.len_utf8())
                };
                draft.selected = false;
                false
            }),
            "right" | "end" => self.change_text(window, cx, |draft| {
                draft.cursor = if draft.selected || key.key == "end" {
                    draft.source.len()
                } else {
                    draft.source[draft.cursor..]
                        .chars()
                        .next()
                        .map_or(draft.cursor, |ch| draft.cursor + ch.len_utf8())
                };
                draft.selected = false;
                false
            }),
            "enter" => self.insert_text("\n", window, cx),
            "escape" | "tab" | "up" | "down" => {}
            _ => {
                if let Some(text) = key.key_char.as_deref() {
                    self.insert_text(text, window, cx);
                }
            }
        }
    }

    pub(super) fn apply_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_apply_text() {
            self.notice("Finish updating the text draft before applying it.", cx);
            return;
        }
        let Some(draft) = self.text_draft.clone() else {
            self.notice("Click the canvas to place text.", cx);
            return;
        };
        if draft.layer.is_none() && draft.source.trim().is_empty() {
            self.notice("Enter some text before applying.", cx);
            return;
        }
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        if let Some(id) = draft.layer {
            if self.active_tab().is_some_and(|tab| {
                tab.document
                    .content
                    .layer(id)
                    .is_some_and(|layer| !layer.visible)
            }) {
                self.notice(
                    "The active layer is hidden. Show it before editing text.",
                    cx,
                );
                return;
            }
        } else if self
            .active_tab()
            .is_some_and(|tab| !tab.document.content.can_add_layer())
        {
            self.notice("This image cannot take another layer.", cx);
            return;
        }
        let fits = self.active_document().is_some_and(|document| {
            if let Some(layer) = draft.layer {
                document
                    .content
                    .accepts_replacement(layer, draft.rendered.image.dimensions())
            } else {
                document
                    .content
                    .room_for(0, 0, &draft.rendered.image)
                    .is_ok()
            }
        });
        if !fits {
            self.notice("The text does not fit in the layer stack.", cx);
            return;
        }
        let above = self.active_tab().map(|tab| tab.active_layer);
        let layer = draft.layer;
        let anchor = draft.anchor;
        let rendered = Arc::unwrap_or_clone(draft.rendered);
        self.cancel_text(window, cx);
        self.edit_content(
            move |content, active, _| {
                if let Some(id) = layer {
                    content.set_text(id, anchor, rendered)
                } else if let Some(id) =
                    content.insert_text_layer(above.unwrap_or(*active), anchor, rendered)
                {
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

    pub(super) fn cancel_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_draft.is_none() && self.text_overlay.is_none() {
            return;
        }
        self.text_generation = self.text_generation.wrapping_add(1);
        self.text_dirty = false;
        self.text_error = None;
        self.text_draft = None;
        if let Some(overlay) = self.text_overlay.take() {
            super::drop_images(overlay.into_images(), window, cx);
        }
        cx.notify();
    }

    pub(super) fn rasterize_text_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_draft.is_some() {
            self.notice("Apply the text before rasterizing it.", cx);
            return;
        }
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

    pub(super) fn can_apply_text(&self) -> bool {
        self.text_error.is_none()
            && self.text_draft.as_ref().is_some_and(|draft| {
                self.active_tab().is_some_and(|tab| {
                    tab.id == draft.tab
                        && tab.history.generation() == draft.generation
                        && !tab.busy()
                }) && (draft.layer.is_some() || !draft.source.trim().is_empty())
                    && draft
                        .rendered
                        .matches(&draft.source, &draft.family, draft.size, draft.color)
            })
    }

    pub(super) fn has_pending_text(&self) -> bool {
        self.text_draft.as_ref().is_some_and(TextDraft::changed)
    }

    pub(super) fn refuse_pending_text(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.has_pending_text() {
            return false;
        }
        self.notice("Apply the text or press Escape to discard it before changing tools, layers, or images.", cx);
        true
    }

    fn step_text_history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .text_draft
            .as_mut()
            .is_some_and(|draft| draft.step_history(redo))
        {
            self.request_text_preview(window, cx);
        }
    }

    fn open_draft(
        &mut self,
        layer: Option<LayerId>,
        anchor: (i32, i32),
        rendered: text::RenderedText,
        opacity: u8,
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
            selected: false,
            family: rendered.style.family.clone(),
            size: rendered.style.size,
            color: rendered.style.color,
            baseline: rendered.clone(),
            baseline_anchor: anchor,
            rendered,
            undo: Vec::new(),
            redo: Vec::new(),
        });
        self.request_text_preview(window, cx);
    }

    fn change_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        mutate: impl FnOnce(&mut TextDraft) -> bool,
    ) {
        let changed = self.text_draft.as_mut().is_some_and(|draft| {
            let previous = draft.state();
            if !mutate(draft) {
                return false;
            }
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
        let existing = if draft.selected {
            0
        } else {
            draft.source.chars().count()
        };
        if existing + cleaned.chars().count() > MAX_TEXT_CHARS {
            self.notice(TextError::TooLong.message(), cx);
            return;
        }
        self.change_text(window, cx, |draft| {
            if draft.selected {
                draft.source.clear();
                draft.cursor = 0;
                draft.selected = false;
            }
            draft.source.insert_str(draft.cursor, &cleaned);
            draft.cursor += cleaned.len();
            true
        });
    }

    fn step_text_size(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.change_text(window, cx, |draft| {
            let next = (draft.size + delta).clamp(MIN_PIXEL_SIZE, MAX_PIXEL_SIZE);
            if (next - draft.size).abs() < f32::EPSILON {
                return false;
            }
            draft.size = next;
            true
        });
    }

    fn recolor_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(color) = self.active_tab().map(|tab| {
            tab.document
                .content
                .mode()
                .constrain(document::solid_color(self.colors[0]))
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

    pub(super) fn copy_text(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.text_draft.as_ref() else {
            return;
        };
        if draft.selected && !draft.source.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(draft.source.clone()));
        }
    }

    pub(super) fn cut_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_text(cx);
        if self
            .text_draft
            .as_ref()
            .is_some_and(|draft| draft.selected && !draft.source.is_empty())
        {
            self.change_text(window, cx, |draft| {
                draft.source.clear();
                draft.cursor = 0;
                draft.selected = false;
                true
            });
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
            draft.selected = true;
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
            let rendered =
                if draft
                    .rendered
                    .matches(&draft.source, &draft.family, draft.size, draft.color)
                {
                    Ok(draft.rendered)
                } else {
                    text::render(&draft.source, &draft.family, draft.size, draft.color)
                        .map(Arc::new)
                }?;
            let substitution = text::substitution(&rendered.style);
            let shown = super::layers::with_layer_opacity(&rendered.image, draft.opacity);
            let sprite = document::preview_render(&shown);
            let (below, above) = base
                .as_ref()
                .map(|content| {
                    if let Some(layer) = draft.layer {
                        content.composite_around(layer)
                    } else {
                        content.composite_at_insertion(draft.above)
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
            Ok::<_, TextError>((rendered, substitution, overlay))
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.text_busy = false;
                if workspace.text_generation != token || workspace.text_draft.is_none() {
                    if let Ok((_, _, overlay)) = result {
                        super::drop_images(overlay.into_images(), window, cx);
                    }
                } else {
                    match result {
                        Ok((rendered, substitution, overlay)) => {
                            let draft = workspace.text_draft.as_mut().unwrap();
                            draft.rendered = rendered;
                            draft.substitution = substitution;
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

fn family_button(
    label: &'static str,
    family: &'static str,
    current: &str,
    cx: &Context<Workspace>,
) -> gpui::Stateful<Div> {
    let chosen = current == family;
    choice_button(label, chosen).on_click(cx.listener(move |this, _, window, cx| {
        this.change_text(window, cx, |draft| {
            if draft.family == family {
                return false;
            }
            draft.family = family.to_owned();
            true
        });
    }))
}

fn size_label(size: f32) -> String {
    if size.fract() == 0.0 {
        format!("{size:.0} px")
    } else {
        format!("{size:.1} px")
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

    fn draft(layer: Option<LayerId>) -> TextDraft {
        let rendered = Arc::new(text::RenderedText {
            image: Arc::new(image::RgbaImage::new(1, 1)),
            style: text::TextStyle {
                source: "café 上".into(),
                family: "sans-serif".into(),
                size: 24.0,
                color: image::Rgba([0, 0, 0, 255]),
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
            selected: false,
            family: "sans-serif".into(),
            size: 24.0,
            color: rendered.style.color,
            baseline: rendered.clone(),
            baseline_anchor: (2, 3),
            rendered,
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
        draft.record(initial);
        assert!(draft.changed());
        assert!(draft.step_history(false));
        assert_eq!(draft.source, "café 上");
        assert!(draft.source.is_char_boundary(draft.cursor));
        assert_eq!(draft.anchor, (2, 3));
        assert_eq!(draft.family, "sans-serif");
        assert!(!draft.changed());
        assert!(draft.step_history(true));
        assert_eq!(draft.source, "café 上!");
        assert_eq!(draft.anchor, (10, 20));
        assert_eq!(draft.family, "serif");
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
    fn pending_text_detects_source_and_anchor_changes_but_not_selection() {
        let mut existing = draft(Some(LayerId(1)));
        existing.selected = true;
        existing.cursor = 0;
        assert!(!existing.changed());
        existing.anchor.0 += 1;
        assert!(existing.changed());
        let mut new = draft(None);
        assert!(new.changed());
        new.source.clear();
        assert!(!new.changed());
        new.source.push('上');
        assert!(new.changed());
    }
}
