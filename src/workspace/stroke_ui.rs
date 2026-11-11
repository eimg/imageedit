use gpui::{Context, Div, Window, prelude::*};

use super::Workspace;
use crate::commands::{StrokePath, StrokeSelection};
use crate::document::solid_color;
use crate::paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StrokeSource {
    Path,
    Selection,
}

enum OutlineRequest {
    Hidden,
    Missing(&'static str),
    Ready {
        layer: crate::document::LayerId,
        lines: Vec<Vec<(f64, f64)>>,
        color: image::Rgba<u8>,
        brush: crate::paint::Brush,
    },
}

impl Workspace {
    pub(super) fn bind_stroke_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &StrokePath, window, cx| {
            this.stroke_outline(StrokeSource::Path, window, cx);
        }))
        .on_action(cx.listener(|this, _: &StrokeSelection, window, cx| {
            this.stroke_outline(StrokeSource::Selection, window, cx);
        }))
    }

    pub(super) fn set_stroke_source(&mut self, source: StrokeSource, cx: &mut Context<Self>) {
        if self.stroke_source != source {
            self.stroke_source = source;
            cx.notify();
        }
    }

    pub(super) fn stroke_outline(
        &mut self,
        source: StrokeSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refuse_text_pixels(false, cx) {
            return;
        }
        let changed = self.stroke_source != source;
        self.stroke_source = source;
        match self.outline_request(source) {
            None => {
                if changed {
                    cx.notify();
                }
            }
            Some(OutlineRequest::Hidden) => {
                self.notice("The active layer is hidden. Show it before painting.", cx);
            }
            Some(OutlineRequest::Missing(message)) => self.notice(message, cx),
            Some(OutlineRequest::Ready {
                layer,
                lines,
                color,
                brush,
            }) => {
                self.edit_content(
                    move |content, _, _| content.stroke_lines(layer, &lines, color, brush),
                    window,
                    cx,
                );
            }
        }
    }

    fn outline_request(&self, source: StrokeSource) -> Option<OutlineRequest> {
        let tab = self.active_tab().filter(|tab| !tab.busy())?;
        let layer = tab.document.content.layer(tab.active_layer)?;
        if !layer.visible {
            return Some(OutlineRequest::Hidden);
        }
        if self.brush.opacity == 0 {
            return None;
        }
        let lines = match source {
            StrokeSource::Path => {
                let Some(path) = tab.document.content.active_path() else {
                    return Some(OutlineRequest::Missing(
                        "There is no active path to stroke.",
                    ));
                };
                let line = paths::stroke(path);
                if !separated(&line) {
                    return Some(OutlineRequest::Missing(
                        "There is no active path to stroke.",
                    ));
                }
                vec![line]
            }
            StrokeSource::Selection => {
                let (width, height) = tab.document.content.size();
                let lines = tab
                    .selection
                    .stroke_polylines(width, height)
                    .into_iter()
                    .filter(|line| separated(line))
                    .collect::<Vec<_>>();
                if lines.is_empty() {
                    return Some(OutlineRequest::Missing(
                        "There is no selection edge to stroke.",
                    ));
                }
                lines
            }
        };
        Some(OutlineRequest::Ready {
            layer: layer.id,
            lines,
            color: solid_color(self.colors[0]),
            brush: self.brush,
        })
    }
}

fn separated(points: &[(f64, f64)]) -> bool {
    points.windows(2).any(|pair| {
        let span = (pair[0].0 - pair[1].0).hypot(pair[0].1 - pair[1].1);
        span >= 1e-6 && span.is_finite()
    })
}
