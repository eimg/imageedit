use std::sync::Arc;

use gpui::{
    Context, CursorStyle, Div, ObjectFit, RenderImage, Window, div, img, prelude::*, px, rgb,
};

use super::{
    Workspace,
    dialogs::{form_button, form_navigation_button},
    tooltip::tooltip,
    view::{ACCENT, BORDER, CANVAS, MUTED},
};
use crate::{
    document,
    pdf::{self, PdfBackground, PdfControl, PdfSource},
};

#[derive(Default)]
struct PreviewJobs {
    generation: u64,
    running: Option<(u64, PdfControl)>,
}

impl PreviewJobs {
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some((_, control)) = &self.running {
            control.cancel();
        }
    }
    fn begin(&mut self) -> Option<(u64, PdfControl)> {
        if self.running.is_some() {
            return None;
        }
        let control = PdfControl::default();
        self.running = Some((self.generation, control.clone()));
        Some((self.generation, control))
    }
    fn finish(&mut self, token: u64) -> bool {
        if self
            .running
            .as_ref()
            .is_none_or(|(running, _)| *running != token)
        {
            return false;
        }
        self.running = None;
        self.generation == token
    }
}

impl Drop for PreviewJobs {
    fn drop(&mut self) {
        if let Some((_, control)) = &self.running {
            control.cancel();
        }
    }
}

struct PreviewEntry {
    page: usize,
    image: Result<(Arc<RenderImage>, (u32, u32)), String>,
}

struct PreviewSession {
    source: Arc<PdfSource>,
    background: PdfBackground,
    start: usize,
    dirty: bool,
    entries: Vec<PreviewEntry>,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct PdfPreviews {
    jobs: PreviewJobs,
    session: Option<PreviewSession>,
}

fn batch_start(page: usize, count: usize) -> Option<usize> {
    ((1..=pdf::MAX_PDF_PAGES).contains(&count) && (1..=count).contains(&page))
        .then(|| ((page - 1) / pdf::THUMBNAIL_BATCH) * pdf::THUMBNAIL_BATCH + 1)
}

fn entry_images(entries: Vec<PreviewEntry>) -> impl Iterator<Item = Arc<RenderImage>> {
    entries
        .into_iter()
        .filter_map(|entry| entry.image.ok().map(|(image, _)| image))
}

impl Workspace {
    pub(super) fn clear_pdf_previews(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pdf_previews.jobs.invalidate();
        if let Some(session) = self.pdf_previews.session.take() {
            super::drop_images(entry_images(session.entries), window, cx);
        }
    }

    pub(super) fn init_pdf_previews(
        &mut self,
        source: Arc<PdfSource>,
        background: PdfBackground,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_pdf_previews(window, cx);
        self.pdf_previews.session = Some(PreviewSession {
            source,
            background,
            start: 1,
            dirty: true,
            entries: Vec::new(),
            error: None,
        });
        self.pump_pdf_previews(window, cx);
    }

    pub(super) fn set_pdf_preview_background(
        &mut self,
        background: PdfBackground,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.pdf_previews.session.as_mut() else {
            return;
        };
        if session.background == background {
            return;
        }
        session.background = background;
        let start = session.start;
        self.request_pdf_previews(start, true, window, cx);
    }

    fn request_pdf_previews(
        &mut self,
        page: usize,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.pdf_previews.session.as_mut() else {
            return;
        };
        let Some(start) = batch_start(page, session.source.info.pages) else {
            return;
        };
        if !force && session.start == start {
            return;
        }
        session.start = start;
        session.dirty = true;
        session.error = None;
        let entries = std::mem::take(&mut session.entries);
        self.pdf_previews.jobs.invalidate();
        super::drop_images(entry_images(entries), window, cx);
        self.pump_pdf_previews(window, cx);
        cx.notify();
    }

    fn pump_pdf_previews(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self
            .pdf_previews
            .session
            .as_mut()
            .filter(|session| session.dirty)
        else {
            return;
        };
        let Some((token, control)) = self.pdf_previews.jobs.begin() else {
            return;
        };
        session.dirty = false;
        let source = session.source.clone();
        let background = session.background;
        let end = (session.start + pdf::THUMBNAIL_BATCH - 1).min(source.info.pages);
        let pages: Vec<_> = (session.start..=end).collect();
        let job = cx.background_executor().spawn(async move {
            let thumbnails = pdf::render_thumbnails(&source, &pages, background, &control)?;
            thumbnails
                .into_iter()
                .map(|thumbnail| {
                    control.check()?;
                    let image = thumbnail.pixels.map(|pixels| {
                        let size = pixels.dimensions();
                        (document::preview_render(&pixels), size)
                    });
                    Ok(PreviewEntry {
                        page: thumbnail.page,
                        image,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                let current = workspace.pdf_previews.jobs.finish(token);
                if current && let Some(session) = workspace.pdf_previews.session.as_mut() {
                    match result {
                        Ok(entries) => session.entries = entries,
                        Err(error) => session.error = Some(error.to_string()),
                    }
                } else if let Ok(entries) = result {
                    super::drop_images(entry_images(entries), window, cx);
                }
                workspace.pump_pdf_previews(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn pdf_preview_browser(
        &self,
        selected: Option<&[usize]>,
        all_pages: bool,
        cx: &Context<Self>,
    ) -> Div {
        let Some(session) = &self.pdf_previews.session else {
            return div();
        };
        let first = selected
            .and_then(|pages| pages.first())
            .copied()
            .unwrap_or(1);
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_sm().child("Select pages"));
        let count = session.source.info.pages;
        let start = session.start;
        let end = (start + pdf::THUMBNAIL_BATCH - 1).min(count);
        let previous = start.saturating_sub(pdf::THUMBNAIL_BATCH).max(1);
        let next = start + pdf::THUMBNAIL_BATCH;
        let loading = session.dirty || self.pdf_previews.jobs.running.is_some();
        let failures =
            session.error.is_some() || session.entries.iter().any(|entry| entry.image.is_err());
        panel = panel
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(format!("Pages {start}–{end} of {count}")),
            )
            .child(div().flex().gap_2().children((start..=end).map(|page| {
                let entry = session.entries.iter().find(|entry| entry.page == page);
                let included =
                    all_pages || selected.is_some_and(|pages| pages.binary_search(&page).is_ok());
                let content = match entry.map(|entry| &entry.image) {
                    Some(Ok((image, (width, height)))) => {
                        let factor = (104. / *width as f32).min(150. / *height as f32);
                        div()
                            .relative()
                            .w(px(*width as f32 * factor))
                            .h(px(*height as f32 * factor))
                            .overflow_hidden()
                            .child(super::layers::thumbnail_checker())
                            .child(
                                img(image.clone())
                                    .absolute()
                                    .size_full()
                                    .object_fit(ObjectFit::Fill),
                            )
                    }
                    Some(Err(error)) => div().child(
                        div()
                            .id(("pdf-page-error", page))
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child("Unavailable")
                            .tooltip(tooltip(error.clone())),
                    ),
                    None => div().text_xs().text_color(rgb(MUTED)).child(if loading {
                        "Loading…"
                    } else {
                        "Unavailable"
                    }),
                };
                div()
                    .id(("pdf-page-choice", page))
                    .cursor_pointer()
                    .tooltip(tooltip(format!(
                        "{} page {page}",
                        if included { "Deselect" } else { "Select" }
                    )))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_pdf_page(page, cx)))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .h(px(156.))
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(CANVAS))
                            .border_1()
                            .border_color(rgb(if included { ACCENT } else { BORDER }))
                            .rounded_md()
                            .child(content),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(if included { ACCENT } else { MUTED }))
                            .child(if included {
                                format!("✓ Page {page}")
                            } else {
                                format!("Page {page}")
                            }),
                    )
            })))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        form_navigation_button("Previous", false)
                            .flex_1()
                            .when(start > 1, |button| {
                                button.on_click(cx.listener(move |this, _, window, cx| {
                                    this.request_pdf_previews(previous, false, window, cx)
                                }))
                            })
                            .when(start == 1, |button| {
                                button.opacity(0.45).cursor(CursorStyle::Arrow)
                            }),
                    )
                    .child(
                        form_button("Selected", false)
                            .flex_1()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.request_pdf_previews(first, false, window, cx)
                            })),
                    )
                    .child(
                        form_navigation_button("Next", true)
                            .flex_1()
                            .when(next <= count, |button| {
                                button.on_click(cx.listener(move |this, _, window, cx| {
                                    this.request_pdf_previews(next, false, window, cx)
                                }))
                            })
                            .when(next > count, |button| {
                                button.opacity(0.45).cursor(CursorStyle::Arrow)
                            }),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("Click a page to select or deselect it."),
            )
            .when_some(session.error.as_ref(), |panel, error| {
                panel.child(div().text_xs().text_color(rgb(MUTED)).child(error.clone()))
            })
            .when(failures, |panel| {
                panel.child(form_button("Retry previews", false).on_click(cx.listener(
                    move |this, _, window, cx| this.request_pdf_previews(start, true, window, cx),
                )))
            });
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_requests_coalesce_reject_stale_results_and_keep_one_worker_until_completion() {
        let mut jobs = PreviewJobs::default();
        jobs.invalidate();
        let (old, control) = jobs.begin().unwrap();
        assert!(jobs.begin().is_none());
        jobs.invalidate();
        jobs.invalidate();
        assert!(control.canceled());
        assert!(jobs.begin().is_none());
        assert!(!jobs.finish(old));
        let (latest, latest_control) = jobs.begin().unwrap();
        assert!(!jobs.finish(old));
        assert!(jobs.begin().is_none());
        assert!(!latest_control.canceled());
        assert!(jobs.finish(latest));
        let (_, control) = jobs.begin().unwrap();
        drop(jobs);
        assert!(control.canceled());
    }

    #[test]
    fn preview_batches_bound_navigation_and_selected_page_jumps() {
        for (page, total, expected) in [
            (1, 1, 1),
            (3, 10, 1),
            (4, 10, 4),
            (9, 10, 7),
            (10, 10, 10),
            (10_000, 10_000, 10_000),
        ] {
            let start = batch_start(page, total).unwrap();
            assert_eq!(start, expected);
            let pages: Vec<_> = (start..=(start + pdf::THUMBNAIL_BATCH - 1).min(total)).collect();
            assert!(pages.contains(&page));
            assert!(pages.len() <= pdf::THUMBNAIL_BATCH);
        }
        for (page, total) in [(0, 1), (2, 1), (1, 0), (1, usize::MAX)] {
            assert_eq!(batch_start(page, total), None);
        }
    }
}
