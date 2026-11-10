mod view;

use std::{cell::Cell, path::PathBuf, rc::Rc};

use futures::{StreamExt, channel::mpsc};
use gpui::{Bounds, Context, FocusHandle, PathPromptOptions, Pixels, Point, ScrollHandle, Window};

use crate::commands::{Tool, Zoom};
use crate::document::{Document, file_name};

const DEFAULT_COLORS: [u32; 2] = [0x000000, 0xffffff];

struct Tab {
    id: usize,
    document: Document,
    zoom: Zoom,
    pan: Point<Pixels>,
}

pub struct Workspace {
    focus: FocusHandle,
    tab_scroll: ScrollHandle,
    tabs: Vec<Tab>,
    active: Option<usize>,
    next_id: usize,
    tool: Tool,
    colors: [u32; 2],
    show_grid: bool,
    show_guides: bool,
    viewport: Rc<Cell<Bounds<Pixels>>>,
    pan_drag: Option<Point<Pixels>>,
    dialog_open: bool,
    loader: mpsc::UnboundedSender<PathBuf>,
    pending: usize,
    errors: Vec<String>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            show_grid: false,
            show_guides: false,
            viewport: Rc::default(),
            pan_drag: None,
            dialog_open: false,
            loader,
            pending: 0,
            errors: Vec::new(),
        }
    }

    pub fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open {
            return;
        }
        self.dialog_open = true;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = prompt.await;
            let _ = workspace.update_in(cx, |workspace, _, cx| {
                workspace.dialog_open = false;
                match result {
                    Ok(Ok(Some(paths))) => workspace.open_paths(paths, cx),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => workspace
                        .errors
                        .push(format!("Could not open the file picker: {error}")),
                    Err(_) => workspace
                        .errors
                        .push("The file picker closed unexpectedly.".into()),
                }
                cx.notify();
            });
        })
        .detach();
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
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.document.path == document.path)
        {
            self.select_tab(index, window, cx);
            return;
        }

        cx.add_recent_document(&document.path);
        self.tabs.push(Tab {
            id: self.next_id,
            document,
            zoom: Zoom::default(),
            pan: Point::default(),
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
        let removed = self.tabs.remove(index);
        cx.drop_image(removed.document.preview, Some(window));

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

    fn update_title(&self, window: &mut Window) {
        let title = self.active_document().map_or_else(
            || "ImageEdit".to_owned(),
            |document| format!("{} — ImageEdit", document.name),
        );
        window.set_window_title(&title);
    }
}
