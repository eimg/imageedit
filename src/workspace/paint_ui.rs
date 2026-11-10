use std::sync::Arc;

use gpui::{Context, Pixels, Point, RenderImage, Window};
use image::RgbaImage;

use super::Workspace;
use super::layers::with_layer_opacity;
use crate::commands::Tool;
use crate::document::{self, LayerId, solid_color};
use crate::paint::{Stroke, StrokeKind};
use crate::preview::PixelRect;
use crate::selection::Selection;

pub(super) struct StrokeSession {
    layer: LayerId,
    offset: (i32, i32),
    opacity: u8,
    source: Arc<RgbaImage>,
    selection: Selection,
    canvas: (u32, u32),
    stroke: Stroke,
    last_point: (f64, f64),
    below: Option<Arc<RenderImage>>,
    above: Option<Arc<RenderImage>>,
    patch: Option<Arc<RenderImage>>,
}

#[derive(Clone)]
pub(super) struct StrokePaint {
    pub rect: PixelRect,
    pub patch: Arc<RenderImage>,
    pub below: Option<Arc<RenderImage>>,
    pub above: Option<Arc<RenderImage>>,
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
        match field {
            BrushField::Size => self.brush.diameter,
            BrushField::Spacing => u32::from(self.brush.spacing),
            BrushField::Hardness => u32::from(self.brush.hardness),
            BrushField::Opacity => u32::from(self.brush.opacity),
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
        match field {
            BrushField::Size => self.brush.diameter = next,
            BrushField::Spacing => self.brush.spacing = next as u8,
            BrushField::Hardness => self.brush.hardness = next as u8,
            BrushField::Opacity => self.brush.opacity = next as u8,
        }
        self.brush = self.brush.clamped();
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
        let Some(kind) = stroke_kind(self.tool) else {
            return;
        };
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
        let opacity = layer.opacity;
        let source = layer.pixels.clone();
        let canvas = tab.document.content.size();
        let selection = tab.selection.clone();
        let color = tab
            .document
            .content
            .mode()
            .constrain(solid_color(self.colors[0]));
        let (below, above) = tab.document.content.composite_around(id);
        let mut session = StrokeSession {
            layer: id,
            offset,
            opacity,
            source,
            selection,
            canvas,
            stroke: Stroke::new(kind, color, self.brush),
            last_point: local_point(point, offset),
            below: below.as_ref().map(document::preview_render),
            above: above.as_ref().map(document::preview_render),
            patch: None,
        };
        if shift
            && let Some((anchor_layer, x, y)) = self.stroke_anchor
            && anchor_layer == id
        {
            session.paint(x, y);
        }
        let here = local_point(point, offset);
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

    pub(super) fn finish_stroke(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.stroke.take() else {
            return;
        };
        self.drop_stroke_images(session.below, session.above, session.patch, window, cx);
        if !session.stroke.changed() || self.active_tab().is_some_and(|tab| tab.busy()) {
            return;
        }
        let layer = session.layer;
        let anchor = session.last_point;
        let stroke = session.stroke;
        self.stroke_anchor = Some((layer, anchor.0, anchor.1));
        self.edit_content(
            move |content, _, _| content.apply_stroke(layer, &stroke),
            window,
            cx,
        );
    }

    pub(super) fn cancel_stroke(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.stroke.take() else {
            return;
        };
        self.drop_stroke_images(session.below, session.above, session.patch, window, cx);
        cx.notify();
    }

    pub(super) fn stroke_paint(&self) -> Option<StrokePaint> {
        let session = self.stroke.as_ref()?;
        let patch = session.patch.clone()?;
        let pixels = session.stroke.pixels()?;
        let origin = session.stroke.origin();
        Some(StrokePaint {
            rect: PixelRect::new(
                i64::from(session.offset.0) + i64::from(origin.0),
                i64::from(session.offset.1) + i64::from(origin.1),
                pixels.width(),
                pixels.height(),
            ),
            patch,
            below: session.below.clone(),
            above: session.above.clone(),
        })
    }

    fn refresh_stroke_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.stroke.as_mut() else {
            return;
        };
        if !session.stroke.changed() {
            cx.notify();
            return;
        }
        let Some(pixels) = session.stroke.pixels() else {
            return;
        };
        let shown = with_layer_opacity(pixels, session.opacity);
        let image = document::preview_render(&shown);
        if let Some(previous) = session.patch.replace(image) {
            super::drop_images(std::iter::once(previous), window, cx);
        }
        cx.notify();
    }

    fn drop_stroke_images(
        &mut self,
        below: Option<Arc<RenderImage>>,
        above: Option<Arc<RenderImage>>,
        patch: Option<Arc<RenderImage>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        super::drop_images(below.into_iter().chain(above).chain(patch), window, cx);
    }
}

impl StrokeSession {
    fn paint(&mut self, x: f64, y: f64) {
        let selection = self.selection.clone();
        let offset = self.offset;
        let canvas = self.canvas;
        let source = self.source.clone();
        self.stroke.sample(x, y, &source, |px, py| {
            selection.coverage(
                i64::from(offset.0) + i64::from(px),
                i64::from(offset.1) + i64::from(py),
                canvas.0,
                canvas.1,
            )
        });
    }
}

fn stroke_kind(tool: Tool) -> Option<StrokeKind> {
    match tool {
        Tool::Pencil | Tool::Brush => Some(StrokeKind::Paint),
        Tool::Eraser => Some(StrokeKind::Erase),
        _ => None,
    }
}

fn local_point(canvas: (f64, f64), offset: (i32, i32)) -> (f64, f64) {
    (
        canvas.0 - f64::from(offset.0),
        canvas.1 - f64::from(offset.1),
    )
}
