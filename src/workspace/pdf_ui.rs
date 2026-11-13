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

pub(super) enum PendingOpen {
    Pdf { path: PathBuf, info: pdf::PdfInfo },
    Raster(Document),
}

enum PdfResult {
    Documents(Vec<Document>),
    Layers(Vec<image::RgbaImage>),
}

impl Workspace {
    pub(super) fn schedule_pending_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_scheduled
            || self.pending_opens.is_empty()
            || self.dialog_open
            || self.form.is_some()
            || self.creating
            || self.pdf_busy
            || self.active_tab().is_some_and(super::Tab::busy)
        {
            return;
        }
        self.open_scheduled = true;
        let entity = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            let _ = entity.update(cx, |this, cx| {
                this.open_scheduled = false;
                if this.dialog_open
                    || this.form.is_some()
                    || this.creating
                    || this.pdf_busy
                    || this.active_tab().is_some_and(super::Tab::busy)
                {
                    return;
                }
                if let Some(pending) = this.pending_opens.pop_front() {
                    match pending {
                        PendingOpen::Pdf { path, info } => {
                            this.pdf_import_form(path, info, PdfTarget::NewDocument, window, cx);
                        }
                        PendingOpen::Raster(document) => this.show_document(document, window, cx),
                    }
                }
            });
        });
    }

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
        let job = cx.background_executor().spawn(async move {
            let pages = pdf::render_pages(&job_options)?;
            Ok::<_, anyhow::Error>(if new_document {
                PdfResult::Documents(prepare_documents(&job_options, pages))
            } else {
                PdfResult::Layers(pages)
            })
        });
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
                    Ok(PdfResult::Documents(documents)) => {
                        for document in documents {
                            workspace.show_document(document, window, cx);
                        }
                    }
                    Ok(PdfResult::Layers(pages)) => {
                        workspace.insert_pdf_layers(target, options.first, pages, window, cx)
                    }
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

fn prepare_documents(options: &PdfOptions, pages: Vec<image::RgbaImage>) -> Vec<Document> {
    let name = document::file_name(&options.path);
    if options.mode == PdfMode::Separate {
        let count = pages.len();
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.trim().is_empty())
            .unwrap_or("PDF");
        pages
            .into_iter()
            .enumerate()
            .map(|(index, pixels)| {
                let title = if count == 1 {
                    name.clone()
                } else {
                    format!("{stem} ({})", options.first + index)
                };
                Document::from_content(
                    title,
                    DocumentContent::single_layer(
                        format!("Page {}", options.first + index),
                        pixels,
                    ),
                )
            })
            .collect()
    } else {
        vec![Document::from_content(
            name,
            document_from_layers(pages, options.first),
        )]
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn prepared_pdf_documents_keep_page_names_order_and_unsaved_project_destinations() {
        let options = PdfOptions {
            path: "art.pdf".into(),
            first: 3,
            last: 4,
            dpi: 72,
            mode: PdfMode::Separate,
        };
        let pages = || {
            vec![
                RgbaImage::from_pixel(3, 2, Rgba([255, 0, 0, 255])),
                RgbaImage::from_pixel(2, 3, Rgba([0, 0, 255, 255])),
            ]
        };
        let separate = prepare_documents(&options, pages());
        assert_eq!(separate.len(), 2);
        assert_eq!(separate[0].name, "art (3)");
        assert_eq!(separate[1].name, "art (4)");
        for doc in &separate {
            assert!(doc.path.is_none());
            assert_eq!(doc.format, document::FileFormat::Project);
            assert_eq!(doc.canvas.pixels.dimensions(), doc.content.size());
        }
        let layered = prepare_documents(
            &PdfOptions {
                mode: PdfMode::Layers,
                ..options
            },
            pages(),
        );
        assert_eq!(layered.len(), 1);
        assert_eq!(layered[0].content.size(), (3, 2));
        assert_eq!(layered[0].content.layers()[0].name, "Page 3");
        assert_eq!(layered[0].content.layers()[1].name, "Page 4");
    }
}
