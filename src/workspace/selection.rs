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
        anchor: (i64, i64),
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

/// Latest requested ratio while a selection-only commit is in flight.
/// Its context must survive that exact commit before another edit can run.
#[derive(Clone, Copy)]
pub(super) struct PendingAspectRatio {
    tab: usize,
    layer: crate::document::LayerId,
    tool: Tool,
    generation: u64,
    crop: bool,
    ratio: Option<crate::constraints::AspectRatio>,
}

impl PendingAspectRatio {
    fn new(
        tab: &super::Tab,
        tool: Tool,
        crop: bool,
        ratio: Option<crate::constraints::AspectRatio>,
    ) -> Self {
        Self {
            tab: tab.id,
            layer: tab.active_layer,
            tool,
            generation: tab.history.generation(),
            crop,
            ratio,
        }
    }

    fn accepts(self, tab: &super::Tab, tool: Tool, generation: u64, published: bool) -> bool {
        self.tab == tab.id
            && self.layer == tab.active_layer
            && self.tool == tool
            && self.generation == generation
            && !tab.busy()
            && generation.checked_add(u64::from(published)) == Some(tab.history.generation())
    }
}

impl Workspace {
    fn creation_feather_radius(&self) -> u32 {
        if self.preferences.selection_feather_enabled {
            self.preferences.selection_feather_radius
        } else {
            0
        }
    }

    pub(super) fn active_selection_radius(&self) -> u32 {
        match self
            .selection_preview
            .as_ref()
            .or_else(|| self.active_tab().map(|tab| &tab.selection))
        {
            Some(selection) if selection.feather_geometry().is_some() => {
                selection.feather_geometry().unwrap().radius
            }
            Some(Selection::RoundedRect(shape)) => shape.radius(),
            Some(Selection::Rect(_)) => 0,
            _ => self.preferences.selection_radius,
        }
    }

    pub(super) fn effective_selection_ratio(&self) -> Option<crate::constraints::AspectRatio> {
        if self.preferences.tool == Tool::OvalSelect && self.preferences.oval_circle {
            Some(crate::constraints::AspectRatio::SQUARE)
        } else {
            self.preferences.selection_ratio
        }
    }

    pub(super) fn active_selection_antialias(&self) -> bool {
        self.active_tab()
            .filter(|tab| {
                tab.selection.feather_geometry().is_some()
                    || matches!(
                        tab.selection,
                        Selection::Ellipse(_) | Selection::RoundedRect(_)
                    )
            })
            .map_or(self.preferences.selection_antialias, |tab| {
                tab.selection.is_antialiased()
            })
    }

    pub(super) fn set_aspect_ratio(
        &mut self,
        crop: bool,
        ratio: Option<crate::constraints::AspectRatio>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A typed selection edit may itself start the commit we must wait for.
        self.commit_numeric_edit(window, cx);
        if let Some(tab) = self.active_tab().filter(|tab| tab.editing) {
            self.pending_aspect_ratio = Some(PendingAspectRatio::new(
                tab,
                self.preferences.tool,
                crop,
                ratio,
            ));
            return;
        }
        if self.active_tab().is_some_and(|tab| tab.busy()) {
            return;
        }
        self.pending_aspect_ratio = None;
        if crop {
            self.preferences.crop_ratio = ratio;
            if let Some(ratio) = ratio {
                self.preferences.crop_ratio_value = ratio;
                self.crop = self.crop.map(|crop| ratio_crop(crop, ratio));
            }
        } else {
            self.preferences.oval_circle = false;
            self.preferences.selection_ratio = ratio;
            if let Some(ratio) = ratio {
                self.preferences.selection_ratio_value = ratio;
                if self.selection_shape_rect().is_some() {
                    self.update_selection(
                        move |current, size| refit_selection(current, ratio, size),
                        window,
                        cx,
                    );
                }
            }
        }
        cx.notify();
    }

    pub(super) fn finish_pending_aspect_ratio(
        &mut self,
        id: usize,
        generation: u64,
        change: super::Change,
        published: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .pending_aspect_ratio
            .as_ref()
            .is_none_or(|pending| pending.tab != id)
        {
            return;
        }
        let pending = self.pending_aspect_ratio.take().unwrap();
        if !matches!(change, super::Change::Selection)
            || self.form.is_some()
            || self.dialog_open
            || self.numeric_edit.is_some()
            || self.active_tab().is_none_or(|tab| {
                !pending.accepts(tab, self.preferences.tool, generation, published)
            })
        {
            return;
        }
        self.set_aspect_ratio(pending.crop, pending.ratio, window, cx);
    }

    pub(super) fn set_selection_antialias(
        &mut self,
        antialias: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preferences.selection_antialias = antialias;
        let Some(tab) = self.active_tab().filter(|tab| !tab.busy()) else {
            cx.notify();
            return;
        };
        let _ = tab;
        self.update_selection(
            move |selection, size| selection.with_antialias(antialias, size.0, size.1),
            window,
            cx,
        );
        cx.notify();
    }

    pub(super) fn clear_selection_gesture(&mut self) {
        self.pending_aspect_ratio = None;
        if self
            .numeric_edit
            .as_ref()
            .is_some_and(|edit| edit.field.is_selection())
        {
            self.numeric_edit = None;
        }
        self.selection_drag = None;
        self.selection_preview = None;
        self.clear_free_draft();
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
        let Some(anchor) = self.unclamped_pointer_pixel(position, scale) else {
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
                        anchor,
                        op,
                    },
                }
            } else {
                SelectionGesture::Create {
                    origin: position,
                    anchor,
                    op,
                }
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
                anchor: start,
                ..
            } => {
                let delta = position - origin;
                if f32::from(delta.x).abs().max(f32::from(delta.y).abs()) < 4. {
                    None
                } else {
                    let shape = if self.preferences.tool == Tool::Select {
                        SelectionShape::Rectangle
                    } else if self.preferences.oval_circle {
                        SelectionShape::Circle
                    } else {
                        SelectionShape::Ellipse
                    };
                    let radius = self.preferences.selection_radius;
                    self.unclamped_pointer_pixel(position, scale).map(|end| {
                        Selection::shape_drag(
                            shape,
                            start,
                            end,
                            (width, height),
                            radius,
                            self.preferences.selection_antialias,
                            self.effective_selection_ratio(),
                        )
                    })
                }
            }
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
                    self.effective_selection_ratio().map_or_else(
                        || resize_rect(rect, edges, delta, width, height),
                        |ratio| {
                            ratio.resize(
                                rect,
                                (edges.left, edges.right, edges.top, edges.bottom),
                                delta,
                                (width, height),
                            )
                        },
                    ),
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
                let feather = self.creation_feather_radius();
                self.update_selection(
                    move |current, size| {
                        combine_created_selection(current, shape, op, size, feather)
                    },
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
            (Some(_), Some(selection)) => self.commit_placed_selection(selection, window, cx),
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
            SelectionCommand::Clear => {
                tab.selection != Selection::Empty
                    && tab
                        .document
                        .content
                        .layer(tab.active_layer)
                        .is_some_and(|layer| !layer.locks.alpha)
            }
            SelectionCommand::Crop => self.selection_crop().is_some(),
        }
    }

    fn selection_crop(&self) -> Option<PixelRect> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        tab.selection.crop_bounds(width, height)
    }

    pub(super) fn commit_placed_selection(
        &mut self,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_selection(
            move |current, size| {
                if let Some(geometry) = current.feather_geometry() {
                    selection.feather(geometry.feather, size.0, size.1)
                } else {
                    selection
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn commit_selection(
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
        _window: &mut Window,
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
        if self.preferences.tool != Tool::FreeSelect {
            return;
        }
        let point = geometry.fraction(position);
        self.free_drag_origin = Some(position);
        self.free_dragging = false;
        self.free_node_drag =
            free_node_at(&self.free_points, &self.free_nodes, &geometry, position)
                .map(|index| (index, self.free_points[index]));
        if self.free_node_drag.is_some() {
            cx.notify();
            return;
        }
        if self.free_points.is_empty() {
            self.free_op = selection_op(modifiers);
        }
        if self.free_points.last() != Some(&point) {
            self.free_nodes.push(self.free_points.len());
            self.free_points.push(point);
        }
        cx.notify();
    }

    pub(super) fn extend_free(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        if self.preferences.tool != Tool::FreeSelect || self.free_points.is_empty() {
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
        if let Some((index, original)) = self.free_node_drag {
            // Preserve the grab offset and derive every frame from the gesture baseline.
            let delta = geometry.fraction(position) - geometry.fraction(origin);
            self.free_points[index] = original + delta;
        } else {
            let point = geometry.fraction(position);
            if self.free_points.last() != Some(&point) {
                self.free_points.push(point);
            }
        }
        cx.notify();
    }

    pub(super) fn finish_free(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preferences.tool != Tool::FreeSelect || self.free_points.is_empty() {
            return;
        }
        if self.free_drag_origin.is_none() {
            return;
        }
        self.extend_free(position, window.scale_factor(), cx);
        let close = self.free_node_drag.is_some_and(|(index, _)| index == 0)
            && !self.free_dragging
            && self.free_points.len() >= 3;
        if self.free_node_drag.is_none() {
            let last = self.free_points.len() - 1;
            if self.free_nodes.last() != Some(&last) {
                self.free_nodes.push(last);
            }
        }
        self.free_node_drag = None;
        self.free_drag_origin = None;
        self.free_dragging = false;
        if close {
            self.close_free(window, cx);
        }
        cx.notify();
    }

    pub(super) fn close_free(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.free_points.len() < 3 || self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let points = std::mem::take(&mut self.free_points);
        self.clear_free_draft();
        let op = self.free_op;
        let antialias = self.preferences.free_antialias;
        let feather = self.creation_feather_radius();
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
                let shape = Selection::polygon_quality(&points, width, height, antialias);
                if shape == Selection::Empty {
                    current
                } else {
                    combine_created_selection(current, shape, op, (width, height), feather)
                }
            },
            window,
            cx,
        );
    }

    pub(super) fn clear_free_draft(&mut self) {
        self.free_points.clear();
        self.free_nodes.clear();
        self.free_node_drag = None;
        self.free_drag_origin = None;
        self.free_dragging = false;
    }

    pub(super) fn cancel_free(&mut self, cx: &mut Context<Self>) {
        if self.free_points.is_empty() {
            return;
        }
        self.clear_free_draft();
        cx.notify();
    }

    pub(super) fn nudge_region_match(
        &mut self,
        wand: bool,
        gap: bool,
        delta: i16,
        cx: &mut Context<Self>,
    ) {
        let matching = if wand {
            &mut self.preferences.wand_match
        } else {
            &mut self.preferences.fill_match
        };
        let (value, max) = if gap {
            (&mut matching.gap_radius, crate::selection::MAX_GAP_RADIUS)
        } else {
            (&mut matching.tolerance, u8::MAX)
        };
        *value = (i16::from(*value) + delta).clamp(0, i16::from(max)) as u8;
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
        let sample_all = self.preferences.wand_sample_all;
        if !sample_all
            && wand_source(&tab.document.content, tab.active_layer, false, (x, y)).is_none()
        {
            self.notice("The active layer does not cover that pixel.", cx);
            return;
        }
        let content = tab.document.content.clone();
        let layer = tab.active_layer;
        let op = selection_op(modifiers);
        let matching = self.preferences.wand_match;
        let antialias = self.preferences.wand_antialias;
        let feather = self.creation_feather_radius();
        self.update_selection(
            move |current, size| {
                let Some(source) = wand_source(&content, layer, sample_all, (x, y)) else {
                    return current;
                };
                let shape = Selection::flood_region_quality(
                    &source.pixels,
                    source.point,
                    matching,
                    source.offset,
                    size,
                    None,
                    antialias,
                );
                combine_created_selection(current, shape, op, size, feather)
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

    pub(super) fn nudge_selection_radius(
        &mut self,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.commit_numeric_with_followup(
            super::numeric::NumericFollowup::SelectionRadius(delta),
            window,
            cx,
        ) || self.active_tab().is_some_and(|tab| tab.busy())
        {
            return;
        }
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
        self.preferences.selection_radius =
            (self.active_selection_radius() as i32 + delta).clamp(0, max) as u32;
        let selection = self
            .selection_preview
            .clone()
            .or_else(|| self.active_tab().map(|tab| tab.selection.clone()));
        if let Some(rect) = selection.as_ref().and_then(rect_for_radius) {
            let Some(tab) = self.active_tab() else {
                return;
            };
            let shape = editable_selection_for_rect(
                selection.as_ref().unwrap(),
                rect,
                Some(self.preferences.selection_radius),
                self.preferences.selection_antialias,
                tab.document.content.size(),
            );
            if let Some(shape) = shape {
                self.commit_placed_selection(shape, window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn selection_shape_rect(&self) -> Option<PixelRect> {
        let selection = self
            .selection_preview
            .as_ref()
            .or_else(|| self.active_tab().map(|tab| &tab.selection))?;
        if let Some(geometry) = selection.feather_geometry() {
            return Some(geometry.rect);
        }
        match selection {
            Selection::Rect(rect) => Some(*rect),
            Selection::Ellipse(ellipse) => Some(ellipse.rect()),
            Selection::RoundedRect(shape) => Some(shape.rect()),
            _ => None,
        }
    }
}

struct WandSource {
    pixels: std::sync::Arc<image::RgbaImage>,
    offset: (i32, i32),
    point: (u32, u32),
}

fn combine_created_selection(
    current: Selection,
    incoming: Selection,
    op: SelectionOp,
    size: (u32, u32),
    feather: u32,
) -> Selection {
    // Only soften the new region. Repeated additions must not blur old coverage.
    current.combine(
        incoming.feather(feather, size.0, size.1),
        op,
        size.0,
        size.1,
    )
}

fn wand_source(
    content: &crate::document::DocumentContent,
    layer: crate::document::LayerId,
    sample_all: bool,
    point: (u32, u32),
) -> Option<WandSource> {
    if sample_all {
        return Some(WandSource {
            pixels: content.composite(),
            offset: (0, 0),
            point,
        });
    }
    let layer = content.layer(layer)?;
    let x = i64::from(point.0) - i64::from(layer.offset.0);
    let y = i64::from(point.1) - i64::from(layer.offset.1);
    if x < 0
        || y < 0
        || x >= i64::from(layer.pixels.width())
        || y >= i64::from(layer.pixels.height())
    {
        return None;
    }
    Some(WandSource {
        pixels: layer.pixels.clone(),
        offset: layer.offset,
        point: (x as u32, y as u32),
    })
}

pub(super) fn editable_selection_for_rect(
    current: &Selection,
    rect: PixelRect,
    radius: Option<u32>,
    default_antialias: bool,
    size: (u32, u32),
) -> Option<Selection> {
    if let Some(mut geometry) = current.feather_geometry() {
        if !geometry.ellipse
            && let Some(radius) = radius
        {
            geometry.radius = radius.min(rect.width.min(rect.height) / 2);
        }
        return Some(geometry.shape(rect, size.0, size.1));
    }
    match current {
        Selection::Rect(_) | Selection::RoundedRect(_) if radius.is_some() => {
            Some(Selection::rounded_rect_quality(
                rect,
                radius?,
                size.0,
                size.1,
                if matches!(current, Selection::RoundedRect(_)) {
                    current.is_antialiased()
                } else {
                    default_antialias
                },
            ))
        }
        Selection::Rect(_) | Selection::RoundedRect(_) | Selection::Ellipse(_) => {
            Some(placed_selection(current.clone(), rect, size.0, size.1))
        }
        _ => None,
    }
}

pub(super) fn ratio_crop(crop: CropRect, ratio: crate::constraints::AspectRatio) -> CropRect {
    let rect = ratio.inscribe(PixelRect::new(
        i64::from(crop.x),
        i64::from(crop.y),
        crop.width,
        crop.height,
    ));
    CropRect {
        x: rect.x as u32,
        y: rect.y as u32,
        width: rect.width,
        height: rect.height,
    }
}

fn refit_selection(
    current: Selection,
    ratio: crate::constraints::AspectRatio,
    size: (u32, u32),
) -> Selection {
    let rect = current
        .feather_geometry()
        .map(|geometry| geometry.rect)
        .or_else(|| match &current {
            Selection::Rect(rect) => Some(*rect),
            Selection::Ellipse(shape) => Some(shape.rect()),
            Selection::RoundedRect(shape) => Some(shape.rect()),
            _ => None,
        });
    let Some(rect) = rect else {
        return current;
    };
    let next = editable_selection_for_rect(
        &current,
        ratio.inscribe(rect),
        None,
        current.is_antialiased(),
        size,
    );
    match next {
        Some(next) => {
            if let Some(geometry) = current.feather_geometry() {
                next.feather(geometry.feather, size.0, size.1)
            } else {
                next
            }
        }
        None => current,
    }
}

fn rect_for_radius(selection: &Selection) -> Option<PixelRect> {
    if let Some(geometry) = selection
        .feather_geometry()
        .filter(|geometry| !geometry.ellipse)
    {
        return Some(geometry.rect);
    }
    match selection {
        Selection::Rect(rect) => Some(*rect),
        Selection::RoundedRect(shape) => Some(shape.rect()),
        _ => None,
    }
}

pub(super) fn selection_op(modifiers: &gpui::Modifiers) -> SelectionOp {
    match (modifiers.shift, modifiers.control) {
        (true, true) => SelectionOp::Intersect,
        (true, false) => SelectionOp::Add,
        (false, true) => SelectionOp::Subtract,
        (false, false) => SelectionOp::Replace,
    }
}

#[cfg(test)]
fn drag_selection(
    shape: SelectionShape,
    radius: u32,
    start: (i64, i64),
    end: (i64, i64),
    width: u32,
    height: u32,
) -> Selection {
    Selection::shape_drag(shape, start, end, (width, height), radius, false, None)
}

pub(super) fn free_node_at(
    points: &[Point<f32>],
    nodes: &[usize],
    geometry: &PreviewGeometry,
    pointer: Point<Pixels>,
) -> Option<usize> {
    nodes
        .iter()
        .copied()
        .filter_map(|index| {
            let delta = geometry.project(*points.get(index)?) - pointer;
            let distance = f32::from(delta.x).hypot(f32::from(delta.y));
            (distance <= POLYGON_CLOSE_SLOP).then_some((index, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

pub(super) fn near_polygon_start(origin: Point<Pixels>, pointer: Point<Pixels>) -> bool {
    let delta = pointer - origin;
    f32::from(delta.x).hypot(f32::from(delta.y)) <= POLYGON_CLOSE_SLOP
}

pub(super) fn adjustable_rect(selection: Selection) -> Option<PixelRect> {
    if let Some(geometry) = selection.feather_geometry() {
        return Some(geometry.rect);
    }
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

pub(super) fn placed_selection(
    previous: Selection,
    rect: PixelRect,
    width: u32,
    height: u32,
) -> Selection {
    if let Some(geometry) = previous.feather_geometry() {
        return geometry.shape(rect, width, height);
    }
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
        Selection::Ellipse(shape) => Selection::ellipse_rect_quality(
            rect,
            width,
            height,
            Selection::Ellipse(shape).is_antialiased(),
        ),
        Selection::RoundedRect(shape) => Selection::rounded_rect_quality(
            rect,
            shape.radius(),
            width,
            height,
            Selection::RoundedRect(shape).is_antialiased(),
        ),
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
        (pointer.0 - grab.0).clamp(rect.x.min(0), max_x.max(rect.x)),
        (pointer.1 - grab.1).clamp(rect.y.min(0), max_y.max(rect.y)),
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
    fn radius_zero_roundtrip_keeps_curve_quality_and_feather() {
        let rect = PixelRect::new(5, 5, 24, 18);
        for feather in [0, 3] {
            let original =
                Selection::rounded_rect_quality(rect, 4, 64, 64, true).feather(feather, 64, 64);
            let square = editable_selection_for_rect(&original, rect, Some(0), false, (64, 64))
                .unwrap()
                .feather(feather, 64, 64);
            assert!(square.is_antialiased());
            // Retained AA is metadata: a zero-radius rectangle still has hard
            // geometric edges, before the unchanged feather is applied.
            let expected_square = Selection::Rect(rect).feather(feather, 64, 64);
            for y in 0..64 {
                for x in 0..64 {
                    assert_eq!(
                        square.coverage(x, y, 64, 64),
                        expected_square.coverage(x, y, 64, 64)
                    );
                }
            }
            let rounded = editable_selection_for_rect(&square, rect, Some(4), false, (64, 64))
                .unwrap()
                .feather(feather, 64, 64);
            assert_eq!(rounded, original);
        }
    }

    #[test]
    fn latest_ratio_waits_for_the_selection_commit_and_validates_its_context() {
        use crate::constraints::AspectRatio;
        let mut tab = super::super::tests::opacity_tab();
        tab.document =
            crate::document::Document::blank("Ratio".into(), 64, 64, image::Rgba([255; 4]));
        tab.active_layer = tab.document.content.preferred_active_layer();
        tab.selection = Selection::Rect(PixelRect::new(10, 10, 24, 18));
        let generation = tab.history.generation();
        tab.editing = true;
        let mut pending = Some(PendingAspectRatio::new(
            &tab,
            Tool::Select,
            false,
            Some(AspectRatio::SQUARE),
        ));
        let latest = AspectRatio {
            width: 2,
            height: 1,
        };
        pending.replace(PendingAspectRatio::new(
            &tab,
            Tool::Select,
            false,
            Some(latest),
        ));
        let pending = pending.unwrap();
        assert_eq!(pending.ratio, Some(latest));
        assert!(!pending.accepts(&tab, Tool::Select, generation, false));
        let state = tab.state();
        let first = refit_selection(state.selection, AspectRatio::SQUARE, state.content.size());
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            Dirty::Unpainted,
        )
        .unwrap();
        assert!(
            tab.publish_change(
                generation,
                super::super::Change::Selection,
                Some((state.content, state.active_layer, first, patch)),
                1
            )
            .0
        );
        assert!(pending.accepts(&tab, Tool::Select, generation, true));
        assert_eq!(
            pending
                .ratio
                .map(|ratio| refit_selection(tab.selection.clone(), ratio, (64, 64))),
            Some(Selection::Rect(PixelRect::new(13, 14, 18, 9)))
        );
        assert!(!pending.accepts(&tab, Tool::OvalSelect, generation, true));
        assert!(!pending.accepts(&tab, Tool::Select, generation + 1, true));
        tab.id += 1;
        assert!(!pending.accepts(&tab, Tool::Select, generation, true));
        tab.id -= 1;
        let layer = tab.active_layer;
        tab.active_layer = crate::document::LayerId(layer.0 + 1);
        assert!(!pending.accepts(&tab, Tool::Select, generation, true));
        tab.active_layer = layer;
        tab.history.commit(tab.state(), 2, false);
        assert!(!pending.accepts(&tab, Tool::Select, generation, true));
    }

    #[test]
    fn ratio_changes_preserve_curve_quality_and_feather_without_compounding_it() {
        let rect = PixelRect::new(-4, 6, 24, 12);
        let target = PixelRect::new(2, 6, 12, 12);
        for (source, expected) in [
            (Selection::Rect(rect), Selection::Rect(target)),
            (
                Selection::ellipse_rect_quality(rect, 64, 64, true),
                Selection::ellipse_rect_quality(target, 64, 64, true),
            ),
            (
                Selection::rounded_rect_quality(rect, 4, 64, 64, true),
                Selection::rounded_rect_quality(target, 4, 64, 64, true),
            ),
        ] {
            for feather in [0, 3] {
                let source = source.clone().feather(feather, 64, 64);
                let expected = expected.clone().feather(feather, 64, 64);
                let fitted =
                    refit_selection(source, crate::constraints::AspectRatio::SQUARE, (64, 64));
                assert_eq!(fitted, expected);
                assert_eq!(
                    refit_selection(
                        fitted.clone(),
                        crate::constraints::AspectRatio::SQUARE,
                        (64, 64)
                    ),
                    fitted
                );
            }
        }
        let arbitrary = Selection::from_coverage(8, 8, |x, y| if x == y { 128 } else { 0 });
        assert_eq!(
            refit_selection(
                arbitrary.clone(),
                crate::constraints::AspectRatio::SQUARE,
                (8, 8)
            ),
            arbitrary
        );
    }

    #[test]
    fn typed_geometry_and_stepped_radius_keep_feather_and_antialias_quality() {
        let size = (64, 64);
        let source = Selection::rounded_rect_quality(PixelRect::new(3, 4, 24, 18), 4, 64, 64, true)
            .feather(3, 64, 64);
        let target = PixelRect::new(10, 12, 30, 20);
        for radius in [None, Some(0), Some(6)] {
            let preview =
                editable_selection_for_rect(&source, target, radius, false, size).unwrap();
            let committed = preview.feather(source.feather_geometry().unwrap().feather, 64, 64);
            let expected =
                Selection::rounded_rect_quality(target, radius.unwrap_or(4), 64, 64, true)
                    .feather(3, 64, 64);
            assert_eq!(committed, expected);
        }
        let ellipse = Selection::ellipse_rect_quality(PixelRect::new(2, 4, 20, 10), 64, 64, true)
            .feather(2, 64, 64);
        let edited = editable_selection_for_rect(&ellipse, target, None, false, size)
            .unwrap()
            .feather(2, 64, 64);
        assert_eq!(
            edited,
            Selection::ellipse_rect_quality(target, 64, 64, true).feather(2, 64, 64)
        );
    }

    #[test]
    fn custom_crop_ratios_preview_from_the_original_box_and_stay_inside_it() {
        let baseline = CropRect {
            x: 8,
            y: 10,
            width: 80,
            height: 60,
        };
        let narrow = ratio_crop(
            baseline,
            crate::constraints::AspectRatio {
                width: 1,
                height: 3,
            },
        );
        assert_eq!(
            (narrow.x, narrow.y, narrow.width, narrow.height),
            (38, 10, 20, 60)
        );
        let final_ratio = crate::constraints::AspectRatio {
            width: 16,
            height: 9,
        };
        let final_box = ratio_crop(baseline, final_ratio);
        assert_eq!(
            (final_box.x, final_box.y, final_box.width, final_box.height),
            (8, 17, 80, 45)
        );
        assert_ne!(final_box, ratio_crop(narrow, final_ratio));
    }

    #[test]
    fn reset_defaults_make_new_regions_hard_without_flattening_existing_curves() {
        let mut preferences = super::super::settings_ui::Preferences {
            selection_antialias: true,
            selection_radius: 5,
            ..Default::default()
        };
        let existing =
            Selection::rounded_rect_quality(PixelRect::new(10, 10, 24, 18), 5, 64, 64, true);
        preferences.reset_tool(Tool::Select);
        let resized = placed_selection(existing, PixelRect::new(20, 20, 30, 24), 64, 64);
        assert!(resized.is_antialiased());
        assert!(matches!(resized, Selection::RoundedRect(shape) if shape.radius() == 5));
        let created = Selection::shape_drag(
            SelectionShape::Rectangle,
            (10, 10),
            (33, 27),
            (64, 64),
            preferences.selection_radius,
            preferences.selection_antialias,
            preferences.selection_ratio,
        );
        assert!(matches!(created, Selection::Rect(_)));
        assert!(!created.is_antialiased());
    }

    #[test]
    fn wand_soft_edges_feather_before_combination_and_undo_without_dirtying_pixels() {
        let image = image::RgbaImage::from_fn(2, 2, |x, y| {
            image::Rgba(if (x, y) == (1, 1) {
                [0, 0, 0, 255]
            } else {
                [255; 4]
            })
        });
        let incoming = Selection::flood_region_quality(
            &image,
            (0, 0),
            crate::selection::RegionMatch::default(),
            (0, 0),
            (2, 2),
            None,
            true,
        );
        assert!((1..255).contains(&incoming.coverage(1, 1, 2, 2)));
        let feathered = incoming.clone().feather(1, 2, 2);
        for op in [
            SelectionOp::Replace,
            SelectionOp::Add,
            SelectionOp::Subtract,
            SelectionOp::Intersect,
        ] {
            let current = Selection::Rect(PixelRect::new(1, 0, 1, 2));
            assert_eq!(
                combine_created_selection(current.clone(), incoming.clone(), op, (2, 2), 1),
                current.combine(feathered.clone(), op, 2, 2)
            );
        }
        let mut tab = super::super::tests::opacity_tab();
        let source = tab
            .document
            .content
            .layer(tab.active_layer)
            .unwrap()
            .pixels
            .clone();
        let state = tab.state();
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            Dirty::Unpainted,
        )
        .unwrap();
        assert!(
            tab.publish_change(
                tab.history.generation(),
                super::super::Change::Selection,
                Some((state.content, state.active_layer, incoming.clone(), patch)),
                1
            )
            .0
        );
        assert!(!tab.history.is_dirty());
        assert_eq!(tab.selection, incoming);
        assert!(std::sync::Arc::ptr_eq(
            &source,
            &tab.document.content.layer(tab.active_layer).unwrap().pixels
        ));
        let target = tab.history.undo_target().unwrap().0.clone();
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &target.content,
            Dirty::Unpainted,
        )
        .unwrap();
        assert!(
            tab.publish_change(
                tab.history.generation(),
                super::super::Change::Undo,
                Some((target.content, target.active_layer, target.selection, patch)),
                2
            )
            .0
        );
        assert_eq!(tab.selection, Selection::Unrestricted);
        assert!(!tab.history.is_dirty());
        assert_eq!(tab.history.redo_target().unwrap().0.selection, incoming);
    }

    #[test]
    fn creation_feather_softens_only_the_incoming_region_before_combining() {
        let size = (20, 12);
        let current = Selection::Rect(PixelRect::new(1, 1, 4, 4));
        let incoming = Selection::Rect(PixelRect::new(10, 4, 4, 4));
        let soft = incoming.clone().feather(1, size.0, size.1);
        let added =
            combine_created_selection(current.clone(), incoming.clone(), SelectionOp::Add, size, 1);
        assert_eq!(added.coverage(1, 1, size.0, size.1), 255);
        assert_eq!(added.coverage(0, 0, size.0, size.1), 0);
        assert_eq!(
            added.coverage(10, 4, size.0, size.1),
            soft.coverage(10, 4, size.0, size.1)
        );
        assert!(added.coverage(9, 3, size.0, size.1) > 0);
        let replaced = combine_created_selection(
            current.clone(),
            incoming.clone(),
            SelectionOp::Replace,
            size,
            1,
        );
        assert_eq!(replaced, soft);
        let subtracted = combine_created_selection(
            Selection::Full,
            incoming.clone(),
            SelectionOp::Subtract,
            size,
            1,
        );
        let intersected = combine_created_selection(
            Selection::Full,
            incoming.clone(),
            SelectionOp::Intersect,
            size,
            1,
        );
        for y in 0..size.1 {
            for x in 0..size.0 {
                let coverage = soft.coverage(i64::from(x), i64::from(y), size.0, size.1);
                assert_eq!(
                    subtracted.coverage(i64::from(x), i64::from(y), size.0, size.1),
                    255 - coverage
                );
                assert_eq!(
                    intersected.coverage(i64::from(x), i64::from(y), size.0, size.1),
                    coverage
                );
            }
        }
        // Disabling feather is exactly the previous hard-selection combination.
        assert_eq!(
            combine_created_selection(current.clone(), incoming.clone(), SelectionOp::Add, size, 0),
            current.combine(incoming, SelectionOp::Add, size.0, size.1)
        );
    }

    #[test]
    fn wand_creation_feather_preserves_coverage_at_layer_offsets_and_canvas_edges() {
        use image::{Rgba, RgbaImage};
        let pixels = RgbaImage::from_pixel(3, 3, Rgba([10, 20, 30, 255]));
        let incoming = Selection::flood(&pixels, 0, 0, 0, (-1, 1), (6, 5));
        let selected = combine_created_selection(
            Selection::Unrestricted,
            incoming,
            SelectionOp::Replace,
            (6, 5),
            1,
        );
        assert_eq!(selected.coverage(0, 1, 6, 5), 113);
        assert_eq!(selected.coverage(2, 2, 6, 5), 85);
        assert_eq!(selected.coverage(-1, 2, 6, 5), 0);
        assert_eq!(selected.coverage(4, 2, 6, 5), 0);
    }

    #[test]
    fn soft_geometry_changes_and_undo_preserve_the_saved_revision() {
        let rounded = placed_selection(
            Selection::rounded_rect_quality(PixelRect::new(0, 0, 8, 8), 2, 20, 20, true),
            PixelRect::new(4, 3, 10, 8),
            20,
            20,
        );
        assert_eq!(
            rounded,
            Selection::rounded_rect_quality(PixelRect::new(4, 3, 10, 8), 2, 20, 20, true)
        );
        let mut tab = super::super::tests::opacity_tab();
        let generation = tab.history.generation();
        let mut state = tab.state();
        state.selection = Selection::ellipse_rect_quality(PixelRect::new(0, 0, 1, 2), 2, 2, true);
        let selection = placed_selection(state.selection, PixelRect::new(1, 0, 1, 2), 2, 2);
        assert!(selection.is_antialiased());
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            Dirty::Unpainted,
        )
        .unwrap();
        assert!(
            tab.publish_change(
                generation,
                super::super::Change::Selection,
                Some((state.content, state.active_layer, selection.clone(), patch)),
                1
            )
            .0
        );
        assert_eq!(tab.selection, selection);
        assert!(!tab.history.is_dirty());
        assert_eq!(
            adjustable_rect(tab.selection.clone()),
            Some(PixelRect::new(1, 0, 1, 2))
        );
        let target = tab.history.undo_target().unwrap().0.clone();
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &target.content,
            Dirty::Unpainted,
        )
        .unwrap();
        assert!(
            tab.publish_change(
                tab.history.generation(),
                super::super::Change::Undo,
                Some((target.content, target.active_layer, target.selection, patch)),
                2
            )
            .0
        );
        assert_eq!(tab.selection, Selection::Unrestricted);
        assert!(!tab.history.is_dirty());
    }

    #[test]
    fn wand_composite_sampling_uses_canvas_coordinates_and_visible_layers() {
        use crate::document::{DocumentContent, LayerId};
        use image::{Rgba, RgbaImage};
        let red = Rgba([255, 0, 0, 255]);
        let blue = Rgba([0, 0, 255, 255]);
        let mut base = RgbaImage::from_pixel(4, 2, red);
        for y in 0..2 {
            base.put_pixel(3, y, blue);
        }
        let mut content = DocumentContent::single_layer("Base".into(), base);
        let active = content
            .add_layer(LayerId(0), "Small offset".into(), Rgba([0; 4]))
            .unwrap();
        content.place_layer(active, RgbaImage::new(1, 1), (1, 0));
        assert!(wand_source(&content, active, false, (0, 0)).is_none());
        let local = wand_source(&content, active, false, (1, 0)).unwrap();
        assert_eq!(local.point, (0, 0));
        assert_eq!(local.offset, (1, 0));
        let source = wand_source(&content, active, true, (0, 0)).unwrap();
        assert_eq!(source.point, (0, 0));
        assert_eq!(source.offset, (0, 0));
        let selected = Selection::flood(&source.pixels, 0, 0, 0, source.offset, content.size());
        assert_eq!(selected.coverage(2, 1, 4, 2), 255);
        assert_eq!(selected.coverage(3, 1, 4, 2), 0);
        let hidden = content
            .add_layer(active, "Hidden blue".into(), blue)
            .unwrap();
        content.set_visible(hidden, false);
        assert_eq!(
            wand_source(&content, active, true, (0, 0)).unwrap().pixels,
            source.pixels
        );
        content.set_visible(hidden, true);
        let covered = wand_source(&content, active, true, (0, 0)).unwrap();
        assert_eq!(*covered.pixels.get_pixel(0, 0), blue);
        assert_eq!(*covered.pixels.get_pixel(2, 1), blue);
    }

    #[test]
    fn free_nodes_hit_only_checkpoints_at_a_fixed_screen_radius() {
        use super::super::Zoom;
        use gpui::{Bounds, px, size};
        let points = [point(0.1, 0.2), point(0.3, 0.4), point(0.8, 0.9)];
        // The middle point is a freehand sample, not a user checkpoint.
        let nodes = [0, 2];
        for zoom in [Zoom::Percent(50.), Zoom::Percent(400.)] {
            let geometry = PreviewGeometry::new(
                1000,
                1000,
                zoom,
                Bounds::new(point(px(0.), px(0.)), size(px(500.), px(500.))),
                2.,
            )
            .unwrap()
            .panned(point(px(30.), px(-20.)));
            let last = geometry.project(points[2]);
            assert_eq!(
                free_node_at(&points, &nodes, &geometry, last + point(px(5.), px(4.))),
                Some(2)
            );
            assert_eq!(
                free_node_at(&points, &nodes, &geometry, last + point(px(9.), px(0.))),
                None
            );
            assert_eq!(
                free_node_at(&points, &nodes, &geometry, geometry.project(points[1])),
                None
            );
            // Overlapping handles choose the closest checkpoint, including the first.
            assert_eq!(
                free_node_at(&points, &[0, 2, 1], &geometry, geometry.project(points[0])),
                Some(0)
            );
        }
    }

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
