use std::{cell::RefCell, rc::Rc};

use gpui::{
    Bounds, Context, Div, Font, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    ScrollHandle, TextRun, Window, canvas, div, prelude::*, px, rgb,
};

use super::{ACCENT, BORDER, CANVAS, Field, FormKind, Workspace};

#[derive(Clone, Copy)]
pub(super) enum PdfInput {
    Dpi,
    Pages,
}

#[derive(Clone)]
struct InputMetrics {
    bounds: Bounds<Pixels>,
    font: Font,
}

#[derive(Clone)]
pub(super) struct PdfInputDrag {
    input: PdfInput,
    metrics: Rc<RefCell<Option<InputMetrics>>>,
    scroll: Option<ScrollHandle>,
}

fn character_at_byte(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    text[..byte].chars().count()
}

fn text_hit_x(pointer: Pixels, left: Pixels, scroll_offset: Pixels) -> Pixels {
    pointer - left - px(9.) - scroll_offset
}

impl Workspace {
    pub(super) fn pdf_input_view(
        &self,
        input: PdfInput,
        field: &Field,
        active: bool,
        scroll: Option<&ScrollHandle>,
        cx: &Context<Self>,
    ) -> gpui::Stateful<Div> {
        let metrics = Rc::new(RefCell::new(None));
        let paint = metrics.clone();
        let drag = PdfInputDrag {
            input,
            metrics,
            scroll: scroll.cloned(),
        };
        let editor = field
            .view(active)
            .id(match input {
                PdfInput::Dpi => "pdf-dpi-editor",
                PdfInput::Pages => "pdf-pages-editor",
            })
            .w_full()
            .min_w_0()
            .h(px(32.))
            .px_2()
            .border_0()
            .rounded_none()
            .text_size(px(13.))
            .whitespace_nowrap()
            .when_some(scroll, |editor, scroll| {
                editor
                    .overflow_x_scroll()
                    .track_scroll(scroll)
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            });
        div()
            .relative()
            .w_full()
            .min_w_0()
            .h(px(34.))
            .rounded_md()
            .bg(rgb(CANVAS))
            .border_1()
            .border_color(rgb(if active { ACCENT } else { BORDER }))
            .id(match input {
                PdfInput::Dpi => "pdf-dpi-input",
                PdfInput::Pages => "pdf-pages-input",
            })
            .cursor(gpui::CursorStyle::IBeam)
            .overflow_hidden()
            .child(editor)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        *paint.borrow_mut() = Some(InputMetrics {
                            bounds,
                            font: window.text_style().font(),
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.begin_pdf_input_drag(drag.clone(), event, window, cx);
                    cx.stop_propagation();
                }),
            )
    }

    fn place_pdf_input_cursor(
        &mut self,
        drag: &PdfInputDrag,
        position: Point<Pixels>,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(metrics) = drag.metrics.borrow().clone() else {
            return;
        };
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let FormKind::PdfImport {
            dpi,
            page_selection,
            active,
            all_pages,
            ..
        } = &mut form.kind
        else {
            return;
        };
        let field = match drag.input {
            PdfInput::Dpi => {
                *active = 0;
                dpi
            }
            PdfInput::Pages => {
                *active = 1;
                *all_pages = false;
                page_selection
            }
        };
        let run = TextRun {
            len: field.text.len(),
            font: metrics.font,
            color: rgb(CANVAS).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line =
            window
                .text_system()
                .shape_line(field.text.clone().into(), px(13.), &[run], None);
        let offset = drag
            .scroll
            .as_ref()
            .map_or(px(0.), |scroll| scroll.offset().x);
        let byte = line.closest_index_for_x(text_hit_x(position.x, metrics.bounds.left(), offset));
        field.move_cursor(character_at_byte(&field.text, byte), extend);
        if let Some(scroll) = &drag.scroll {
            scroll.scroll_to_item(field.caret_item());
        }
        form.error = None;
        cx.notify();
    }

    fn begin_pdf_input_drag(
        &mut self,
        drag: PdfInputDrag,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.place_pdf_input_cursor(&drag, event.position, event.modifiers.shift, window, cx);
        if let Some(form) = self.form.as_mut() {
            if event.click_count >= 2 {
                if let FormKind::PdfImport {
                    dpi,
                    page_selection,
                    ..
                } = &mut form.kind
                {
                    match drag.input {
                        PdfInput::Dpi => dpi,
                        PdfInput::Pages => page_selection,
                    }
                    .select_all();
                    if let Some(scroll) = &drag.scroll {
                        scroll.scroll_to_item(0);
                    }
                }
                form.pdf_input_drag = None;
            } else {
                form.pdf_input_drag = Some(drag);
            }
        }
        cx.notify();
    }

    pub(super) fn extend_pdf_input_drag(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_pdf_input_drag();
            return;
        }
        if let Some(drag) = self
            .form
            .as_ref()
            .and_then(|form| form.pdf_input_drag.clone())
        {
            self.place_pdf_input_cursor(&drag, event.position, true, window, cx);
        }
    }

    pub(super) fn end_pdf_input_drag(&mut self) {
        if let Some(form) = &mut self.form {
            form.pdf_input_drag = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hit_testing_accounts_for_padding_and_horizontal_scroll() {
        assert_eq!(text_hit_x(px(129.), px(100.), px(0.)), px(20.));
        assert_eq!(text_hit_x(px(129.), px(100.), px(-50.)), px(70.));
        assert_eq!(text_hit_x(px(100.), px(100.), px(0.)), px(-9.));
    }
    #[test]
    fn hit_indices_stay_on_character_boundaries_through_unicode_dashes() {
        let text = "1, 3–5";
        assert_eq!(character_at_byte(text, 4), 4);
        assert_eq!(character_at_byte(text, 5), 4);
        assert_eq!(character_at_byte(text, 7), 5);
        assert_eq!(character_at_byte(text, usize::MAX), text.chars().count());
        assert_eq!(character_at_byte("", 3), 0);
    }
}
