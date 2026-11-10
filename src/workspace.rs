mod canvas_tools;
mod color;
mod cursor;
mod dialogs;
mod files;
mod view;

use std::{cell::Cell, path::PathBuf, rc::Rc};

use futures::{StreamExt, channel::mpsc};
use gpui::{Bounds, Context, FocusHandle, Pixels, Point, ScrollHandle, Window};
use image::RgbaImage;

use crate::commands::{Tool, Zoom};
use crate::document::{CropRect, Document, file_name, render_preview};

use self::{cursor::CanvasCursor, dialogs::Form};

const DEFAULT_COLORS: [u32; 2] = [0x000000, 0xffffff];
const RECENT_COLOR_LIMIT: usize = 12;

struct Tab {
    id: usize,
    document: Document,
    zoom: Zoom,
    pan: Point<Pixels>,
    dirty: bool,
    editing: bool,
    saving: Option<PathBuf>,
}

impl Tab {
    fn busy(&self) -> bool {
        self.editing || self.saving.is_some()
    }
}

#[derive(Clone, Copy)]
struct CropDrag {
    origin: Point<Pixels>,
    start: (u32, u32),
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
    viewport: Rc<Cell<Bounds<Pixels>>>,
    canvas_cursor: Rc<CanvasCursor>,
    pan_drag: Option<(gpui::MouseButton, Point<Pixels>)>,
    crop_drag: Option<CropDrag>,
    crop: Option<CropRect>,
    dialog_open: bool,
    form: Option<Form>,
    creating: bool,
    next_untitled: usize,
    loader: mpsc::UnboundedSender<PathBuf>,
    pending: usize,
    errors: Vec<String>,
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
            viewport: Rc::default(),
            canvas_cursor: Rc::default(),
            pan_drag: None,
            crop_drag: None,
            crop: None,
            dialog_open: false,
            form: None,
            creating: false,
            next_untitled: 1,
            loader,
            pending: 0,
            errors: Vec::new(),
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
        let dirty = document.path.is_none();
        if let Some(path) = &document.path {
            if let Some(index) = self.tabs.iter().position(|tab| {
                tab.document.path.as_ref() == Some(path) || tab.saving.as_ref() == Some(path)
            }) {
                cx.drop_image(document.preview.image, Some(window));
                self.select_tab(index, window, cx);
                return;
            }
            cx.add_recent_document(path);
        }
        self.tabs.push(Tab {
            id: self.next_id,
            document,
            zoom: Zoom::default(),
            pan: Point::default(),
            dirty,
            editing: false,
            saving: None,
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
        if self.active != Some(index) {
            self.clear_crop();
        }
        self.active = Some(index);
        self.pan_drag = None;
        self.tab_scroll.scroll_to_item(index);
        self.update_title(window);
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
        if self.active == Some(index) {
            self.clear_crop();
        }
        let removed = self.tabs.remove(index);
        cx.drop_image(removed.document.preview.image, Some(window));

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
        cx.notify();
    }

    fn clear_crop(&mut self) {
        self.crop = None;
        self.crop_drag = None;
    }

    fn edit_pixels(
        &mut self,
        reset_view: bool,
        edit: impl FnOnce(&RgbaImage) -> Option<RgbaImage> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active.map(|index| &mut self.tabs[index]) else {
            return;
        };
        if tab.busy() {
            return;
        }
        tab.editing = true;
        let id = tab.id;
        let source = tab.document.pixels.clone();
        let job = cx.background_executor().spawn(async move {
            let pixels = edit(&source)?;
            let preview = render_preview(&pixels);
            Some((pixels, preview))
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                let Some(index) = workspace.tab_index(id) else {
                    return;
                };
                let tab = &mut workspace.tabs[index];
                tab.editing = false;
                if let Some((pixels, preview)) = result {
                    tab.document.pixels = pixels.into();
                    let old = std::mem::replace(&mut tab.document.preview, preview);
                    cx.drop_image(old.image, Some(window));
                    tab.dirty = true;
                    if reset_view {
                        tab.pan = Point::default();
                    }
                    workspace.update_title(window);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn update_title(&self, window: &mut Window) {
        let title = self.active_tab().map_or_else(
            || "ImageEdit".to_owned(),
            |tab| {
                let marker = if tab.dirty { " •" } else { "" };
                format!("{}{marker} — ImageEdit", tab.document.name)
            },
        );
        window.set_window_title(&title);
        window.set_window_edited(self.active_tab().is_some_and(|tab| tab.dirty));
    }
}
