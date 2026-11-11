use gpui::{ClipboardItem, Context, Div, KeyDownEvent, SharedString, Window, div, prelude::*, rgb};

use super::Workspace;
use super::dialogs::form_button;
use super::layers::MovePaint;
use super::view::MUTED;
use crate::commands::{
    ApplyText, CancelText, RasterizeText, TextCopy, TextCut, TextPaste, TextSelectAll,
};
use crate::document::{self, LayerId};
use crate::preview::Dirty;
use crate::text::{self, MAX_PIXEL_SIZE, MAX_TEXT_CHARS, MIN_PIXEL_SIZE, TextError};

const DEFAULT_TEXT_SIZE: f32 = 32.0;

#[derive(Clone)]
pub(super) struct TextDraft {
    layer: Option<LayerId>,
    anchor: (i32, i32),
    source: String,
    cursor: usize,
    selected: bool,
    family: String,
    size: f32,
    color: image::Rgba<u8>,
    rendered: text::RenderedText,
}

impl Workspace {
    pub(super) fn bind_text_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &ApplyText, window, cx| {
            this.apply_text(window, cx);
        }))
        .on_action(cx.listener(|this, _: &CancelText, window, cx| {
            this.cancel_text(window, cx);
        }))
        .on_action(cx.listener(|this, _: &RasterizeText, window, cx| {
            this.rasterize_text_layer(window, cx);
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
    }

    pub(super) fn text_options(&self, panel: Div, cx: &Context<Self>) -> Div {
        let Some(draft) = &self.text_draft else {
            return panel
                .child("Click the canvas to place text. Typing stays in the text layer.")
                .child(form_button("Rasterize Text", false).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.rasterize_text_layer(window, cx);
                    },
                )));
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
        let substituted = draft.rendered.style.substituted().then(|| {
            format!(
                "Using {} instead of {}.",
                draft.rendered.style.resolved_family, draft.family
            )
        });
        let can_apply = !draft.source.trim().is_empty();
        panel
            .child("Apply commits one undo step. Escape discards the draft.")
            .child(div().text_color(rgb(MUTED)).text_ellipsis().child(source))
            .when_some(substituted, |panel, message| panel.child(message))
            .child(div().flex().gap_2().children([
                family_button("Sans Serif", "sans-serif", &family, cx),
                family_button("Serif", "serif", &family, cx),
                family_button("Monospace", "monospace", &family, cx),
            ]))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(form_button("Smaller", false).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.step_text_size(-1.0, window, cx);
                        },
                    )))
                    .child(size)
                    .child(form_button("Larger", false).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.step_text_size(1.0, window, cx);
                        },
                    ))),
            )
            .child(form_button("Use Foreground", false).on_click(cx.listener(
                |this, _, window, cx| {
                    this.recolor_text(window, cx);
                },
            )))
            .child(div().flex().gap_2().children([
                form_button("Apply Text", can_apply).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.apply_text(window, cx);
                    },
                )),
                form_button("Discard Text", false).on_click(cx.listener(|this, _, window, cx| {
                    this.cancel_text(window, cx);
                })),
            ]))
            .child(form_button("Rasterize Text", false).on_click(cx.listener(
                |this, _, window, cx| {
                    this.rasterize_text_layer(window, cx);
                },
            )))
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
            layer
                .text
                .clone()
                .map(|style| (layer.offset, style, layer.visible))
        });
        if let Some((offset, style, visible)) = existing {
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
            self.open_draft(Some(active), style.anchor(offset), style, window, cx);
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
            if let Some(draft) = self.text_draft.as_mut() {
                draft.anchor = anchor;
            }
            self.refresh_text_sprite(window, cx);
            cx.notify();
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
            text::TextStyle {
                source: String::new(),
                family: "sans-serif".to_owned(),
                size: DEFAULT_TEXT_SIZE,
                color,
                resolved_family: "sans-serif".to_owned(),
                origin: (0, 0),
            },
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
        let Some(draft) = self.text_draft.clone() else {
            self.notice("Click the canvas to place text.", cx);
            return;
        };
        if draft.source.trim().is_empty() {
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
        let above = self.active_tab().map(|tab| tab.active_layer);
        let layer = draft.layer;
        let anchor = draft.anchor;
        let rendered = draft.rendered;
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

    fn open_draft(
        &mut self,
        layer: Option<LayerId>,
        anchor: (i32, i32),
        style: text::TextStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rendered = match text::render(&style.source, &style.family, style.size, style.color) {
            Ok(rendered) => rendered,
            Err(error) => {
                self.notice(error.message(), cx);
                return;
            }
        };
        let cursor = rendered.style.source.len();
        self.text_draft = Some(TextDraft {
            layer,
            anchor,
            source: rendered.style.source.clone(),
            cursor,
            selected: false,
            family: rendered.style.family.clone(),
            size: rendered.style.size,
            color: rendered.style.color,
            rendered,
        });
        self.show_text_overlay(window, cx);
        cx.notify();
    }

    fn change_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        mutate: impl FnOnce(&mut TextDraft) -> bool,
    ) {
        let Some(saved) = self.text_draft.clone() else {
            return;
        };
        let changed = self.text_draft.as_mut().is_some_and(mutate);
        if !changed {
            cx.notify();
            return;
        }
        let Some(draft) = self.text_draft.as_ref() else {
            return;
        };
        match text::render(&draft.source, &draft.family, draft.size, draft.color) {
            Ok(rendered) => {
                if let Some(draft) = self.text_draft.as_mut() {
                    draft.rendered = rendered;
                }
                self.refresh_text_sprite(window, cx);
                cx.notify();
            }
            Err(error) => {
                self.text_draft = Some(saved);
                self.notice(error.message(), cx);
            }
        }
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

    fn copy_text(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.text_draft.as_ref() else {
            return;
        };
        if draft.selected && !draft.source.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(draft.source.clone()));
        }
    }

    fn cut_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn paste_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.insert_text(&text, window, cx);
    }

    fn select_all_text(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.text_draft.as_mut()
            && !draft.source.is_empty()
        {
            draft.selected = true;
            cx.notify();
        }
    }

    fn show_text_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.text_draft.as_ref() else {
            return;
        };
        let target = draft.layer;
        let (below, above) = {
            let Some(tab) = self.active_tab() else {
                return;
            };
            let content = &tab.document.content;
            if let Some(id) = target {
                if content.layer(id).is_none() {
                    return;
                }
                let (below, above) = content.composite_around(id);
                (
                    below.as_ref().map(document::preview_render),
                    above.as_ref().map(document::preview_render),
                )
            } else {
                let image = content.composite();
                (Some(document::preview_render(&image)), None)
            }
        };
        let Some(draft) = self.text_draft.as_ref() else {
            return;
        };
        let sprite = document::preview_render(&draft.rendered.image);
        let offset = draft.rendered.style.offset_for(draft.anchor);
        let size = draft.rendered.image.dimensions();
        if let Some(overlay) = self.text_overlay.take() {
            super::drop_images(overlay.into_images(), window, cx);
        }
        self.text_overlay = Some(MovePaint {
            below,
            sprite,
            above,
            offset,
            size,
        });
    }

    fn refresh_text_sprite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.text_draft.as_ref() else {
            return;
        };
        let sprite = document::preview_render(&draft.rendered.image);
        let offset = draft.rendered.style.offset_for(draft.anchor);
        let size = draft.rendered.image.dimensions();
        let Some(overlay) = self.text_overlay.as_mut() else {
            return;
        };
        let previous = std::mem::replace(&mut overlay.sprite, sprite);
        overlay.offset = offset;
        overlay.size = size;
        super::drop_images([previous], window, cx);
    }
}

fn family_button(
    label: &'static str,
    family: &'static str,
    current: &str,
    cx: &Context<Workspace>,
) -> gpui::Stateful<Div> {
    let chosen = current == family;
    form_button(label, chosen).on_click(cx.listener(move |this, _, window, cx| {
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
