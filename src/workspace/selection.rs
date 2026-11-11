use gpui::{Action, Context, CursorStyle, Div, Pixels, Point, Window, point, prelude::*};

use super::Workspace;
use super::canvas_tools::PreviewGeometry;
use crate::commands::{
    ClearToTransparency, CropToSelection, InvertSelection, SelectAll, SelectNone, Tool,
};
use crate::document::CropRect;
use crate::preview::{Dirty, PixelRect};
use crate::selection::{Selection, SelectionOp, SelectionShape};

const HANDLE_SLOP: f32 = 6.;
const POLYGON_CLOSE_SLOP: f32 = 8.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MaskEdit {
    Feather,
    Grow,
    Shrink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectionGesture {
    Create {
        origin: Point<Pixels>,
        anchor: Option<(u32, u32)>,
        op: SelectionOp,
    },
    Move {
        rect: PixelRect,
        grab: (i64, i64),
    },
    Resize {
        rect: PixelRect,
        edges: SelectionEdges,
        anchor: (i64, i64),
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectionEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionHit {
    Outside,
    Inside,
    Edge(SelectionEdges),
}

impl Workspace {
    pub(super) fn clear_selection_gesture(&mut self) {
        self.selection_drag = None;
        self.selection_preview = None;
        self.free_points.clear();
        self.free_drag_origin = None;
        self.free_dragging = false;
    }

    pub(super) fn begin_selection(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        op: SelectionOp,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        self.clear_selection_gesture();
        let Some(selection) = self.active_tab().map(|tab| tab.selection.clone()) else {
            return;
        };
        let gesture =
            if let Some(rect) = adjustable_rect(selection).filter(|_| op == SelectionOp::Replace) {
                let hit = self
                    .selection_screen_rect(rect, scale)
                    .map(|(left, top, right, bottom)| {
                        hit_selection(
                            left,
                            top,
                            right,
                            bottom,
                            f32::from(position.x),
                            f32::from(position.y),
                            HANDLE_SLOP,
                        )
                    })
                    .unwrap_or(SelectionHit::Outside);
                match hit {
                    SelectionHit::Edge(edges) => {
                        let Some(anchor) = self.unclamped_pointer_pixel(position, scale) else {
                            return;
                        };
                        SelectionGesture::Resize {
                            rect,
                            edges,
                            anchor,
                        }
                    }
                    SelectionHit::Inside => {
                        let pixel = self
                            .pointer_pixel(position, scale)
                            .unwrap_or((rect.x, rect.y));
                        SelectionGesture::Move {
                            rect,
                            grab: (pixel.0 - rect.x, pixel.1 - rect.y),
                        }
                    }
                    SelectionHit::Outside => SelectionGesture::Create {
                        origin: position,
                        anchor: self.canvas_pixel(position, scale, false),
                        op,
                    },
                }
            } else if let Some(anchor) = self.canvas_pixel(position, scale, false) {
                SelectionGesture::Create {
                    origin: position,
                    anchor: Some(anchor),
                    op,
                }
            } else {
                return;
            };
        self.selection_drag = Some(gesture);
        cx.notify();
    }

    pub(super) fn extend_selection(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.selection_drag else {
            return;
        };
        let Some(tab) = self.active_tab() else {
            return;
        };
        let (width, height) = tab.document.content.size();
        let previous = tab.selection.clone();
        let pointer = self.pointer_pixel(position, scale);
        let preview = match drag {
            SelectionGesture::Create {
                origin,
                anchor: Some(start),
                ..
            } => {
                let delta = position - origin;
                if f32::from(delta.x).abs().max(f32::from(delta.y).abs()) < 4. {
                    None
                } else {
                    let shape = if self.tool == Tool::Select {
                        SelectionShape::Rectangle
                    } else if self.oval_circle {
                        SelectionShape::Circle
                    } else {
                        SelectionShape::Ellipse
                    };
                    let radius = self.selection_radius;
                    pointer.map(|end| {
                        drag_selection(
                            shape,
                            radius,
                            start,
                            (end.0 as u32, end.1 as u32),
                            width,
                            height,
                        )
                    })
                }
            }
            SelectionGesture::Create { anchor: None, .. } => None,
            SelectionGesture::Move { rect, grab } => pointer.map(|pixel| {
                placed_selection(
                    previous,
                    move_rect(rect, pixel, grab, width, height),
                    width,
                    height,
                )
            }),
            SelectionGesture::Resize {
                rect,
                edges,
                anchor,
            } => self.unclamped_pointer_pixel(position, scale).map(|pixel| {
                let delta = (
                    pixel.0.saturating_sub(anchor.0),
                    pixel.1.saturating_sub(anchor.1),
                );
                placed_selection(
                    previous,
                    resize_rect(rect, edges, delta, width, height),
                    width,
                    height,
                )
            }),
        };
        if self.selection_preview != preview {
            self.selection_preview = preview;
            cx.notify();
        }
    }

    pub(super) fn finish_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let gesture = self.selection_drag.take();
        let preview = self.selection_preview.take();
        match (gesture, preview) {
            (Some(SelectionGesture::Create { op, .. }), Some(shape)) => {
                self.update_selection(
                    move |current, (width, height)| current.combine(shape, op, width, height),
                    window,
                    cx,
                );
            }
            // A click outside a replace selection deselects.
            (
                Some(SelectionGesture::Create {
                    op: SelectionOp::Replace,
                    ..
                }),
                None,
            ) => {
                self.select_none(window, cx);
            }
            (Some(_), Some(selection)) => self.commit_selection(selection, window, cx),
            (Some(_), None) => cx.notify(),
            (None, _) => {}
        }
    }

    pub(super) fn selection_cursor(&self, scale: f32) -> CursorStyle {
        if let Some(gesture) = self.selection_drag {
            return match gesture {
                SelectionGesture::Create { .. } => CursorStyle::Crosshair,
                SelectionGesture::Move { .. } => CursorStyle::ClosedHand,
                SelectionGesture::Resize { edges, .. } => edges.cursor(),
            };
        }
        let Some(position) = self.canvas_cursor.position.get() else {
            return CursorStyle::Crosshair;
        };
        let Some(selection) = self.active_tab().map(|tab| tab.selection.clone()) else {
            return CursorStyle::Crosshair;
        };
        let Some(rect) = adjustable_rect(selection) else {
            return CursorStyle::Crosshair;
        };
        let Some((left, top, right, bottom)) = self.selection_screen_rect(rect, scale) else {
            return CursorStyle::Crosshair;
        };
        match hit_selection(
            left,
            top,
            right,
            bottom,
            f32::from(position.x),
            f32::from(position.y),
            HANDLE_SLOP,
        ) {
            SelectionHit::Inside => CursorStyle::OpenHand,
            SelectionHit::Edge(edges) => edges.cursor(),
            SelectionHit::Outside => CursorStyle::Crosshair,
        }
    }

    fn pointer_pixel(&self, position: Point<Pixels>, scale: f32) -> Option<(i64, i64)> {
        let (x, y) = self.unclamped_pointer_pixel(position, scale)?;
        let (width, height) = self.active_document()?.content.size();
        Some((
            x.clamp(0, i64::from(width) - 1),
            y.clamp(0, i64::from(height) - 1),
        ))
    }

    pub(super) fn unclamped_pointer_pixel(
        &self,
        position: Point<Pixels>,
        scale: f32,
    ) -> Option<(i64, i64)> {
        let tab = self.active_tab()?;
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let fraction = geometry.fraction(position);
        Some((
            fraction_pixel(fraction.x, tab.document.width()),
            fraction_pixel(fraction.y, tab.document.height()),
        ))
    }

    fn selection_screen_rect(&self, rect: PixelRect, scale: f32) -> Option<(f32, f32, f32, f32)> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        if width == 0 || height == 0 {
            return None;
        }
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let start = geometry.project(point(
            rect.x as f32 / width as f32,
            rect.y as f32 / height as f32,
        ));
        let end = geometry.project(point(
            rect.right() as f32 / width as f32,
            rect.bottom() as f32 / height as f32,
        ));
        Some((
            f32::from(start.x.min(end.x)),
            f32::from(start.y.min(end.y)),
            f32::from(start.x.max(end.x)),
            f32::from(start.y.max(end.y)),
        ))
    }

    pub(super) fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_selection(Selection::Full, window, cx);
    }

    pub(super) fn select_none(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_selection(Selection::Unrestricted, window, cx);
    }

    pub(super) fn invert_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let (width, height) = tab.document.content.size();
        let selection = tab.selection.clone().invert(width, height);
        self.commit_selection(selection, window, cx);
    }

    pub(super) fn clear_to_transparency(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_text_pixels(false, cx) {
            return;
        }
        let Some(tab) = self.active.map(|index| &mut self.tabs[index]) else {
            return;
        };
        if tab.selection == Selection::Empty {
            tab.notice = Some("Nothing is selected.".into());
            cx.notify();
            return;
        }
        let layer = tab.active_layer;
        if tab
            .document
            .content
            .layer(layer)
            .is_some_and(|layer| !layer.visible)
        {
            tab.notice = Some("The active layer is hidden. Show it before clearing.".into());
            cx.notify();
            return;
        }
        self.edit_content(
            move |content, _, selection| {
                content
                    .clear(layer, selection)
                    .map(Dirty::Region)
                    .unwrap_or(Dirty::None)
            },
            window,
            cx,
        );
    }

    pub(super) fn crop_to_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_text_pixels(true, cx) {
            return;
        }
        let Some(bounds) = self.selection_crop() else {
            return;
        };
        let crop = CropRect {
            x: bounds.x as u32,
            y: bounds.y as u32,
            width: bounds.width,
            height: bounds.height,
        };
        self.edit_content(
            move |content, _, selection| {
                if content.crop(crop) {
                    *selection = selection.clone().rebase(
                        i64::from(crop.x),
                        i64::from(crop.y),
                        crop.width,
                        crop.height,
                    );
                    Dirty::Canvas
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn bind_selection_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        if self.text_draft.is_some() {
            return root
                .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all_text(cx)));
        }
        let root = self.bind_selection::<SelectAll>(root, SelectionCommand::All, cx);
        let root = self.bind_selection::<SelectNone>(root, SelectionCommand::None, cx);
        let root = self.bind_selection::<InvertSelection>(root, SelectionCommand::Invert, cx);
        let root = self.bind_selection::<ClearToTransparency>(root, SelectionCommand::Clear, cx);
        self.bind_selection::<CropToSelection>(root, SelectionCommand::Crop, cx)
    }

    fn bind_selection<A: Action>(
        &self,
        root: Div,
        command: SelectionCommand,
        cx: &Context<Self>,
    ) -> Div {
        if !self.selection_command_enabled(command) {
            return root;
        }
        root.on_action(cx.listener(move |this, _: &A, window, cx| match command {
            SelectionCommand::All => this.select_all(window, cx),
            SelectionCommand::None => this.select_none(window, cx),
            SelectionCommand::Invert => this.invert_selection(window, cx),
            SelectionCommand::Clear => this.clear_to_transparency(window, cx),
            SelectionCommand::Crop => this.crop_to_selection(window, cx),
        }))
    }

    pub(super) fn selection_command_enabled(&self, command: SelectionCommand) -> bool {
        let Some(tab) = self.active_tab() else {
            return false;
        };
        if tab.busy() {
            return false;
        }
        match command {
            SelectionCommand::All | SelectionCommand::None | SelectionCommand::Invert => true,
            SelectionCommand::Clear => tab.selection != Selection::Empty,
            SelectionCommand::Crop => self.selection_crop().is_some(),
        }
    }

    fn selection_crop(&self) -> Option<PixelRect> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        tab.selection.crop_bounds(width, height)
    }

    fn commit_selection(
        &mut self,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_selection(move |_, _| selection, window, cx);
    }

    fn update_selection(
        &mut self,
        update: impl FnOnce(Selection, (u32, u32)) -> Selection + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_change(
            super::Change::Selection,
            move |content, _, current| {
                let selection = update(current.clone(), content.size());
                if *current == selection {
                    return Dirty::None;
                }
                *current = selection;
                Dirty::Unpainted
            },
            window,
            cx,
        );
    }

    pub(super) fn begin_free(
        &mut self,
        position: Point<Pixels>,
        modifiers: &gpui::Modifiers,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        let Some(geometry) = PreviewGeometry::for_tab(tab, self.viewport.get(), scale) else {
            return;
        };
        if self.tool != Tool::FreeSelect {
            return;
        }
        let point = geometry.fraction(position);
        if self.free_points.len() >= 3
            && near_polygon_start(geometry.project(self.free_points[0]), position)
        {
            self.close_free(window, cx);
            return;
        }
        if self.free_points.is_empty() {
            self.free_op = selection_op(modifiers);
        }
        self.free_drag_origin = Some(position);
        self.free_dragging = false;
        if self.free_points.last() != Some(&point) {
            self.free_points.push(point);
            cx.notify();
        }
    }

    pub(super) fn extend_free(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        if self.tool != Tool::FreeSelect || self.free_points.is_empty() {
            return;
        }
        let Some(origin) = self.free_drag_origin else {
            return;
        };
        let Some(geometry) = self
            .active_tab()
            .and_then(|tab| PreviewGeometry::for_tab(tab, self.viewport.get(), scale))
        else {
            return;
        };
        if !self.free_dragging {
            let delta = position - origin;
            if f32::from(delta.x).abs().max(f32::from(delta.y).abs()) < 3. {
                return;
            }
            self.free_dragging = true;
        }
        let point = geometry.fraction(position);
        if self.free_points.last() != Some(&point) {
            self.free_points.push(point);
            cx.notify();
        }
    }

    pub(super) fn finish_free(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tool != Tool::FreeSelect || self.free_points.is_empty() {
            return;
        }
        self.extend_free(position, window.scale_factor(), cx);
        self.free_drag_origin = None;
        self.free_dragging = false;
        cx.notify();
    }

    pub(super) fn close_free(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.free_points.len() < 3 || self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let points = std::mem::take(&mut self.free_points);
        self.free_drag_origin = None;
        self.free_dragging = false;
        let op = self.free_op;
        self.update_selection(
            move |current, (width, height)| {
                let points: Vec<_> = points
                    .into_iter()
                    .map(|point| {
                        (
                            f64::from(point.x) * f64::from(width),
                            f64::from(point.y) * f64::from(height),
                        )
                    })
                    .collect();
                let shape = Selection::polygon(&points, width, height);
                if shape == Selection::Empty {
                    current
                } else {
                    current.combine(shape, op, width, height)
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn cancel_free(&mut self, cx: &mut Context<Self>) {
        if self.free_points.is_empty() {
            return;
        }
        self.free_points.clear();
        self.free_drag_origin = None;
        self.free_dragging = false;
        cx.notify();
    }

    pub(super) fn wand_at(
        &mut self,
        position: Point<Pixels>,
        modifiers: &gpui::Modifiers,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((x, y)) = self.canvas_pixel(position, scale, false) else {
            return;
        };
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.busy() {
            return;
        }
        let layer_id = tab.active_layer;
        let Some(layer) = tab.document.content.layer(layer_id) else {
            return;
        };
        let local_x = i64::from(x) - i64::from(layer.offset.0);
        let local_y = i64::from(y) - i64::from(layer.offset.1);
        if local_x < 0
            || local_y < 0
            || local_x as u32 >= layer.pixels.width()
            || local_y as u32 >= layer.pixels.height()
        {
            self.notice("The active layer does not cover that pixel.", cx);
            return;
        }
        let pixels = layer.pixels.clone();
        let offset = layer.offset;
        let op = selection_op(modifiers);
        let tolerance = self.tolerance;
        self.update_selection(
            move |current, size| {
                let shape = Selection::flood(
                    &pixels,
                    local_x as u32,
                    local_y as u32,
                    tolerance,
                    offset,
                    size,
                );
                current.combine(shape, op, size.0, size.1)
            },
            window,
            cx,
        );
    }

    pub(super) fn reshape_selection(
        &mut self,
        edit: MaskEdit,
        radius: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.busy() {
            return;
        }
        if matches!(tab.selection, Selection::Unrestricted | Selection::Empty) {
            self.notice("Select part of the image first.", cx);
            return;
        }
        self.update_selection(
            move |current, (width, height)| match edit {
                MaskEdit::Feather => current.feather(radius, width, height),
                MaskEdit::Grow => current.grow(radius, width, height),
                MaskEdit::Shrink => current.shrink(radius, width, height),
            },
            window,
            cx,
        );
    }

    pub(super) fn set_oval_circle(
        &mut self,
        circle: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.oval_circle = circle;
        if circle {
            let state = self
                .active_tab()
                .filter(|tab| !tab.busy())
                .map(|tab| (tab.document.content.size(), tab.selection.clone()));
            if let Some(((width, height), selection)) = state {
                if let Some(circle) = circle_selection(&selection, width, height) {
                    if circle != selection {
                        self.commit_selection(circle, window, cx);
                    }
                } else if !matches!(selection, Selection::Unrestricted | Selection::Empty) {
                    self.select_none(window, cx);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn nudge_selection_radius(
        &mut self,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let max = self.selection_shape_rect().map_or_else(
            || {
                self.active_document().map_or(1, |document| {
                    let (width, height) = document.content.size();
                    width.max(height)
                })
            },
            |rect| rect.width.min(rect.height) / 2,
        );
        let max = max.min(i32::MAX as u32) as i32;
        self.selection_radius = (self.selection_radius as i32 + delta).clamp(0, max) as u32;
        let selection = self
            .selection_preview
            .clone()
            .or_else(|| self.active_tab().map(|tab| tab.selection.clone()));
        if let Some(rect) = selection.as_ref().and_then(rect_for_radius) {
            let Some(tab) = self.active_tab() else {
                return;
            };
            let (width, height) = tab.document.content.size();
            let shape = Selection::rounded_rect_rect(rect, self.selection_radius, width, height);
            self.commit_selection(shape, window, cx);
        }
        cx.notify();
    }

    pub(super) fn selection_shape_rect(&self) -> Option<PixelRect> {
        let selection = self
            .selection_preview
            .as_ref()
            .or_else(|| self.active_tab().map(|tab| &tab.selection))?;
        match selection {
            Selection::Rect(rect) => Some(*rect),
            Selection::Ellipse(ellipse) => Some(ellipse.rect()),
            Selection::RoundedRect(shape) => Some(shape.rect()),
            _ => None,
        }
    }
}

fn rect_for_radius(selection: &Selection) -> Option<PixelRect> {
    match selection {
        Selection::Rect(rect) => Some(*rect),
        Selection::RoundedRect(shape) => Some(shape.rect()),
        _ => None,
    }
}

fn circle_selection(selection: &Selection, width: u32, height: u32) -> Option<Selection> {
    let rect = match selection {
        Selection::Rect(rect) => *rect,
        Selection::Ellipse(ellipse) => ellipse.rect(),
        Selection::RoundedRect(shape) => shape.rect(),
        _ => return None,
    };
    Some(Selection::ellipse_rect(
        centered_square(rect),
        width,
        height,
    ))
}

fn centered_square(rect: PixelRect) -> PixelRect {
    let side = rect.width.min(rect.height);
    PixelRect::new(
        rect.x + i64::from((rect.width - side) / 2),
        rect.y + i64::from((rect.height - side) / 2),
        side,
        side,
    )
}

pub(super) fn selection_op(modifiers: &gpui::Modifiers) -> SelectionOp {
    match (modifiers.shift, modifiers.control) {
        (true, true) => SelectionOp::Intersect,
        (true, false) => SelectionOp::Add,
        (false, true) => SelectionOp::Subtract,
        (false, false) => SelectionOp::Replace,
    }
}

fn drag_selection(
    shape: SelectionShape,
    radius: u32,
    start: (u32, u32),
    end: (u32, u32),
    width: u32,
    height: u32,
) -> Selection {
    match shape {
        SelectionShape::Rectangle if radius == 0 => Selection::rectangle(start, end, width, height),
        SelectionShape::Rectangle => Selection::rounded_rect(start, end, radius, width, height),
        SelectionShape::Ellipse => Selection::ellipse(start, end, width, height),
        SelectionShape::Circle => Selection::circle(start, end, width, height),
    }
}

pub(super) fn near_polygon_start(origin: Point<Pixels>, pointer: Point<Pixels>) -> bool {
    let delta = pointer - origin;
    f32::from(delta.x).hypot(f32::from(delta.y)) <= POLYGON_CLOSE_SLOP
}

pub(super) fn adjustable_rect(selection: Selection) -> Option<PixelRect> {
    match selection {
        Selection::Rect(rect) | Selection::Inverse(rect) if rect.width > 0 && rect.height > 0 => {
            Some(rect)
        }
        Selection::Ellipse(ellipse) if ellipse.rect().width > 0 && ellipse.rect().height > 0 => {
            Some(ellipse.rect())
        }
        Selection::RoundedRect(shape) if shape.rect().width > 0 && shape.rect().height > 0 => {
            Some(shape.rect())
        }
        _ => None,
    }
}

fn placed_selection(previous: Selection, rect: PixelRect, width: u32, height: u32) -> Selection {
    let covers = width > 0
        && height > 0
        && rect.x <= 0
        && rect.y <= 0
        && rect.right() >= i64::from(width)
        && rect.bottom() >= i64::from(height);
    match previous {
        Selection::Inverse(_) => {
            if covers {
                Selection::Empty
            } else {
                Selection::Inverse(rect)
            }
        }
        Selection::Ellipse(_) => Selection::ellipse_rect(rect, width, height),
        Selection::RoundedRect(shape) => {
            Selection::rounded_rect_rect(rect, shape.radius(), width, height)
        }
        _ => {
            if covers {
                Selection::Full
            } else {
                Selection::Rect(rect)
            }
        }
    }
}

fn move_rect(
    rect: PixelRect,
    pointer: (i64, i64),
    grab: (i64, i64),
    width: u32,
    height: u32,
) -> PixelRect {
    let max_x = i64::from(width)
        .saturating_sub(i64::from(rect.width))
        .max(0);
    let max_y = i64::from(height)
        .saturating_sub(i64::from(rect.height))
        .max(0);
    PixelRect::new(
        (pointer.0 - grab.0).clamp(0, max_x),
        (pointer.1 - grab.1).clamp(0, max_y),
        rect.width,
        rect.height,
    )
}

fn resize_rect(
    rect: PixelRect,
    edges: SelectionEdges,
    delta: (i64, i64),
    width: u32,
    height: u32,
) -> PixelRect {
    let clamp_x = |value: i64| value.clamp(0, i64::from(width).saturating_sub(1).max(0));
    let clamp_y = |value: i64| value.clamp(0, i64::from(height).saturating_sub(1).max(0));
    let mut x0 = rect.x;
    let mut y0 = rect.y;
    let mut x1 = rect.right().saturating_sub(1);
    let mut y1 = rect.bottom().saturating_sub(1);
    if edges.left {
        x0 = clamp_x(x0.saturating_add(delta.0));
    }
    if edges.right {
        x1 = clamp_x(x1.saturating_add(delta.0));
    }
    if edges.top {
        y0 = clamp_y(y0.saturating_add(delta.1));
    }
    if edges.bottom {
        y1 = clamp_y(y1.saturating_add(delta.1));
    }
    let left = x0.min(x1);
    let right = x0.max(x1);
    let top = y0.min(y1);
    let bottom = y0.max(y1);
    PixelRect::new(
        left,
        top,
        (right - left + 1) as u32,
        (bottom - top + 1) as u32,
    )
}

fn fraction_pixel(fraction: f32, edge: u32) -> i64 {
    (fraction * edge as f32).floor() as i64
}

fn hit_selection(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    x: f32,
    y: f32,
    slop: f32,
) -> SelectionHit {
    if x < left - slop || x > right + slop || y < top - slop || y > bottom + slop {
        return SelectionHit::Outside;
    }
    let mut near_left = (x - left).abs() <= slop;
    let mut near_right = (x - right).abs() <= slop;
    let mut near_top = (y - top).abs() <= slop;
    let mut near_bottom = (y - bottom).abs() <= slop;
    if near_left && near_right {
        near_left = x - left <= right - x;
        near_right = !near_left;
    }
    if near_top && near_bottom {
        near_top = y - top <= bottom - y;
        near_bottom = !near_top;
    }
    if near_left || near_right || near_top || near_bottom {
        return SelectionHit::Edge(SelectionEdges {
            left: near_left,
            right: near_right,
            top: near_top,
            bottom: near_bottom,
        });
    }
    SelectionHit::Inside
}

impl SelectionEdges {
    fn cursor(self) -> CursorStyle {
        match (self.left || self.right, self.top || self.bottom) {
            (true, true) if (self.left && self.top) || (self.right && self.bottom) => {
                CursorStyle::ResizeUpLeftDownRight
            }
            (true, true) => CursorStyle::ResizeUpRightDownLeft,
            (true, false) => CursorStyle::ResizeLeftRight,
            (false, true) => CursorStyle::ResizeUpDown,
            (false, false) => CursorStyle::Crosshair,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum SelectionCommand {
    All,
    None,
    Invert,
    Clear,
    Crop,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_closes_near_its_start_in_screen_space() {
        let start = point(gpui::px(100.), gpui::px(80.));
        assert!(near_polygon_start(
            start,
            point(gpui::px(104.), gpui::px(84.))
        ));
        assert!(!near_polygon_start(
            start,
            point(gpui::px(107.), gpui::px(87.))
        ));
    }

    #[test]
    fn corner_cursors_follow_their_diagonals() {
        assert_eq!(
            SelectionEdges {
                left: true,
                right: false,
                top: true,
                bottom: false,
            }
            .cursor(),
            CursorStyle::ResizeUpLeftDownRight
        );
        assert_eq!(
            SelectionEdges {
                left: false,
                right: true,
                top: true,
                bottom: false,
            }
            .cursor(),
            CursorStyle::ResizeUpRightDownLeft
        );
    }

    #[test]
    fn corners_win_over_edges_and_a_click_inside_is_not_an_edge() {
        assert_eq!(
            hit_selection(0., 0., 100., 80., 1., 1., 6.),
            SelectionHit::Edge(SelectionEdges {
                left: true,
                right: false,
                top: true,
                bottom: false,
            })
        );
        assert_eq!(
            hit_selection(0., 0., 100., 80., 50., 2., 6.),
            SelectionHit::Edge(SelectionEdges {
                left: false,
                right: false,
                top: true,
                bottom: false,
            })
        );
        assert_eq!(
            hit_selection(0., 0., 100., 80., 50., 40., 6.),
            SelectionHit::Inside
        );
        assert_eq!(
            hit_selection(0., 0., 100., 80., 120., 40., 6.),
            SelectionHit::Outside
        );
    }

    #[test]
    fn moving_stays_on_the_canvas_and_resizing_can_cross_the_opposite_edge() {
        let rect = PixelRect::new(10, 20, 30, 40);
        assert_eq!(
            move_rect(rect, (22, 33), (2, 3), 100, 100),
            PixelRect::new(20, 30, 30, 40)
        );
        assert_eq!(
            move_rect(rect, (0, 0), (2, 3), 100, 100),
            PixelRect::new(0, 0, 30, 40)
        );
        assert_eq!(
            move_rect(rect, (200, 200), (0, 0), 100, 100),
            PixelRect::new(70, 60, 30, 40)
        );
        let right = SelectionEdges {
            left: false,
            right: true,
            top: false,
            bottom: false,
        };
        assert_eq!(
            resize_rect(rect, right, (11, 0), 100, 100),
            PixelRect::new(10, 20, 41, 40)
        );
        assert_eq!(resize_rect(rect, right, (0, 0), 100, 100), rect);
        let left = SelectionEdges {
            left: true,
            right: false,
            top: false,
            bottom: false,
        };
        assert_eq!(
            resize_rect(rect, left, (40, 0), 100, 100),
            PixelRect::new(39, 20, 12, 40)
        );
    }

    #[test]
    fn an_adjusted_inverse_stays_inverse_until_it_covers_the_canvas() {
        let hole = PixelRect::new(1, 1, 2, 2);
        assert_eq!(
            placed_selection(Selection::Inverse(hole), hole, 8, 8),
            Selection::Inverse(hole)
        );
        assert_eq!(
            placed_selection(Selection::Inverse(hole), PixelRect::new(0, 0, 8, 8), 8, 8),
            Selection::Empty
        );
        assert_eq!(
            placed_selection(Selection::Rect(hole), PixelRect::new(0, 0, 8, 8), 8, 8),
            Selection::Full
        );
    }

    #[test]
    fn circle_toggle_uses_the_inscribed_square() {
        assert_eq!(
            centered_square(PixelRect::new(0, 0, 10, 6)),
            PixelRect::new(2, 0, 6, 6)
        );
        assert_eq!(
            circle_selection(
                &Selection::ellipse_rect(PixelRect::new(0, 0, 10, 6), 20, 20),
                20,
                20
            ),
            Some(Selection::ellipse_rect(PixelRect::new(2, 0, 6, 6), 20, 20))
        );
        assert_eq!(circle_selection(&Selection::Unrestricted, 20, 20), None);
    }

    #[test]
    fn ellipse_keeps_its_shape_when_moved_and_resized() {
        let ellipse = Selection::ellipse_rect(PixelRect::new(1, 1, 4, 4), 10, 10);
        assert_eq!(
            adjustable_rect(ellipse.clone()),
            Some(PixelRect::new(1, 1, 4, 4))
        );
        assert_eq!(
            placed_selection(ellipse, PixelRect::new(2, 2, 4, 4), 10, 10),
            Selection::ellipse_rect(PixelRect::new(2, 2, 4, 4), 10, 10)
        );
    }

    #[test]
    fn selection_shapes_follow_the_chosen_mode() {
        let start = (1, 2);
        let end = (5, 3);
        assert_eq!(
            drag_selection(SelectionShape::Rectangle, 0, start, end, 10, 10),
            Selection::rectangle(start, end, 10, 10)
        );
        assert_eq!(
            drag_selection(SelectionShape::Rectangle, 2, start, end, 10, 10),
            Selection::rounded_rect(start, end, 2, 10, 10)
        );
        assert_eq!(
            drag_selection(SelectionShape::Ellipse, 0, start, end, 10, 10),
            Selection::ellipse(start, end, 10, 10)
        );
        assert_eq!(
            drag_selection(SelectionShape::Circle, 0, start, end, 10, 10),
            Selection::circle(start, end, 10, 10)
        );
    }

    #[test]
    fn pointer_coordinates_extend_past_the_canvas() {
        assert_eq!(fraction_pixel(-0.2, 10), -2);
        assert_eq!(fraction_pixel(0.15, 10), 1);
        assert_eq!(fraction_pixel(3., 10), 30);
    }
}
