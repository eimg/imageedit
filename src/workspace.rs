mod adjust_ui;
mod canvas_tools;
mod clipboard;
mod color;
mod cursor;
mod dialogs;
mod files;
mod filter_ui;
mod gradient_ui;
mod guide_ui;
mod history;
mod layers;
mod numeric;
mod paint_ui;
mod path_ui;
mod pdf_ui;
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
    pdf_ui::PdfTarget,
};

const DEFAULT_COLORS: [u32; 2] = [0x000000, 0xffffff];
const RECENT_COLOR_LIMIT: usize = 12;

type PreparedChange = (
    DocumentContent,
    LayerId,
    Selection,
    crate::document::CanvasPatch,
);

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
    // Keep a recoverable draft until its background commit has published.
    pending_text: Option<text_ui::TextDraft>,
    // Keep the slider at the released value while the canvas patch renders.
    pending_opacity: Option<(LayerId, u8)>,
    pending_guides: Option<Vec<crate::guides::Guide>>,
}

impl Tab {
    fn layer_opacity(&self) -> u8 {
        self.pending_opacity
            .filter(|(id, _)| *id == self.active_layer)
            .map(|(_, opacity)| opacity)
            .or_else(|| {
                self.document
                    .content
                    .layer(self.active_layer)
                    .map(|layer| layer.opacity)
            })
            .unwrap_or(255)
    }

    fn busy(&self) -> bool {
        self.editing || self.saving.is_some()
    }

    fn dirty(&self) -> bool {
        self.history.is_dirty()
            || self
                .pending_text
                .as_ref()
                .is_some_and(text_ui::TextDraft::changed)
    }

    fn settle_text_commit(&mut self, published: bool) -> bool {
        if published {
            self.pending_text = None;
            return false;
        }
        if self.pending_text.is_none() {
            return false;
        }
        self.notice = Some(
            "Text could not be committed. The draft was kept; check its dimensions and the layer limit."
                .into(),
        );
        true
    }

    fn state(&self) -> State {
        State {
            content: self.document.content.clone(),
            active_layer: self.active_layer,
            selection: self.selection.clone(),
        }
    }

    // Return replaced or rejected images to the workspace for GPU cache cleanup.
    fn publish_change(
        &mut self,
        generation: u64,
        change: Change,
        result: Option<PreparedChange>,
        stamp: u64,
    ) -> (bool, Vec<Arc<RenderImage>>) {
        self.editing = false;
        self.pending_opacity = None;
        self.pending_guides = None;
        let Some((content, edited_layer, edited_selection, patch)) = result else {
            return (false, Vec::new());
        };
        if self.history.generation() != generation {
            return (false, patch.into_images());
        }
        let current = self.state();
        let (content, active_layer, selection) = match change {
            Change::Edit | Change::Stroke { .. } | Change::Selection => {
                self.history
                    .commit(current, stamp, !matches!(change, Change::Selection));
                (content, edited_layer, edited_selection)
            }
            Change::Undo | Change::Redo => {
                let state = if matches!(change, Change::Undo) {
                    self.history.undo(current, stamp)
                } else {
                    self.history.redo(current, stamp)
                };
                // Another tab can evict this history target while the patch is rendering.
                let Some(state) = state else {
                    return (false, patch.into_images());
                };
                (state.content, state.active_layer, state.selection)
            }
        };
        match change {
            Change::Stroke { layer, end } => self.stroke_anchor = Some((layer, end.0, end.1)),
            Change::Selection => {}
            _ => self.stroke_anchor = None,
        }
        self.selection = selection;
        self.active_layer = if content.layer(active_layer).is_some() {
            active_layer
        } else {
            content.top_layer_id()
        };
        if content.size() != self.document.content.size() {
            self.pan = Point::default();
        }
        self.document.content = content;
        let replaced = self.document.canvas.apply(patch);
        self.history
            .enforce_tab_limits(&self.document.content, &self.selection, &LIMITS);
        (true, replaced)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum InspectorTab {
    #[default]
    Layers,
    Paths,
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
    numeric_focus: FocusHandle,
    numeric_edit: Option<numeric::NumericEdit>,
    numeric_followup: Option<numeric::PendingNumeric>,
    tab_scroll: ScrollHandle,
    tabs: Vec<Tab>,
    active: Option<usize>,
    next_id: usize,
    tool: Tool,
    colors: [u32; 2],
    recent_colors: Vec<u32>,
    show_grid: bool,
    show_guides: bool,
    guide_drag: Option<guide_ui::GuideDrag>,
    snap_grid: bool,
    snap_guides: bool,
    grid_spacing: u32,
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
    scale_token: u64,
    scale_lock: bool,
    move_settled: Option<(i32, i32)>,
    move_token: u64,
    pending_nudge: Option<layers::PendingNudge>,
    warp: Option<transform_ui::WarpSession>,
    warp_overlay: Option<layers::MovePaint>,
    warp_token: u64,
    warp_busy: bool,
    warp_dirty: bool,
    warp_applying: bool,
    warp_error: Option<&'static str>,
    warp_resample: crate::transform::Resample,
    brushes: paint_ui::BrushSettings,
    fill_mode: FillMode,
    sample_all: bool,
    wand_sample_all: bool,
    gradient_shape: crate::paint::GradientShape,
    gradient_blend: crate::paint::GradientBlend,
    clone_source: Option<paint_ui::CloneSource>,
    clone_aligned: bool,
    inspector_tab: InspectorTab,
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
    path_stroke_style: stroke_ui::PathStrokeStyle,
    stroke: Option<paint_ui::StrokeSession>,
    stroke_commit_preview: Option<(usize, Vec<paint_ui::StrokePaint>)>,
    text_draft: Option<text_ui::TextDraft>,
    text_generation: u64,
    text_busy: bool,
    text_dirty: bool,
    text_error: Option<crate::text::TextError>,
    text_overlay: Option<layers::MovePaint>,
    // The tab whose committed text still shows the preview until its patch lands.
    text_settled: Option<usize>,
    text_continuation: Option<(usize, text_ui::TextContinuation)>,
    text_defaults: text_ui::TextDefaults,
    font_families: Option<Arc<crate::text::FontFamilies>>,
    font_loading: bool,
    font_list_open: bool,
    font_search: text_ui::FontSearch,
    font_search_focus: FocusHandle,
    font_scroll: ScrollHandle,
    font_input_scroll: ScrollHandle,
    caret_shown_at: std::time::Instant,
    caret_blinking: bool,
    copy_token: u64,
    paste_token: u64,
    pdf_import_token: u64,
    pdf_busy: bool,
    dialog_open: bool,
    form: Option<Form>,
    creating: bool,
    next_untitled: usize,
    loader: mpsc::UnboundedSender<PathBuf>,
    pending: usize,
    errors: Vec<String>,
    history_clock: u64,
    jpeg_quality: u8,
    opacity_drag: Option<layers::OpacityDrag>,
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
                workspace.cancel_guide(cx);
                workspace.pending_nudge = None;
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
        let numeric_focus = cx.focus_handle();
        let font_search_focus = cx.focus_handle();
        cx.on_blur(&font_search_focus, window, |workspace, _, cx| {
            workspace.font_list_open = false;
            cx.notify();
        })
        .detach();
        cx.on_blur(&numeric_focus, window, |workspace, window, cx| {
            workspace.commit_numeric_edit(window, cx);
        })
        .detach();
        let numeric_entity = cx.weak_entity();
        // Inline editors consume keystrokes before key bindings and element
        // listeners run. Stopping propagation here also skips `on_key_down`,
        // so the edit must be applied in the interceptor itself.
        cx.intercept_keystrokes(move |event, window, cx| {
            let handled = numeric_entity
                .update(cx, |workspace, cx| {
                    if workspace.font_list_open && workspace.font_search_focus.is_focused(window) {
                        return workspace.font_search_key(&event.keystroke, window, cx);
                    }
                    if workspace.numeric_edit.is_none() {
                        return false;
                    }
                    if workspace.numeric_key(&event.keystroke, window, cx) {
                        return true;
                    }
                    // Unhandled shortcuts such as Undo or Save act on the
                    // committed value rather than a half-typed draft.
                    workspace.commit_numeric_with_followup(
                        numeric::NumericFollowup::Key(event.keystroke.clone()),
                        window,
                        cx,
                    )
                })
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        })
        .detach();
        let (loader, mut requests) = mpsc::unbounded::<PathBuf>();
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |workspace, cx| {
            while let Some(path) = requests.next().await {
                let name = file_name(&path);
                if crate::pdf::is_pdf_path(&path) {
                    let probe_path = path.clone();
                    let info = executor
                        .spawn(async move { crate::pdf::read_info(&probe_path) })
                        .await;
                    if workspace
                        .update_in(cx, |workspace, window, cx| {
                            workspace.pending -= 1;
                            match info {
                                Ok(info) => {
                                    workspace.pdf_import_form(
                                        path,
                                        info,
                                        PdfTarget::NewDocument,
                                        window,
                                        cx,
                                    );
                                }
                                Err(error) => {
                                    workspace.errors.push(format!("{name}: {error:#}"));
                                }
                            }
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }
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
            numeric_focus,
            numeric_edit: None,
            numeric_followup: None,
            tab_scroll: ScrollHandle::new(),
            tabs: Vec::new(),
            active: None,
            next_id: 0,
            tool: Tool::Move,
            colors: DEFAULT_COLORS,
            recent_colors: Vec::new(),
            show_grid: false,
            show_guides: false,
            guide_drag: None,
            snap_grid: false,
            snap_guides: false,
            grid_spacing: 16,
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
            scale_token: 0,
            scale_lock: true,
            move_settled: None,
            move_token: 0,
            pending_nudge: None,
            warp: None,
            warp_overlay: None,
            warp_token: 0,
            warp_busy: false,
            warp_dirty: false,
            warp_applying: false,
            warp_error: None,
            warp_resample: crate::transform::Resample::Bilinear,
            brushes: paint_ui::BrushSettings::default(),
            fill_mode: FillMode::Connected,
            sample_all: false,
            wand_sample_all: false,
            gradient_shape: crate::paint::GradientShape::Linear,
            gradient_blend: crate::paint::GradientBlend::ForegroundToBackground,
            clone_source: None,
            clone_aligned: true,
            inspector_tab: InspectorTab::Layers,
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
            path_stroke_style: stroke_ui::PathStrokeStyle::Brush,
            stroke: None,
            stroke_commit_preview: None,
            text_draft: None,
            text_generation: 0,
            text_busy: false,
            text_dirty: false,
            text_error: None,
            text_overlay: None,
            text_settled: None,
            text_continuation: None,
            text_defaults: text_ui::TextDefaults::default(),
            font_families: None,
            font_loading: false,
            font_list_open: false,
            font_search: text_ui::FontSearch::default(),
            font_search_focus,
            font_scroll: ScrollHandle::new(),
            font_input_scroll: ScrollHandle::new(),
            caret_shown_at: std::time::Instant::now(),
            caret_blinking: false,
            copy_token: 0,
            paste_token: 0,
            pdf_import_token: 0,
            pdf_busy: false,
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
            pending_text: None,
            pending_opacity: None,
            pending_guides: None,
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
        if switched {
            self.cancel_guide(cx);
            self.pending_nudge = None;
            self.clone_source = None;
            self.commit_numeric_edit(window, cx);
            self.numeric_followup = None;
            // A commit that starts here finishes on the tab it belongs to.
            self.finish_text(window, cx);
            self.drop_settled_text(window, cx);
            self.text_continuation = None;
            self.dismiss_form(window, cx);
            self.clear_path_edit();
            self.clear_crop();
            self.clear_selection_gesture();
            self.clear_move_preview(window, cx);
            self.cancel_warp(window, cx);
            self.cancel_stroke(window, cx);
            self.cancel_gradient(window, cx);
            self.cancel_scale(window, cx);
            self.cancel_text(window, cx);
            self.opacity_drag = None;
        }
        self.active = Some(index);
        self.recover_pending_text(window, cx);
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
            self.cancel_guide(cx);
            self.pending_nudge = None;
            self.clone_source = None;
            self.numeric_followup = None;
            self.opacity_drag = None;
            self.dismiss_form(window, cx);
            self.clear_path_edit();
            self.clear_crop();
            self.clear_selection_gesture();
            self.clear_move_preview(window, cx);
            self.cancel_warp(window, cx);
            self.cancel_stroke(window, cx);
            self.cancel_gradient(window, cx);
            self.cancel_scale(window, cx);
            self.cancel_text(window, cx);
            self.drop_settled_text(window, cx);
            self.text_continuation = None;
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
        self.recover_pending_text(window, cx);
        self.update_title(window);
        if closed_active && self.active.is_some() {
            self.begin_warp(window, cx);
        }
        cx.notify();
    }

    fn clear_crop(&mut self) {
        if self
            .numeric_edit
            .as_ref()
            .is_some_and(|edit| edit.is_crop())
        {
            self.numeric_edit = None;
        }
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
        // Typed text lands as its own undo step before this change runs.
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| {
                this.start_change(change, edit, window, cx);
            });
            return;
        }
        self.cancel_guide(cx);
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

    fn finish_change(
        &mut self,
        id: usize,
        generation: u64,
        change: Change,
        result: Option<PreparedChange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.text_settled == Some(id) {
            self.drop_settled_text(window, cx);
        }
        let published = self.publish_change(id, generation, change, result, window, cx);
        self.finish_clone_alignment(id, generation, published);
        if let Some(index) = self.tab_index(id)
            && self.tabs[index].settle_text_commit(published)
        {
            if self
                .text_continuation
                .as_ref()
                .is_some_and(|(tab, _)| *tab == id)
            {
                self.text_continuation = None;
            }
            self.recover_pending_text(window, cx);
        }
        if self
            .text_continuation
            .as_ref()
            .is_some_and(|(tab, _)| *tab == id)
            && let Some((_, then)) = self.text_continuation.take()
            && self.active_tab().is_some_and(|tab| tab.id == id)
        {
            then(self, window, cx);
        }
        self.finish_numeric_followup(id, generation, published, window, cx);
        self.finish_pending_nudge(id, generation, published, window, cx);
    }

    // Publishes a background result only if the tab has not changed since the job started.
    fn publish_change(
        &mut self,
        id: usize,
        generation: u64,
        change: Change,
        result: Option<PreparedChange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self.tab_index(id) else {
            if let Some((_, _, _, patch)) = result {
                drop_images(patch.into_images(), window, cx);
            }
            return false;
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
        let (published, replaced) =
            self.tabs[index].publish_change(generation, change, result, stamp);
        drop_images(replaced, window, cx);
        if !published {
            return false;
        }
        history::enforce_app_budget(
            self.tabs
                .iter_mut()
                .map(|tab| (&mut tab.history, &tab.document.content, &tab.selection)),
            &LIMITS,
        );
        self.update_title(window);
        self.ensure_preview_level(window.scale_factor(), window, cx);
        true
    }

    fn update_title(&self, window: &mut Window) {
        let title = self.active_tab().map_or_else(
            || "Lime — Lite Image Editor".to_owned(),
            |tab| {
                let marker = if tab.dirty() { " •" } else { "" };
                format!("{}{marker} — Lime", tab.document.name)
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

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn opacity_tab() -> Tab {
        let document = Document::blank("Opacity".into(), 2, 2, image::Rgba([255; 4]));
        Tab {
            id: 0,
            stroke_anchor: None,
            active_layer: document.content.preferred_active_layer(),
            document,
            zoom: Zoom::default(),
            pan: Point::default(),
            history: History::new(true),
            editing: false,
            saving: None,
            exporting: false,
            notice: None,
            building_level: None,
            selection: Selection::Unrestricted,
            pending_text: None,
            pending_opacity: None,
            pending_guides: None,
        }
    }

    fn prepare(tab: &Tab, state: State, dirty: Dirty) -> Option<PreparedChange> {
        RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            dirty,
        )
        .map(|patch| (state.content, state.active_layer, state.selection, patch))
    }

    fn prepare_opacity(tab: &Tab, opacity: u8) -> Option<PreparedChange> {
        let mut state = tab.state();
        let dirty = state.content.change_opacity(state.active_layer, opacity);
        prepare(tab, state, dirty)
    }

    #[test]
    fn released_opacity_stays_visible_until_the_canvas_commit_lands() {
        let mut tab = opacity_tab();
        let layer = tab.active_layer;
        let generation = tab.history.generation();
        let result = prepare_opacity(&tab, 80);
        tab.editing = true;
        tab.pending_opacity = Some((layer, 80));

        // The background job has not published: document and history are still original,
        // but repainting the slider must never show that old value.
        assert_eq!(tab.document.content.layer(layer).unwrap().opacity, 255);
        assert_eq!(tab.layer_opacity(), 80);
        assert!(!tab.history.is_dirty());
        assert_eq!(opacity_tab().layer_opacity(), 255);

        assert!(tab.publish_change(generation, Change::Edit, result, 1).0);
        assert_eq!(tab.layer_opacity(), 80);
        assert!(!tab.busy());
        assert!(tab.pending_opacity.is_none());
        assert_eq!(
            *tab.document.canvas.pixels,
            *tab.document.content.composite()
        );

        let previous = tab.history.undo_target().unwrap().0.clone();
        let result = prepare(&tab, previous, Dirty::Canvas);
        assert!(
            tab.publish_change(tab.history.generation(), Change::Undo, result, 2)
                .0
        );
        assert_eq!(tab.layer_opacity(), 255);
        assert!(tab.history.undo_target().is_none());
        assert!(!tab.history.is_dirty());

        let next = tab.history.redo_target().unwrap().0.clone();
        let result = prepare(&tab, next, Dirty::Canvas);
        assert!(
            tab.publish_change(tab.history.generation(), Change::Redo, result, 3)
                .0
        );
        assert_eq!(tab.layer_opacity(), 80);
        assert_eq!(
            *tab.document.canvas.pixels,
            *tab.document.content.composite()
        );
    }

    #[test]
    fn pending_opacity_only_overrides_its_layer_and_clears_on_failure() {
        let mut tab = opacity_tab();
        tab.pending_opacity = Some((LayerId(tab.active_layer.0 + 1), 80));
        assert_eq!(tab.layer_opacity(), 255);

        tab.pending_opacity = Some((tab.active_layer, 0));
        assert_eq!(tab.layer_opacity(), 0);
        assert!(
            !tab.publish_change(tab.history.generation(), Change::Edit, None, 1)
                .0
        );
        assert_eq!(tab.layer_opacity(), 255);
        assert!(tab.pending_opacity.is_none());
        assert!(tab.history.undo_target().is_none());
    }

    #[test]
    fn stale_opacity_results_cannot_change_pixels_or_history() {
        let mut tab = opacity_tab();
        let generation = tab.history.generation();
        let result = prepare_opacity(&tab, 0);
        let pixels = tab.document.canvas.pixels.clone();
        tab.editing = true;
        tab.pending_opacity = Some((tab.active_layer, 0));
        tab.history.commit(tab.state(), 1, false);
        let current_generation = tab.history.generation();

        let (published, rejected) = tab.publish_change(generation, Change::Edit, result, 2);
        assert!(!published);
        assert!(rejected.is_empty());
        assert!(Arc::ptr_eq(&pixels, &tab.document.canvas.pixels));
        assert_eq!(tab.history.generation(), current_generation);
        assert_eq!(tab.layer_opacity(), 255);
        assert!(!tab.busy());
        assert!(tab.pending_opacity.is_none());
        assert!(!tab.history.is_dirty());
    }

    #[test]
    fn unchanged_opacity_does_not_create_an_undo_entry() {
        let mut tab = opacity_tab();
        let result = prepare_opacity(&tab, 255);
        assert!(result.is_none());
        assert!(
            !tab.publish_change(tab.history.generation(), Change::Edit, result, 1)
                .0
        );
        assert!(tab.history.undo_target().is_none());
        assert!(!tab.history.is_dirty());
    }
}
