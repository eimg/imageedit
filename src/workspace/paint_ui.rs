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
    clone_delta: Option<(i64, i64)>,
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
    Flow,
}

pub(super) struct CloneSource {
    tab: usize,
    layer: LayerId,
    point: (u32, u32),
    offset: (i32, i32),
    size: (u32, u32),
    delta: Option<(i64, i64)>,
    pending: Option<(u64, (i64, i64))>,
}

impl CloneSource {
    fn settle(&mut self, tab: usize, generation: u64, published: bool) {
        if tab != self.tab || self.pending.is_none_or(|pending| pending.0 != generation) {
            return;
        }
        let (_, delta) = self.pending.take().unwrap();
        if published {
            self.delta = Some(delta);
        }
    }

    fn kind(
        &self,
        tab: usize,
        layer: LayerId,
        offset: (i32, i32),
        size: (u32, u32),
        here: (f64, f64),
        aligned: bool,
    ) -> Option<StrokeKind> {
        if (self.tab, self.layer, self.offset, self.size) != (tab, layer, offset, size) {
            return None;
        }
        if aligned && let Some((dx, dy)) = self.delta {
            return Some(StrokeKind::Clone { dx, dy });
        }
        Some(clone_kind(self.point, offset, here))
    }
}

pub(super) struct BrushSettings {
    brush: crate::paint::Brush,
    pencil: crate::paint::Brush,
    eraser: crate::paint::Brush,
    blur: crate::paint::Brush,
    sharpen: crate::paint::Brush,
    clone_stamp: crate::paint::Brush,
    pub(super) outline: crate::paint::Brush,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            brush: Default::default(),
            pencil: crate::paint::Brush {
                diameter: 1,
                ..Default::default()
            },
            eraser: Default::default(),
            blur: Default::default(),
            sharpen: Default::default(),
            clone_stamp: Default::default(),
            outline: Default::default(),
        }
    }
}

impl BrushSettings {
    fn get(&self, tool: Tool) -> &crate::paint::Brush {
        match tool {
            Tool::Pencil => &self.pencil,
            Tool::Eraser => &self.eraser,
            Tool::Blur => &self.blur,
            Tool::Sharpen => &self.sharpen,
            Tool::CloneStamp => &self.clone_stamp,
            Tool::Path => &self.outline,
            _ => &self.brush,
        }
    }

    fn get_mut(&mut self, tool: Tool) -> &mut crate::paint::Brush {
        match tool {
            Tool::Pencil => &mut self.pencil,
            Tool::Eraser => &mut self.eraser,
            Tool::Blur => &mut self.blur,
            Tool::Sharpen => &mut self.sharpen,
            Tool::CloneStamp => &mut self.clone_stamp,
            Tool::Path => &mut self.outline,
            _ => &mut self.brush,
        }
    }
}

impl Workspace {
    pub(super) fn brush_value(&self, field: BrushField) -> u32 {
        let brush = self.active_brush();
        match field {
            BrushField::Size => brush.diameter,
            BrushField::Spacing => u32::from(brush.spacing),
            BrushField::Hardness => u32::from(brush.hardness),
            BrushField::Opacity => u32::from(brush.opacity),
            BrushField::Flow => u32::from(brush.flow),
        }
    }

    pub(super) fn nudge_brush(&mut self, field: BrushField, delta: i32, cx: &mut Context<Self>) {
        let current = self.brush_value(field) as i32;
        self.set_brush_value(field, (current + delta).max(0) as u32, cx);
    }

    pub(super) fn set_brush_value(
        &mut self,
        field: BrushField,
        value: u32,
        cx: &mut Context<Self>,
    ) {
        let (min, max) = match field {
            BrushField::Size => (1, 500),
            BrushField::Spacing => (1, 100),
            BrushField::Hardness | BrushField::Opacity | BrushField::Flow => (0, 100),
        };
        let next = value.clamp(min, max);
        let brush = self.brushes.get_mut(self.tool);
        match field {
            BrushField::Size => brush.diameter = next,
            BrushField::Spacing => brush.spacing = next as u8,
            BrushField::Hardness => brush.hardness = next as u8,
            BrushField::Opacity => brush.opacity = next as u8,
            BrushField::Flow => brush.flow = next as u8,
        }
        *brush = brush.clamped();
        cx.notify();
    }

    pub(super) fn adjust_brush_size(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.nudge_brush(BrushField::Size, delta, cx);
    }

    pub(super) fn brush_buildup(&self) -> bool {
        self.active_brush().buildup
    }

    pub(super) fn set_brush_buildup(&mut self, buildup: bool, cx: &mut Context<Self>) {
        self.brushes.get_mut(self.tool).buildup = buildup;
        cx.notify();
    }

    pub(super) fn set_clone_source(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        if self.stroke.is_some() {
            return;
        }
        let Some((x, y)) = self.canvas_pixel(position, scale, false) else {
            return;
        };
        let Some(tab) = self.active_tab().filter(|tab| !tab.busy()) else {
            return;
        };
        let Some(layer) = tab.document.content.layer(tab.active_layer) else {
            return;
        };
        let local = (
            i64::from(x) - i64::from(layer.offset.0),
            i64::from(y) - i64::from(layer.offset.1),
        );
        if !layer.visible
            || layer.text.is_some()
            || local.0 < 0
            || local.1 < 0
            || local.0 >= i64::from(layer.pixels.width())
            || local.1 >= i64::from(layer.pixels.height())
        {
            self.notice("Choose a source pixel inside the visible raster layer.", cx);
            return;
        }
        self.clone_source = Some(CloneSource {
            tab: tab.id,
            layer: layer.id,
            point: (x, y),
            offset: layer.offset,
            size: layer.pixels.dimensions(),
            delta: None,
            pending: None,
        });
        self.notice("Clone source set.", cx);
    }

    pub(super) fn set_clone_aligned(&mut self, aligned: bool, cx: &mut Context<Self>) {
        if self.stroke.is_some() || self.clone_aligned == aligned {
            return;
        }
        self.clone_aligned = aligned;
        if let Some(source) = self.clone_source.as_mut() {
            source.delta = None;
            source.pending = None;
        }
        cx.notify();
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
            let Some(kind) = self.clone_source.as_ref().and_then(|source| {
                source.kind(
                    tab.id,
                    id,
                    offset,
                    layer.pixels.dimensions(),
                    here,
                    self.clone_aligned,
                )
            }) else {
                self.notice("Alt-click to set the clone source.", cx);
                return;
            };
            kind
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
            clone_delta: match kind {
                StrokeKind::Clone { dx, dy } if self.clone_aligned => Some((dx, dy)),
                _ => None,
            },
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

    pub(super) fn finish_clone_alignment(&mut self, tab: usize, generation: u64, published: bool) {
        if let Some(source) = self.clone_source.as_mut() {
            source.settle(tab, generation, published);
        }
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
        let generation = self.active_tab().unwrap().history.generation();
        if let Some(delta) = session.clone_delta
            && let Some(source) = self.clone_source.as_mut()
        {
            source.pending = Some((generation, delta));
        }
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
        self.brushes.get(self.tool)
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
    fn aligned_cloning_keeps_the_offset_and_unaligned_cloning_restarts_at_the_source() {
        let pixels = RgbaImage::from_fn(10, 1, |x, _| image::Rgba([x as u8 * 20, 0, 0, 255]));
        let mut source = CloneSource {
            tab: 7,
            layer: LayerId(3),
            point: (6, 0),
            offset: (0, 0),
            size: (10, 1),
            delta: None,
            pending: None,
        };
        let first = source
            .kind(7, LayerId(3), (0, 0), (10, 1), (0.5, 0.5), true)
            .unwrap();
        assert_eq!(first, StrokeKind::Clone { dx: 6, dy: 0 });
        // The first accepted stroke establishes the alignment. Resolving a
        // canceled stroke alone does not change it.
        assert_eq!(source.delta, None);
        source.pending = Some((9, (6, 0)));
        source.settle(7, 9, false);
        assert_eq!(source.delta, None);
        source.pending = Some((9, (6, 0)));
        source.settle(8, 9, true);
        source.settle(7, 8, true);
        assert_eq!(source.delta, None);
        source.settle(7, 9, true);
        assert_eq!(source.delta, Some((6, 0)));
        for (aligned, expected) in [(true, 8), (false, 6)] {
            let kind = source
                .kind(7, LayerId(3), (0, 0), (10, 1), (2.5, 0.5), aligned)
                .unwrap();
            let mut stroke = Stroke::new(
                kind,
                image::Rgba([0; 4]),
                crate::paint::Brush {
                    diameter: 1,
                    ..Default::default()
                },
            );
            stroke.sample(2.5, 0.5, &pixels, |_, _| 255);
            let (painted, _) = crate::paint::apply_stroke(&pixels, &stroke).unwrap();
            assert_eq!(painted.get_pixel(2, 0), pixels.get_pixel(expected, 0));
        }
        assert!(
            source
                .kind(8, LayerId(3), (0, 0), (10, 1), (0.5, 0.5), true)
                .is_none()
        );
        assert!(
            source
                .kind(7, LayerId(4), (0, 0), (10, 1), (0.5, 0.5), true)
                .is_none()
        );
        assert!(
            source
                .kind(7, LayerId(3), (1, 0), (10, 1), (0.5, 0.5), true)
                .is_none()
        );
        assert!(
            source
                .kind(7, LayerId(3), (0, 0), (8, 1), (0.5, 0.5), true)
                .is_none()
        );
    }

    #[test]
    fn paint_tools_and_outline_strokes_retain_independent_settings() {
        let mut settings = BrushSettings::default();
        assert_eq!(settings.get(Tool::Pencil).diameter, 1);
        let tools = [
            Tool::Brush,
            Tool::Pencil,
            Tool::Eraser,
            Tool::Blur,
            Tool::Sharpen,
            Tool::CloneStamp,
            Tool::Path,
        ];
        for (index, tool) in tools.iter().enumerate() {
            let brush = settings.get_mut(*tool);
            brush.diameter = index as u32 + 2;
            brush.opacity = (index as u8 + 1) * 10;
            brush.flow = (index as u8 + 2) * 10;
            brush.buildup = index % 2 == 1;
        }
        for (index, tool) in tools.iter().enumerate() {
            assert_eq!(settings.get(*tool).diameter, index as u32 + 2);
            assert_eq!(settings.get(*tool).opacity, (index as u8 + 1) * 10);
            assert_eq!(settings.get(*tool).flow, (index as u8 + 2) * 10);
            assert_eq!(settings.get(*tool).buildup, index % 2 == 1);
        }
        assert_eq!(settings.outline.diameter, 8);
    }

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
