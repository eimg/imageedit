use std::{collections::BTreeMap, sync::Arc};

use gpui::{Context, Pixels, Point, RenderImage, Window};
use image::RgbaImage;

use super::Workspace;
use crate::commands::Tool;
use crate::document::{self, LayerId, solid_color};
use crate::paint::{Stroke, StrokeKind, TileKey};
use crate::preview::PixelRect;
use crate::selection::Selection;

pub(super) struct StrokeSession {
    layer: LayerId,
    offset: (i32, i32),
    source: Arc<RgbaImage>,
    selection: Selection,
    canvas: (u32, u32),
    stroke: Stroke,
    last_point: (f64, f64),
    patches: BTreeMap<TileKey, StrokePaint>,
}

#[derive(Clone)]
pub(super) struct StrokePaint {
    pub content: PixelRect,
    pub stored: PixelRect,
    pub pixels: Arc<RgbaImage>,
    pub image: Arc<RenderImage>,
}

#[derive(Clone, Copy)]
pub(super) enum BrushField {
    Size,
    Spacing,
    Hardness,
    Opacity,
}

impl Workspace {
    pub(super) fn brush_value(&self, field: BrushField) -> u32 {
        let brush = self.active_brush();
        match field {
            BrushField::Size => brush.diameter,
            BrushField::Spacing => u32::from(brush.spacing),
            BrushField::Hardness => u32::from(brush.hardness),
            BrushField::Opacity => u32::from(brush.opacity),
        }
    }

    pub(super) fn nudge_brush(&mut self, field: BrushField, delta: i32, cx: &mut Context<Self>) {
        let current = self.brush_value(field) as i32;
        let (min, max) = match field {
            BrushField::Size => (1, 500),
            BrushField::Spacing => (1, 100),
            BrushField::Hardness | BrushField::Opacity => (0, 100),
        };
        let next = (current + delta).clamp(min, max) as u32;
        let brush = if self.tool == Tool::Pencil {
            &mut self.pencil
        } else {
            &mut self.brush
        };
        match field {
            BrushField::Size => brush.diameter = next,
            BrushField::Spacing => brush.spacing = next as u8,
            BrushField::Hardness => brush.hardness = next as u8,
            BrushField::Opacity => brush.opacity = next as u8,
        }
        *brush = brush.clamped();
        cx.notify();
    }

    pub(super) fn adjust_brush_size(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.nudge_brush(BrushField::Size, delta, cx);
    }

    pub(super) fn set_clone_source(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some((x, y)) = self.canvas_pixel(position, scale, false) else {
            return;
        };
        self.clone_source = Some((x, y));
        self.notice("Clone source set.", cx);
    }

    pub(super) fn begin_stroke(
        &mut self,
        position: Point<Pixels>,
        shift: bool,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.stroke.is_some() || self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        if self.refuse_text_pixels(false, cx) {
            return;
        }
        let Some(kind) = stroke_kind(self.tool) else {
            return;
        };
        if self.canvas_pixel(position, scale, false).is_none() {
            return;
        }
        let Some(point) = self.canvas_point(position, scale) else {
            return;
        };
        let Some(tab) = self.active_tab() else {
            return;
        };
        let Some(layer) = tab.document.content.layer(tab.active_layer) else {
            return;
        };
        if !layer.visible {
            self.notice("The active layer is hidden. Show it before painting.", cx);
            return;
        }
        let id = layer.id;
        let offset = layer.offset;
        let source = layer.pixels.clone();
        let canvas = tab.document.content.size();
        let selection = tab.selection.clone();
        let color = tab
            .document
            .content
            .mode()
            .constrain(solid_color(self.colors[0]));
        let here = local_point(point, offset);
        let kind = if matches!(kind, StrokeKind::Clone { .. }) {
            let Some(source_point) = self.clone_source else {
                self.notice("Alt-click to set the clone source.", cx);
                return;
            };
            clone_kind(source_point, offset, here)
        } else {
            kind
        };
        let mut session = StrokeSession {
            layer: id,
            offset,
            source,
            selection,
            canvas,
            stroke: Stroke::new(kind, color, *self.active_brush()),
            last_point: here,
            patches: BTreeMap::new(),
        };
        if shift
            && let Some((anchor_layer, x, y)) = tab.stroke_anchor
            && anchor_layer == id
        {
            session.paint(x, y);
        }
        session.paint(here.0, here.1);
        session.last_point = here;
        self.stroke = Some(session);
        self.refresh_stroke_patch(window, cx);
    }

    pub(super) fn extend_stroke(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.stroke.is_none() {
            return;
        }
        let Some(point) = self.canvas_point(position, scale) else {
            return;
        };
        let Some(session) = self.stroke.as_mut() else {
            return;
        };
        let here = local_point(point, session.offset);
        if (here.0 - session.last_point.0).hypot(here.1 - session.last_point.1) < 1e-3 {
            return;
        }
        session.paint(here.0, here.1);
        session.last_point = here;
        self.refresh_stroke_patch(window, cx);
    }

    pub(super) fn finish_stroke(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_id) = self
            .active_tab()
            .filter(|tab| !tab.busy())
            .map(|tab| tab.id)
        else {
            self.cancel_stroke(window, cx);
            return;
        };
        let point = self.canvas_point(position, window.scale_factor());
        let Some(session) = self.stroke.as_mut() else {
            return;
        };
        if let Some(point) = point {
            session.last_point = local_point(point, session.offset);
            session.paint(session.last_point.0, session.last_point.1);
        }
        session.stroke.finish(&session.source, |x, y| {
            selection_coverage(&session.selection, session.offset, session.canvas, x, y)
        });
        if !session.stroke.changed() {
            self.cancel_stroke(window, cx);
            return;
        }
        self.refresh_stroke_patch(window, cx);
        let session = self.stroke.take().unwrap();
        let layer = session.layer;
        let end = session.last_point;
        let patches = session.patches.into_values().collect();
        let stroke = session.stroke;
        self.start_change(
            super::Change::Stroke { layer, end },
            move |content, _, _| content.apply_stroke(layer, &stroke),
            window,
            cx,
        );
        self.stroke_commit_preview = Some((tab_id, patches));
    }

    pub(super) fn cancel_stroke(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.stroke.take() {
            super::drop_images(
                session.patches.into_values().map(|patch| patch.image),
                window,
                cx,
            );
            cx.notify();
        }
        self.clear_stroke_commit_preview(window, cx);
    }

    pub(super) fn clear_stroke_commit_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, patches)) = self.stroke_commit_preview.take() {
            super::drop_images(patches.into_iter().map(|patch| patch.image), window, cx);
        }
    }

    pub(super) fn stroke_paint(&self) -> Vec<StrokePaint> {
        if let Some(session) = &self.stroke {
            return session.patches.values().cloned().collect();
        }
        self.stroke_commit_preview
            .as_ref()
            .map_or_else(Vec::new, |(_, patches)| patches.clone())
    }

    fn refresh_stroke_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.active else {
            return;
        };
        let content = &self.tabs[index].document.content;
        let Some(session) = self.stroke.as_mut() else {
            return;
        };
        let changed = session.stroke.take_pending_tiles();
        if changed.is_empty() {
            return;
        }
        let mut dirty = changed.clone();
        // Neighboring tiles can sample the changed tile through their one-pixel apron.
        for (x, y) in changed {
            for row in y.saturating_sub(1)..=y + 1 {
                for column in x.saturating_sub(1)..=x + 1 {
                    if session.patches.contains_key(&(column, row)) {
                        dirty.insert((column, row));
                    }
                }
            }
        }
        let (width, height) = session.canvas;
        for key in dirty {
            let Some(rect) = session.stroke.tile_bounds(key) else {
                continue;
            };
            let Some(rect) = clipped_tile(rect, session.offset, (width, height), 0) else {
                continue;
            };
            let stored = clipped_tile(rect, (0, 0), (width, height), 1).unwrap();
            let pixels =
                Arc::new(content.composite_stroke_tile(session.layer, &session.stroke, stored));
            let image = document::preview_render(&pixels);
            let patch = StrokePaint {
                content: rect,
                stored,
                pixels,
                image,
            };
            if let Some(previous) = session.patches.insert(key, patch) {
                super::drop_images([previous.image], window, cx);
            }
        }
        cx.notify();
    }

    fn active_brush(&self) -> &crate::paint::Brush {
        if self.tool == Tool::Pencil {
            &self.pencil
        } else {
            &self.brush
        }
    }

    pub(super) fn active_brush_settings(&self) -> (u32, u8) {
        let brush = self.active_brush();
        (brush.diameter, brush.hardness)
    }
}

impl StrokeSession {
    fn paint(&mut self, x: f64, y: f64) {
        self.stroke.sample(x, y, &self.source, |px, py| {
            selection_coverage(&self.selection, self.offset, self.canvas, px, py)
        });
    }
}

fn selection_coverage(
    selection: &Selection,
    offset: (i32, i32),
    canvas: (u32, u32),
    x: i32,
    y: i32,
) -> u8 {
    let x = i64::from(offset.0) + i64::from(x);
    let y = i64::from(offset.1) + i64::from(y);
    if (0..i64::from(canvas.0)).contains(&x) && (0..i64::from(canvas.1)).contains(&y) {
        selection.coverage(x, y, canvas.0, canvas.1)
    } else {
        0
    }
}

fn clipped_tile(
    rect: PixelRect,
    offset: (i32, i32),
    canvas: (u32, u32),
    apron: i64,
) -> Option<PixelRect> {
    let dx = i64::from(offset.0);
    let dy = i64::from(offset.1);
    let left = (rect.x + dx - apron).max(0);
    let top = (rect.y + dy - apron).max(0);
    let right = (rect.right() + dx + apron).min(i64::from(canvas.0));
    let bottom = (rect.bottom() + dy + apron).min(i64::from(canvas.1));
    (left < right && top < bottom)
        .then(|| PixelRect::new(left, top, (right - left) as u32, (bottom - top) as u32))
}

fn stroke_kind(tool: Tool) -> Option<StrokeKind> {
    match tool {
        Tool::Pencil => Some(StrokeKind::Pencil),
        Tool::Brush => Some(StrokeKind::Paint),
        Tool::Eraser => Some(StrokeKind::Erase),
        Tool::Blur => Some(StrokeKind::Blur),
        Tool::Sharpen => Some(StrokeKind::Sharpen),
        Tool::CloneStamp => Some(StrokeKind::Clone { dx: 0, dy: 0 }),
        _ => None,
    }
}

fn local_point(canvas: (f64, f64), offset: (i32, i32)) -> (f64, f64) {
    (
        canvas.0 - f64::from(offset.0),
        canvas.1 - f64::from(offset.1),
    )
}

fn clone_kind(source: (u32, u32), offset: (i32, i32), here: (f64, f64)) -> StrokeKind {
    StrokeKind::Clone {
        dx: i64::from(source.0) - i64::from(offset.0) - here.0.floor() as i64,
        dy: i64::from(source.1) - i64::from(offset.1) - here.1.floor() as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloning_at_a_pixel_center_samples_the_chosen_source_on_an_offset_layer() {
        let mut source = RgbaImage::new(5, 3);
        let ink = image::Rgba([220, 30, 40, 255]);
        source.put_pixel(4, 2, ink);
        let offset = (-2, 3);
        let mut stroke = Stroke::new(
            clone_kind((2, 5), offset, (0.5, 0.5)),
            ink,
            crate::paint::Brush {
                diameter: 1,
                ..Default::default()
            },
        );
        stroke.sample(0.5, 0.5, &source, |_, _| 255);
        let (painted, _) = crate::paint::apply_stroke(&source, &stroke).unwrap();
        assert_eq!(*painted.get_pixel(0, 0), ink);
    }
}
