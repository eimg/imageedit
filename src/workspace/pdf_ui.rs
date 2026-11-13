use std::path::{Path, PathBuf};

use gpui::{Context, Window};

use super::Workspace;
use crate::{
    document::{self, Document, DocumentContent, ImageLayer, InsertStop, LayerId},
    pdf::{self, PdfMode, PdfOptions},
    preview::Dirty,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PdfTarget {
    NewDocument,
    Layers {
        tab_id: usize,
        generation: u64,
        above: LayerId,
    },
}

impl Workspace {
    pub(super) fn request_pdf_import(
        &mut self,
        path: PathBuf,
        target: PdfTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog_open || self.form.is_some() || self.creating || self.pdf_busy {
            self.errors
                .push("Wait for the current file operation to finish.".into());
            cx.notify();
            return;
        }
        let name = document::file_name(&path);
        self.dialog_open = true;
        let job_path = path.clone();
        let job = cx
            .background_executor()
            .spawn(async move { pdf::read_info(&job_path) });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.dialog_open = false;
                match result {
                    Ok(info) => {
                        workspace.pdf_import_form(path, info, target, window, cx);
                    }
                    Err(error) => workspace.errors.push(format!("{name}: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn begin_pdf_import(
        &mut self,
        options: PdfOptions,
        target: PdfTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pdf_busy {
            self.errors
                .push("Wait for the current PDF import to finish.".into());
            cx.notify();
            return;
        }
        self.pdf_import_token = self.pdf_import_token.wrapping_add(1);
        let token = self.pdf_import_token;
        let new_document = matches!(target, PdfTarget::NewDocument);
        self.pdf_busy = true;
        if new_document {
            self.creating = true;
        }
        let name = document::file_name(&options.path);
        let job_options = options.clone();
        let job = cx
            .background_executor()
            .spawn(async move { pdf::render_pages(&job_options) });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                if workspace.pdf_import_token != token {
                    return;
                }
                workspace.pdf_busy = false;
                if new_document {
                    workspace.creating = false;
                }
                match result {
                    Ok(pages) => match target {
                        PdfTarget::NewDocument => {
                            workspace.show_pdf_documents(&options, pages, window, cx);
                        }
                        PdfTarget::Layers { .. } => {
                            workspace.insert_pdf_layers(target, options.first, pages, window, cx)
                        }
                    },
                    Err(error) => {
                        let message = format!("{name}: {error:#}");
                        if new_document {
                            workspace.errors.push(message);
                        } else {
                            workspace.notice(&message, cx);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn show_pdf_documents(
        &mut self,
        options: &PdfOptions,
        pages: Vec<image::RgbaImage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = document::file_name(&options.path);
        if options.mode == PdfMode::Separate {
            let count = pages.len();
            let stem = Path::new(&name)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.trim().is_empty())
                .unwrap_or("PDF");
            for (index, pixels) in pages.into_iter().enumerate() {
                let title = if count == 1 {
                    name.clone()
                } else {
                    format!("{stem} ({})", options.first + index)
                };
                let content = DocumentContent::single_layer(
                    format!("Page {}", options.first + index),
                    pixels,
                );
                self.show_document(Document::from_content(title, content), window, cx);
            }
        } else {
            let content = document_from_layers(pages, options.first);
            self.show_document(Document::from_content(name, content), window, cx);
        }
    }

    fn insert_pdf_layers(
        &mut self,
        target: PdfTarget,
        first_page: usize,
        pages: Vec<image::RgbaImage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PdfTarget::Layers {
            tab_id,
            generation,
            above,
        } = target
        else {
            return;
        };
        if self.active_tab().map(|tab| tab.id) != Some(tab_id) {
            self.notice(
                "Open as Layers was canceled because the active tab changed.",
                cx,
            );
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.history.generation() != generation
            || tab.busy()
            || tab.document.content.layer(above).is_none()
        {
            self.notice("Open as Layers was canceled because the image changed.", cx);
            return;
        }

        let mut accepted = Vec::new();
        let mut extra_layers = 0;
        let mut extra_bytes = 0;
        let mut stopped = None;
        for (index, pixels) in pages.into_iter().enumerate() {
            let layer = ImageLayer {
                name: format!("Page {}", first_page + index),
                pixels,
            };
            match tab
                .document
                .content
                .room_for(extra_layers, extra_bytes, &layer.pixels)
            {
                Ok(()) => {
                    extra_bytes += u64::from(layer.pixels.width())
                        .saturating_mul(u64::from(layer.pixels.height()))
                        .saturating_mul(4);
                    extra_layers += 1;
                    accepted.push(layer);
                }
                Err(stop) => {
                    stopped = Some(stop);
                    break;
                }
            }
        }
        let added = accepted.len();
        if added == 0 {
            let message = match stopped {
                Some(InsertStop::TooLarge) => {
                    "The PDF pages are too large to add to this image.".to_owned()
                }
                _ => "The document cannot hold another layer.".to_owned(),
            };
            self.notice(&message, cx);
            return;
        }
        self.edit_content(
            move |content, active, _| {
                let inserted = content.insert_centered_layers(above, accepted);
                if let Some(top) = inserted.top {
                    *active = top;
                }
                if inserted.added == 0 {
                    Dirty::None
                } else {
                    Dirty::Canvas
                }
            },
            window,
            cx,
        );
        let layers = if added == 1 { "layer" } else { "layers" };
        let mut message = format!("Added {added} {layers} from the PDF.");
        if let Some(stop) = stopped {
            message.push(' ');
            message.push_str(match stop {
                InsertStop::Full => "The document cannot hold another layer.",
                InsertStop::TooLarge => "Some pages were too large to add.",
            });
        }
        self.notice(&message, cx);
    }
}

fn document_from_layers(mut pages: Vec<image::RgbaImage>, first_page: usize) -> DocumentContent {
    let first = pages.remove(0);
    let mut content = DocumentContent::single_layer(format!("Page {first_page}"), first);
    let above = content.top_layer_id();
    let layers = pages
        .into_iter()
        .enumerate()
        .map(|(index, pixels)| ImageLayer {
            name: format!("Page {}", first_page + index + 1),
            pixels,
        })
        .collect();
    content.insert_centered_layers(above, layers);
    content
}
