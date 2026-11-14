use super::canvas_tools::PreviewGeometry;
use super::{Tab, Workspace};
use crate::commands::Tool;
use crate::guides::{Axis, Guide, MAX_GUIDES};
use crate::preview::Dirty;
use gpui::{
    App, Bounds, Context, CursorStyle, Pixels, Point, Window, fill, point, px, rgb, rgba, size,
};

pub(super) const RULER_SIZE: f32 = 28.;
const HIT_SLOP: f32 = 5.;

pub(super) fn canvas_viewport(mut bounds: Bounds<Pixels>, rulers: bool) -> Bounds<Pixels> {
    if rulers {
        bounds.origin += point(px(RULER_SIZE), px(RULER_SIZE));
        bounds.size.width = (bounds.size.width - px(RULER_SIZE)).max(px(0.));
        bounds.size.height = (bounds.size.height - px(RULER_SIZE)).max(px(0.));
    }
    bounds
}

pub(super) struct GuideDrag {
    tab: usize,
    generation: u64,
    index: Option<usize>,
    axis: Axis,
    grab: f32,
    target: Option<Guide>,
}

impl GuideDrag {
    fn belongs_to(&self, tab: &Tab) -> bool {
        self.tab == tab.id && self.generation == tab.history.generation() && !tab.busy()
    }

    fn apply(&self, source: &[Guide]) -> Vec<Guide> {
        let mut guides = source.to_vec();
        if let Some(index) = self.index {
            if index >= guides.len() {
                return guides;
            }
            if let Some(target) = self.target {
                guides[index] = target;
            } else {
                guides.remove(index);
            }
        } else if let Some(target) = self.target
            && guides.len() < MAX_GUIDES
        {
            guides.push(target);
        }
        guides
    }
}

pub(super) fn ruler_hit(viewport: Bounds<Pixels>, position: Point<Pixels>) -> bool {
    let outer = Bounds::new(
        viewport.origin - point(px(RULER_SIZE), px(RULER_SIZE)),
        viewport.size + size(px(RULER_SIZE), px(RULER_SIZE)),
    );
    outer.contains(&position) && (position.x < viewport.left() || position.y < viewport.top())
}

fn ruler_axis(viewport: Bounds<Pixels>, position: Point<Pixels>) -> Option<Axis> {
    if position.x >= viewport.left()
        && position.x < viewport.right()
        && position.y >= viewport.top() - px(RULER_SIZE)
        && position.y < viewport.top()
    {
        Some(Axis::Horizontal)
    } else if position.y >= viewport.top()
        && position.y < viewport.bottom()
        && position.x >= viewport.left() - px(RULER_SIZE)
        && position.x < viewport.left()
    {
        Some(Axis::Vertical)
    } else {
        None
    }
}

fn guide_at(
    guides: &[Guide],
    geometry: &PreviewGeometry,
    canvas: (u32, u32),
    position: Point<Pixels>,
) -> Option<usize> {
    guides
        .iter()
        .enumerate()
        .filter_map(|(index, guide)| {
            let projected = match guide.axis {
                Axis::Horizontal => {
                    geometry
                        .project(point(0., guide.position as f32 / canvas.1 as f32))
                        .y
                }
                Axis::Vertical => {
                    geometry
                        .project(point(guide.position as f32 / canvas.0 as f32, 0.))
                        .x
                }
            };
            let distance = f32::from(
                projected
                    - match guide.axis {
                        Axis::Horizontal => position.y,
                        Axis::Vertical => position.x,
                    },
            )
            .abs();
            (distance <= HIT_SLOP).then_some((distance, index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, index)| index)
}

impl Workspace {
    pub(super) fn displayed_guides(&self) -> Vec<Guide> {
        let Some(tab) = self.active_tab() else {
            return Vec::new();
        };
        let guides = tab
            .pending_guides
            .as_deref()
            .unwrap_or_else(|| tab.document.content.guides());
        self.guide_drag
            .as_ref()
            .filter(|drag| drag.belongs_to(tab))
            .map_or_else(|| guides.to_vec(), |drag| drag.apply(guides))
    }

    pub(super) fn begin_guide(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.preferences.show_guides
            || self.preferences.tool != Tool::Move
            || self.space_pan
            || self.move_drag.is_some()
            || self.numeric_edit.is_some()
            || self.guide_drag.is_some()
            || self.form.is_some()
            || self.dialog_open
        {
            return false;
        }
        let Some(tab) = self.active_tab().filter(|tab| !tab.busy()) else {
            return false;
        };
        let viewport = self.viewport.get();
        let Some(geometry) = PreviewGeometry::for_tab(tab, viewport, scale) else {
            return false;
        };
        let guides = tab.document.content.guides();
        let (index, axis) = if let Some(axis) = ruler_axis(viewport, position) {
            if guides.len() >= MAX_GUIDES {
                self.notice("The image guide limit has been reached.", cx);
                return true;
            }
            (None, axis)
        } else if viewport.contains(&position)
            && geometry.bounds.contains(&position)
            && let Some(index) = guide_at(guides, &geometry, tab.document.content.size(), position)
        {
            (Some(index), guides[index].axis)
        } else {
            return false;
        };
        let fraction = geometry.fraction(position);
        let pointer = match axis {
            Axis::Horizontal => fraction.y * tab.document.height() as f32,
            Axis::Vertical => fraction.x * tab.document.width() as f32,
        };
        self.guide_drag = Some(GuideDrag {
            tab: tab.id,
            generation: tab.history.generation(),
            index,
            axis,
            grab: index.map_or(0., |index| guides[index].position as f32 - pointer),
            target: index.map(|index| guides[index]),
        });
        cx.notify();
        true
    }

    pub(super) fn extend_guide(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.guide_drag.as_ref() else {
            return;
        };
        let Some(tab) = self.active_tab().filter(|tab| drag.belongs_to(tab)) else {
            self.cancel_guide(cx);
            return;
        };
        let Some(geometry) = PreviewGeometry::for_tab(tab, self.viewport.get(), scale) else {
            return;
        };
        let fraction = geometry.fraction(position);
        let (value, limit) = match drag.axis {
            Axis::Horizontal => (
                fraction.y * tab.document.height() as f32 + drag.grab,
                tab.document.height(),
            ),
            Axis::Vertical => (
                fraction.x * tab.document.width() as f32 + drag.grab,
                tab.document.width(),
            ),
        };
        let target = (self.viewport.get().contains(&position)
            && value.is_finite()
            && value >= -0.5
            && value <= limit as f32 + 0.5)
            .then_some(Guide {
                axis: drag.axis,
                position: value.round().clamp(0., limit as f32) as u32,
            });
        if self.guide_drag.as_ref().unwrap().target != target {
            self.guide_drag.as_mut().unwrap().target = target;
            cx.notify();
        }
    }

    pub(super) fn finish_guide(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.extend_guide(position, window.scale_factor(), cx);
        let Some(drag) = self.guide_drag.take() else {
            return;
        };
        let Some(tab) = self.active_tab().filter(|tab| drag.belongs_to(tab)) else {
            cx.notify();
            return;
        };
        let guides = drag.apply(tab.document.content.guides());
        if guides == tab.document.content.guides() {
            cx.notify();
            return;
        }
        let pending = guides.clone();
        self.edit_content(
            move |content, _, _| {
                if content.set_guides(guides).is_ok() {
                    Dirty::Unpainted
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
        if let Some(tab) = self
            .active
            .and_then(|index| self.tabs.get_mut(index))
            .filter(|tab| tab.id == drag.tab && tab.editing)
        {
            tab.pending_guides = Some(pending);
        }
        cx.notify();
    }

    pub(super) fn cancel_guide(&mut self, cx: &mut Context<Self>) {
        if self.guide_drag.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn guide_cursor(&self, scale: f32) -> Option<CursorStyle> {
        if !self.preferences.show_guides {
            return None;
        }
        if let Some(drag) = &self.guide_drag {
            return Some(match drag.axis {
                Axis::Horizontal => CursorStyle::ResizeUpDown,
                Axis::Vertical => CursorStyle::ResizeLeftRight,
            });
        }
        let position = self.canvas_cursor.position.get()?;
        let viewport = self.viewport.get();
        let ruler = ruler_axis(viewport, position);
        if ruler_hit(viewport, position) && (self.preferences.tool != Tool::Move || ruler.is_none())
        {
            return Some(CursorStyle::Arrow);
        }
        if self.preferences.tool != Tool::Move {
            return None;
        }
        let tab = self.active_tab()?;
        let geometry = PreviewGeometry::for_tab(tab, viewport, scale)?;
        let axis = ruler.or_else(|| {
            (viewport.contains(&position) && geometry.bounds.contains(&position))
                .then(|| {
                    guide_at(
                        tab.document.content.guides(),
                        &geometry,
                        tab.document.content.size(),
                        position,
                    )
                })
                .flatten()
                .map(|index| tab.document.content.guides()[index].axis)
        })?;
        Some(match axis {
            Axis::Horizontal => CursorStyle::ResizeUpDown,
            Axis::Vertical => CursorStyle::ResizeLeftRight,
        })
    }
}

pub(super) fn paint_guides(
    window: &mut Window,
    geometry: &PreviewGeometry,
    visible: Bounds<Pixels>,
    canvas: (u32, u32),
    guides: &[Guide],
) {
    for guide in guides {
        let (mut start, extent) = match guide.axis {
            Axis::Horizontal => (
                point(
                    visible.left(),
                    geometry
                        .project(point(0., guide.position as f32 / canvas.1 as f32))
                        .y,
                ),
                size(visible.size.width, px(1.)),
            ),
            Axis::Vertical => (
                point(
                    geometry
                        .project(point(guide.position as f32 / canvas.0 as f32, 0.))
                        .x,
                    visible.top(),
                ),
                size(px(1.), visible.size.height),
            ),
        };
        if guide.axis == Axis::Horizontal && start.y == visible.bottom() {
            start.y -= px(1.);
        }
        if guide.axis == Axis::Vertical && start.x == visible.right() {
            start.x -= px(1.);
        }
        let line = Bounds::new(start, extent).intersect(&visible);
        if !line.is_empty() {
            window.paint_quad(fill(line, rgba(0x69d4ffb0)));
        }
    }
}

pub(super) fn paint_rulers(
    window: &mut Window,
    cx: &mut App,
    viewport: Bounds<Pixels>,
    geometry: &PreviewGeometry,
    canvas: (u32, u32),
) {
    let top = Bounds::new(
        point(
            viewport.left() - px(RULER_SIZE),
            viewport.top() - px(RULER_SIZE),
        ),
        size(viewport.size.width + px(RULER_SIZE), px(RULER_SIZE)),
    );
    let left = Bounds::new(
        point(viewport.left() - px(RULER_SIZE), viewport.top()),
        size(px(RULER_SIZE), viewport.size.height),
    );
    window.paint_quad(fill(top, rgb(super::view::CHROME)));
    window.paint_quad(fill(left, rgb(super::view::CHROME)));
    for horizontal in [true, false] {
        let (origin, extent, pixels, visible_start, visible_end) = if horizontal {
            (
                geometry.bounds.left(),
                canvas.0,
                geometry.bounds.size.width,
                viewport.left(),
                viewport.right(),
            )
        } else {
            (
                geometry.bounds.top(),
                canvas.1,
                geometry.bounds.size.height,
                viewport.top(),
                viewport.bottom(),
            )
        };
        let factor = f32::from(pixels) / extent as f32;
        let step = ruler_step(factor);
        let first =
            (((f32::from(visible_start - origin) / factor).max(0.) / step).ceil() * step) as u32;
        let end = ((f32::from(visible_end - origin) / factor)
            .min(extent as f32)
            .max(0.)) as u32;
        let tick_step = (step as u32).max(1);
        for value in (first..=end).step_by(tick_step as usize) {
            let at = origin + px(value as f32 * factor);
            let major = value % (tick_step * 5) == 0;
            let length = if major { 7. } else { 3. };
            let bounds = if horizontal {
                Bounds::new(
                    point(at, viewport.top() - px(length)),
                    size(px(1.), px(length)),
                )
            } else {
                Bounds::new(
                    point(viewport.left() - px(length), at),
                    size(px(length), px(1.)),
                )
            };
            window.paint_quad(fill(bounds, rgb(super::view::MUTED)));
            if major {
                let text = value.to_string();
                let run = gpui::TextRun {
                    len: text.len(),
                    font: window.text_style().font(),
                    color: rgb(super::view::MUTED).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window
                    .text_system()
                    .shape_line(text.into(), px(8.), &[run], None);
                let point = if horizontal {
                    point(at + px(3.), viewport.top() - px(RULER_SIZE - 2.))
                } else {
                    point(viewport.left() - px(RULER_SIZE - 1.), at + px(2.))
                };
                let clip = if horizontal { top } else { left };
                window.with_content_mask(Some(gpui::ContentMask { bounds: clip }), |window| {
                    let _ = line.paint(point, px(11.), window, cx);
                });
            }
        }
    }
}

fn ruler_step(factor: f32) -> f32 {
    let wanted = 14. / factor.max(0.000001);
    let magnitude = 10f32.powf(wanted.max(1.).log10().floor());
    [1., 2., 5., 10.]
        .into_iter()
        .map(|step| step * magnitude)
        .find(|step| *step >= wanted)
        .unwrap_or(magnitude * 10.)
        .max(1.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Zoom;

    #[test]
    fn ruler_regions_and_guide_hits_share_the_panned_canvas_geometry() {
        let outer = Bounds::new(point(px(10.), px(20.)), size(px(228.), px(188.)));
        let viewport = canvas_viewport(outer, true);
        assert_eq!(viewport.origin, point(px(38.), px(48.)));
        assert_eq!(viewport.size, size(px(200.), px(160.)));
        assert_eq!(
            ruler_axis(viewport, point(px(100.), px(30.))),
            Some(Axis::Horizontal)
        );
        assert_eq!(
            ruler_axis(viewport, point(px(20.), px(100.))),
            Some(Axis::Vertical)
        );
        assert!(ruler_hit(viewport, point(px(20.), px(30.))));
        assert!(!ruler_hit(viewport, viewport.center()));
        let geometry = PreviewGeometry::new(400, 300, Zoom::Percent(200.), viewport, 2.)
            .unwrap()
            .panned(point(px(50.), px(20.)));
        let guides = [
            Guide {
                axis: Axis::Vertical,
                position: 200,
            },
            Guide {
                axis: Axis::Horizontal,
                position: 150,
            },
        ];
        let center = geometry.project(point(0.5, 0.5));
        assert_eq!(guide_at(&guides, &geometry, (400, 300), center), Some(0));
        assert_eq!(
            guide_at(
                &guides,
                &geometry,
                (400, 300),
                center + point(px(20.), px(4.))
            ),
            Some(1)
        );
        assert_eq!(
            guide_at(
                &guides,
                &geometry,
                (400, 300),
                center + point(px(20.), px(20.))
            ),
            None
        );
        for factor in [0.01, 0.5, 1., 8.] {
            assert!(ruler_step(factor) * factor >= 14.);
        }
    }

    #[test]
    fn guide_drafts_are_reversible_bounded_and_scoped_to_their_revision() {
        let mut tab = super::super::tests::opacity_tab();
        let source = [Guide {
            axis: Axis::Vertical,
            position: 1,
        }];
        let mut drag = GuideDrag {
            tab: tab.id,
            generation: tab.history.generation(),
            index: None,
            axis: Axis::Horizontal,
            grab: 0.,
            target: Some(Guide {
                axis: Axis::Horizontal,
                position: 2,
            }),
        };
        assert!(drag.belongs_to(&tab));
        assert_eq!(drag.apply(&source).len(), 2);
        assert_eq!(source.len(), 1);
        drag.index = Some(0);
        assert_eq!(drag.apply(&source), vec![drag.target.unwrap()]);
        drag.target = None;
        assert!(drag.apply(&source).is_empty());
        drag.index = None;
        assert_eq!(drag.apply(&source), source);
        drag.target = Some(source[0]);
        assert_eq!(drag.apply(&vec![source[0]; MAX_GUIDES]).len(), MAX_GUIDES);
        tab.editing = true;
        assert!(!drag.belongs_to(&tab));
        tab.editing = false;
        drag.generation += 1;
        assert!(!drag.belongs_to(&tab));
    }

    #[test]
    fn guide_commits_reuse_pixels_and_undo_back_to_the_saved_state() {
        let mut tab = super::super::tests::opacity_tab();
        let pixels = tab.document.canvas.pixels.clone();
        let guides = vec![Guide {
            axis: Axis::Horizontal,
            position: 1,
        }];
        let generation = tab.history.generation();
        let mut state = tab.state();
        state.content.set_guides(guides.clone()).unwrap();
        tab.pending_guides = Some(guides.clone());
        assert!(tab.document.content.guides().is_empty());
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            Dirty::Unpainted,
        )
        .unwrap();
        tab.editing = true;
        assert!(
            tab.publish_change(
                generation,
                super::super::Change::Edit,
                Some((state.content, state.active_layer, state.selection, patch)),
                1
            )
            .0
        );
        assert_eq!(tab.document.content.guides(), guides);
        assert!(tab.pending_guides.is_none());
        assert!(std::sync::Arc::ptr_eq(&pixels, &tab.document.canvas.pixels));
        assert!(tab.history.is_dirty());
        let state = tab.history.undo_target().unwrap().0.clone();
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            Dirty::Unpainted,
        )
        .unwrap();
        let generation = tab.history.generation();
        assert!(
            tab.publish_change(
                generation,
                super::super::Change::Undo,
                Some((state.content, state.active_layer, state.selection, patch)),
                2
            )
            .0
        );
        assert!(tab.document.content.guides().is_empty());
        assert!(!tab.history.is_dirty());
    }
}
