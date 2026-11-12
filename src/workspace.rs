mod adjust_ui;
mod canvas_tools;
mod clipboard;
mod color;
mod cursor;
mod dialogs;
mod files;
mod filter_ui;
mod gradient_ui;
mod history;
mod layers;
mod paint_ui;
mod path_ui;
mod scale_ui;
mod selection;
mod stroke_ui;
mod text_ui;
mod tooltip;
mod transform_ui;
mod view;

use std::{cell::Cell, path::PathBuf, rc::Rc, sync::Arc};

use futures::{StreamExt, channel::mpsc};
use gpui::{Bounds, Context, FocusHandle, Pixels, Point, RenderImage, ScrollHandle, Window};

use crate::commands::{Tool, Zoom};
use crate::document::{
    CropRect, DEFAULT_JPEG_QUALITY, Document, DocumentContent, FillMode, LayerId, RenderedCanvas,
    build_level, file_name,
};
use crate::preview::{self, Dirty};
use crate::selection::Selection;

use self::{
    cursor::CanvasCursor,
    dialogs::Form,
    history::{History, LIMITS, State},
};

const DEFAULT_COLORS: [u32; 2] = [0x000000, 0xffffff];
const RECENT_COLOR_LIMIT: usize = 12;

struct Tab {
    id: usize,
    stroke_anchor: Option<(LayerId, f64, f64)>,
    document: Document,
    active_layer: LayerId,
    zoom: Zoom,
    pan: Point<Pixels>,
    history: History,
    editing: bool,
    saving: Option<PathBuf>,
    exporting: bool,
    notice: Option<String>,
    building_level: Option<u32>,
    selection: Selection,
}

impl Tab {
    fn busy(&self) -> bool {
        self.editing || self.saving.is_some()
    }

    fn dirty(&self) -> bool {
        self.history.is_dirty()
    }

    fn state(&self) -> State {
        State {
            content: self.document.content.clone(),
            active_layer: self.active_layer,
            selection: self.selection.clone(),
        }
    }
}

#[derive(Clone, Copy)]
enum Change {
    Edit,
    Stroke { layer: LayerId, end: (f64, f64) },
    Selection,
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CropEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

#[derive(Clone, Copy)]
enum CropDrag {
    Create {
        origin: Point<Pixels>,
        start: (u32, u32),
    },
    Move {
        crop: CropRect,
        grab: (i64, i64),
    },
    Resize {
        crop: CropRect,
        edges: CropEdges,
        anchor: (i64, i64),
    },
}

pub struct Workspace {
    focus: FocusHandle,
    tab_scroll: ScrollHandle,
    tabs: Vec<Tab>,
    active: Option<usize>,
    next_id: usize,
    tool: Tool,
    colors: [u32; 2],
    recent_colors: Vec<u32>,
    show_grid: bool,
    show_guides: bool,
    show_layer_bounds: bool,
    viewport: Rc<Cell<Bounds<Pixels>>>,
    canvas_cursor: Rc<CanvasCursor>,
    pan_drag: Option<(gpui::MouseButton, Point<Pixels>)>,
    space_pan: bool,
    crop_drag: Option<CropDrag>,
    crop: Option<CropRect>,
    selection_drag: Option<selection::SelectionGesture>,
    selection_preview: Option<Selection>,
    oval_circle: bool,
    selection_radius: u32,
    tolerance: u8,
    free_points: Vec<Point<f32>>,
    free_op: crate::selection::SelectionOp,
    free_drag_origin: Option<Point<Pixels>>,
    free_dragging: bool,
    move_drag: Option<layers::MoveDrag>,
    move_overlay: Option<layers::MovePaint>,
    scale_drag: Option<scale_ui::ScaleDrag>,
    scale_overlay: Option<layers::MovePaint>,
    move_settled: Option<(i32, i32)>,
    move_token: u64,
    warp: Option<transform_ui::WarpSession>,
    warp_overlay: Option<layers::MovePaint>,
    warp_token: u64,
    warp_busy: bool,
    warp_dirty: bool,
    warp_applying: bool,
    warp_error: Option<&'static str>,
    warp_resample: crate::transform::Resample,
    brush: crate::paint::Brush,
    fill_mode: FillMode,
    sample_all: bool,
    gradient_shape: crate::paint::GradientShape,
    gradient_blend: crate::paint::GradientBlend,
    clone_source: Option<(u32, u32)>,
    gradient: Option<gradient_ui::GradientSession>,
    gradient_overlay: Option<layers::MovePaint>,
    gradient_token: u64,
    gradient_busy: bool,
    gradient_dirty: bool,
    adjustment: Option<adjust_ui::AdjustmentSession>,
    adjustment_preview: Option<Arc<RenderImage>>,
    adjustment_generation: crate::adjustments::PreviewGeneration,
    adjustment_busy: bool,
    adjustment_dirty: bool,
    adjustment_histogram: Option<crate::adjustments::Histogram>,
    histogram_token: u64,
    filter: Option<filter_ui::FilterSession>,
    filter_preview: Option<Arc<RenderImage>>,
    filter_generation: crate::adjustments::PreviewGeneration,
    filter_busy: bool,
    filter_dirty: bool,
    path_gesture: Option<path_ui::PathGesture>,
    path_hold: Option<crate::paths::VectorPath>,
    path_anchor: Option<(crate::paths::PathId, crate::paths::AnchorId)>,
    stroke_source: stroke_ui::StrokeSource,
    stroke: Option<paint_ui::StrokeSession>,
    stroke_commit_preview: Option<(usize, Vec<paint_ui::StrokePaint>)>,
    text_draft: Option<text_ui::TextDraft>,
    text_generation: u64,
    text_busy: bool,
    text_dirty: bool,
    text_error: Option<crate::text::TextError>,
    text_overlay: Option<layers::MovePaint>,
    pencil: crate::paint::Brush,
    copy_token: u64,
    paste_token: u64,
    dialog_open: bool,
    form: Option<Form>,
    creating: bool,
    next_untitled: usize,
    loader: mpsc::UnboundedSender<PathBuf>,
    pending: usize,
    errors: Vec<String>,
    history_clock: u64,
    jpeg_quality: u8,
    opacity_drag: Option<(LayerId, u8)>,
    toolbox_width: f32,
    toolbox_folded: bool,
    toolbox_drag: Option<(f32, f32)>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let entity = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            entity
                .update(cx, |workspace, cx| {
                    workspace.request_exit(false, window, cx)
                })
                .unwrap_or(true)
        });
        cx.observe_window_activation(window, |workspace, window, cx| {
            if !window.is_window_active() {
                workspace.canvas_cursor.leave();
                workspace.pan_drag = None;
                workspace.space_pan = false;
                workspace.crop_drag = None;
                workspace.selection_drag = None;
                workspace.selection_preview = None;
                workspace.opacity_drag = None;
                workspace.toolbox_drag = None;
                if workspace.tool == Tool::FreeSelect {
                    workspace.free_points.clear();
                    workspace.free_drag_origin = None;
                    workspace.free_dragging = false;
                }
                workspace.cancel_path_gesture(cx);
                if workspace.move_drag.is_some() {
                    workspace.cancel_move(window, cx);
                }
                if workspace.stroke.is_some() {
                    workspace.cancel_stroke(window, cx);
                }
                workspace.end_warp_drag(cx);
                workspace.end_gradient_drag(cx);
                workspace.cancel_scale(window, cx);
            }
            cx.notify();
        })
        .detach();
        let focus = cx.focus_handle();
        focus.focus(window);
        let (loader, mut requests) = mpsc::unbounded::<PathBuf>();
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |workspace, cx| {
            while let Some(path) = requests.next().await {
                let name = file_name(&path);
                let result = executor.spawn(async move { Document::open(&path) }).await;
                if workspace
                    .update_in(cx, |workspace, window, cx| {
                        workspace.pending -= 1;
                        match result {
                            Ok(document) => workspace.show_document(document, window, cx),
                            Err(error) => workspace.errors.push(format!("{name}: {error}")),
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            focus,
            tab_scroll: ScrollHandle::new(),
            tabs: Vec::new(),
            active: None,
            next_id: 0,
            tool: Tool::Move,
            colors: DEFAULT_COLORS,
            recent_colors: Vec::new(),
            show_grid: false,
            show_guides: false,
            show_layer_bounds: false,
            viewport: Rc::default(),
            canvas_cursor: Rc::default(),
            pan_drag: None,
            space_pan: false,
            crop_drag: None,
            crop: None,
            selection_drag: None,
            selection_preview: None,
            oval_circle: false,
            selection_radius: 0,
            tolerance: 0,
            free_points: Vec::new(),
            free_op: crate::selection::SelectionOp::Replace,
            free_drag_origin: None,
            free_dragging: false,
            move_drag: None,
            move_overlay: None,
            scale_drag: None,
            scale_overlay: None,
            move_settled: None,
            move_token: 0,
            warp: None,
            warp_overlay: None,
            warp_token: 0,
            warp_busy: false,
            warp_dirty: false,
            warp_applying: false,
            warp_error: None,
            warp_resample: crate::transform::Resample::Bilinear,
            brush: crate::paint::Brush::default(),
            fill_mode: FillMode::Connected,
            sample_all: false,
            gradient_shape: crate::paint::GradientShape::Linear,
            gradient_blend: crate::paint::GradientBlend::ForegroundToBackground,
            clone_source: None,
            gradient: None,
            gradient_overlay: None,
            gradient_token: 0,
            gradient_busy: false,
            gradient_dirty: false,
            adjustment: None,
            adjustment_preview: None,
            adjustment_generation: crate::adjustments::PreviewGeneration::default(),
            adjustment_busy: false,
            adjustment_dirty: false,
            adjustment_histogram: None,
            histogram_token: 0,
            filter: None,
            filter_preview: None,
            filter_generation: crate::adjustments::PreviewGeneration::default(),
            filter_busy: false,
            filter_dirty: false,
            path_gesture: None,
            path_hold: None,
            path_anchor: None,
            stroke_source: stroke_ui::StrokeSource::Path,
            stroke: None,
            stroke_commit_preview: None,
            text_draft: None,
            text_generation: 0,
            text_busy: false,
            text_dirty: false,
            text_error: None,
            text_overlay: None,
            pencil: crate::paint::Brush {
                diameter: 1,
                ..Default::default()
            },
            copy_token: 0,
            paste_token: 0,
            dialog_open: false,
            form: None,
            creating: false,
            next_untitled: 1,
            loader,
            pending: 0,
            errors: Vec::new(),
            history_clock: 0,
            jpeg_quality: DEFAULT_JPEG_QUALITY,
            opacity_drag: None,
            toolbox_width: view::TOOLBOX_DEFAULT_WIDTH,
            toolbox_folded: false,
            toolbox_drag: None,
        }
    }

    fn set_color(&mut self, swatch: usize, color: u32) {
        self.colors[swatch] = color;
        self.recent_colors.retain(|&recent| recent != color);
        self.recent_colors.insert(0, color);
        self.recent_colors.truncate(RECENT_COLOR_LIMIT);
    }

    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.pending == 0 {
            self.errors.clear();
        }
        for path in paths {
            if self.loader.unbounded_send(path).is_ok() {
                self.pending += 1;
            }
        }
        cx.notify();
    }

    fn show_document(&mut self, document: Document, window: &mut Window, cx: &mut Context<Self>) {
        let history = History::new(document.path.is_some());
        if let Some(path) = &document.path {
            if let Some(index) = self.tabs.iter().position(|tab| {
                tab.document.path.as_ref() == Some(path) || tab.saving.as_ref() == Some(path)
            }) {
                drop_canvas(document.canvas, window, cx);
                self.select_tab(index, window, cx);
                return;
            }
            cx.add_recent_document(path);
        }
        self.tabs.push(Tab {
            stroke_anchor: None,
            id: self.next_id,
            active_layer: document.content.preferred_active_layer(),
            document,
            zoom: Zoom::default(),
            pan: Point::default(),
            history,
            editing: false,
            saving: None,
            exporting: false,
            notice: None,
            building_level: None,
            selection: Selection::Unrestricted,
        });
        self.next_id += 1;
        self.select_tab(self.tabs.len() - 1, window, cx);
    }

    fn active_document(&self) -> Option<&Document> {
        self.active_tab().map(|tab| &tab.document)
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.active.map(|index| &self.tabs[index])
    }

    fn tab_index(&self, id: usize) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let switched = self.active != Some(index);
        if switched && self.refuse_pending_text(cx) {
            return;
        }
        if switched {
            self.dismiss_form(window, cx);
            self.clear_path_edit();
            self.clear_crop();
            self.clear_selection_gesture();
            self.clear_move_preview(window, cx);
            self.cancel_warp(window, cx);
            self.cancel_stroke(window, cx);
            self.cancel_gradient(window, cx);
            self.cancel_text(window, cx);
            self.opacity_drag = None;
        }
        self.active = Some(index);
        self.pan_drag = None;
        self.tab_scroll.scroll_to_item(index);
        self.update_title(window);
        self.ensure_preview_level(window.scale_factor(), window, cx);
        if switched {
            self.begin_warp(window, cx);
        }
        cx.notify();
    }

    fn cycle_tabs(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.active {
            let count = self.tabs.len();
            let step = if backwards { count - 1 } else { 1 };
            self.select_tab((index + step) % count, window, cx);
        }
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.pan_drag = None;
        let closed_active = self.active == Some(index);
        if closed_active {
            self.opacity_drag = None;
            self.dismiss_form(window, cx);
            self.clear_path_edit();
            self.clear_crop();
            self.clear_selection_gesture();
            self.clear_move_preview(window, cx);
            self.cancel_warp(window, cx);
            self.cancel_stroke(window, cx);
            self.cancel_gradient(window, cx);
            self.cancel_text(window, cx);
        }
        let removed = self.tabs.remove(index);
        drop_canvas(removed.document.canvas, window, cx);

        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
            Some(active) if active > index => Some(active - 1),
            active => active,
        };
        if let Some(active) = self.active {
            self.tab_scroll.scroll_to_item(active);
        }
        self.update_title(window);
        if closed_active && self.active.is_some() {
            self.begin_warp(window, cx);
        }
        cx.notify();
    }

    fn clear_crop(&mut self) {
        self.crop = None;
        self.crop_drag = None;
    }

    // The edit may also choose a new active layer.
    fn edit_content(
        &mut self,
        edit: impl FnOnce(&mut DocumentContent, &mut LayerId, &mut Selection) -> Dirty + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_change(Change::Edit, edit, window, cx);
    }

    fn can_step_history(&self, change: Change) -> bool {
        self.text_draft.is_none()
            && self.active_tab().is_some_and(|tab| {
                !tab.busy()
                    && match change {
                        Change::Edit | Change::Stroke { .. } | Change::Selection => false,
                        Change::Undo => tab.history.undo_target().is_some(),
                        Change::Redo => tab.history.redo_target().is_some(),
                    }
            })
    }

    fn step_history(&mut self, change: Change, window: &mut Window, cx: &mut Context<Self>) {
        if self.can_step_history(change) {
            self.clear_crop();
            self.clear_selection_gesture();
            self.clear_move_preview(window, cx);
            self.cancel_warp(window, cx);
            self.cancel_stroke(window, cx);
            self.cancel_gradient(window, cx);
            self.cancel_text(window, cx);
            self.start_change(change, |_, _, _| Dirty::Canvas, window, cx);
        }
    }

    fn start_change(
        &mut self,
        change: Change,
        edit: impl FnOnce(&mut DocumentContent, &mut LayerId, &mut Selection) -> Dirty + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_drag.is_some() {
            return;
        }
        let Some(index) = self.active else {
            return;
        };
        if self.tabs[index].busy() {
            return;
        }
        if self.refuse_pending_text(cx) {
            return;
        }
        self.dismiss_effect_form(window, cx);
        self.clear_crop();
        self.clear_selection_gesture();
        self.cancel_path_gesture(cx);
        self.opacity_drag = None;
        self.cancel_text(window, cx);
        self.cancel_warp(window, cx);
        self.cancel_stroke(window, cx);
        self.cancel_gradient(window, cx);
        self.cancel_scale(window, cx);
        let tab = &mut self.tabs[index];
        self.free_points.clear();
        let target = match change {
            Change::Edit | Change::Stroke { .. } | Change::Selection => None,
            Change::Undo => tab.history.undo_target(),
            Change::Redo => tab.history.redo_target(),
        };
        let mut content = target
            .map_or(&tab.document.content, |(state, _)| &state.content)
            .clone();
        let session_only = target.is_some_and(|(_, revision)| revision == tab.history.revision());
        tab.editing = true;
        tab.notice = None;
        let id = tab.id;
        let generation = tab.history.generation();
        let pixels = tab.document.canvas.pixels.clone();
        let has_transparency = tab.document.canvas.preview.has_transparency;
        let levels = tab.document.canvas.level_caches();
        let mut active_layer = tab.active_layer;
        let mut selection = tab.selection.clone();
        let job = cx.background_executor().spawn(async move {
            let dirty = if session_only {
                Dirty::Unpainted
            } else {
                edit(&mut content, &mut active_layer, &mut selection)
            };
            if dirty != Dirty::None {
                selection.outline_edges();
            }
            RenderedCanvas::prepare_patch(&pixels, has_transparency, levels, &content, dirty)
                .map(|patch| (content, active_layer, selection, patch))
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.finish_change(id, generation, change, result, window, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // Publishes a background result only if the tab has not changed since the job started.
    fn finish_change(
        &mut self,
        id: usize,
        generation: u64,
        change: Change,
        result: Option<(
            DocumentContent,
            LayerId,
            Selection,
            crate::document::CanvasPatch,
        )>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        if self.active == Some(index) && self.move_settled.is_some() {
            self.move_token = self.move_token.wrapping_add(1);
            self.drop_move_overlay(window, cx);
        }
        if self
            .stroke_commit_preview
            .as_ref()
            .is_some_and(|(tab, _)| *tab == id)
        {
            self.clear_stroke_commit_preview(window, cx);
        }
        if self.active == Some(index) {
            self.path_hold = None;
        }
        self.history_clock += 1;
        let stamp = self.history_clock;
        let tab = &mut self.tabs[index];
        tab.editing = false;
        let Some((content, edited_layer, edited_selection, patch)) = result else {
            return;
        };
        if tab.history.generation() != generation {
            drop_images(patch.into_images(), window, cx);
            return;
        }
        let current = tab.state();
        let (content, active_layer, selection) = match change {
            Change::Edit | Change::Stroke { .. } | Change::Selection => {
                tab.history
                    .commit(current, stamp, !matches!(change, Change::Selection));
                (content, edited_layer, edited_selection)
            }
            Change::Undo | Change::Redo => {
                let state = if matches!(change, Change::Undo) {
                    tab.history.undo(current, stamp)
                } else {
                    tab.history.redo(current, stamp)
                };
                // Another tab can evict this history target while the patch is rendering.
                let Some(state) = state else {
                    drop_images(patch.into_images(), window, cx);
                    return;
                };
                (state.content, state.active_layer, state.selection)
            }
        };
        match change {
            Change::Stroke { layer, end } => tab.stroke_anchor = Some((layer, end.0, end.1)),
            Change::Selection => {}
            _ => tab.stroke_anchor = None,
        }
        tab.selection = selection;
        tab.active_layer = if content.layer(active_layer).is_some() {
            active_layer
        } else {
            content.top_layer_id()
        };
        if content.size() != tab.document.content.size() {
            tab.pan = Point::default();
        }
        tab.document.content = content;
        let replaced = tab.document.canvas.apply(patch);
        drop_images(replaced, window, cx);
        tab.history
            .enforce_tab_limits(&tab.document.content, &tab.selection, &LIMITS);
        history::enforce_app_budget(
            self.tabs
                .iter_mut()
                .map(|tab| (&mut tab.history, &tab.document.content, &tab.selection)),
            &LIMITS,
        );
        self.update_title(window);
        self.ensure_preview_level(window.scale_factor(), window, cx);
    }

    fn update_title(&self, window: &mut Window) {
        let title = self.active_tab().map_or_else(
            || "ImageEdit".to_owned(),
            |tab| {
                let marker = if tab.dirty() { " •" } else { "" };
                format!("{}{marker} — ImageEdit", tab.document.name)
            },
        );
        window.set_window_title(&title);
        window.set_window_edited(self.active_tab().is_some_and(|tab| tab.dirty()));
    }
}

impl Workspace {
    fn request_preview_level(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scale = window.scale_factor();
        let Some(index) = self.active else {
            return;
        };
        let tab = &self.tabs[index];
        let Some(shift) = preview_shift(tab, self.viewport.get(), scale) else {
            return;
        };
        if shift == 0
            || tab.document.canvas.has_level(shift)
            || tab.building_level.is_some()
            || tab.busy()
        {
            return;
        }
        cx.defer_in(window, move |workspace, window, cx| {
            workspace.ensure_preview_level(scale, window, cx);
        });
    }

    pub(super) fn ensure_preview_level(
        &mut self,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.active else {
            return;
        };
        let Some(shift) = preview_shift(&self.tabs[index], self.viewport.get(), scale) else {
            return;
        };
        let tab = &mut self.tabs[index];
        if shift == 0
            || tab.document.canvas.has_level(shift)
            || tab.building_level.is_some()
            || tab.busy()
        {
            return;
        }
        tab.building_level = Some(shift);
        let id = tab.id;
        let source_pixels = Arc::clone(&tab.document.canvas.pixels);
        let content = tab.document.content.clone();
        let job = cx
            .background_executor()
            .spawn(async move { build_level(&content, shift) });
        cx.spawn_in(window, async move |workspace, cx| {
            let cache = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                let Some(index) = workspace.tab_index(id) else {
                    return;
                };
                let tab = &mut workspace.tabs[index];
                tab.building_level = None;
                // A rename keeps the same composite. A painted edit replaces it, so this
                // reduction belongs to the previous pixels and must not be installed.
                if Arc::ptr_eq(&tab.document.canvas.pixels, &source_pixels) {
                    let replaced = tab.document.canvas.install_cache(cache);
                    drop_images(replaced, window, cx);
                    cx.notify();
                }
                workspace.ensure_preview_level(window.scale_factor(), window, cx);
            });
        })
        .detach();
    }
}

fn preview_shift(tab: &Tab, viewport: Bounds<Pixels>, scale: f32) -> Option<u32> {
    let geometry = canvas_tools::PreviewGeometry::for_tab(tab, viewport, scale)?;
    let display = f32::from(geometry.bounds.size.width)
        .max(f32::from(geometry.bounds.size.height))
        .max(1.)
        * scale;
    Some(preview::fit_level_shift(
        tab.document.width().max(tab.document.height()),
        display as u32,
    ))
}

fn drop_images(
    images: impl IntoIterator<Item = Arc<RenderImage>>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    for image in images {
        cx.drop_image(image, Some(window));
    }
}

fn drop_canvas(canvas: RenderedCanvas, window: &mut Window, cx: &mut Context<Workspace>) {
    for image in canvas.into_images() {
        cx.drop_image(image, Some(window));
    }
}
