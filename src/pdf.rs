use std::{
    fs::File,
    io::Read,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow, ensure};
use hayro::{
    PixmapSettings, RenderCache, RenderSettings,
    hayro_interpret::InterpreterSettings,
    hayro_syntax::{Pdf, PdfData},
    render,
    vello_cpu::{
        color::palette::css::{TRANSPARENT, WHITE},
        peniko::ImageAlphaType,
    },
};
use image::RgbaImage;

use crate::document::{Document, MAX_STACK_BYTES};

mod pages;
mod worker;
pub use worker::run_if_requested;
mod thumbnails;
pub use pages::{
    MAX_PAGE_SELECTION_CHARS, parse_page_selection, toggle_page_selection, validate_page_numbers,
};
pub use thumbnails::{THUMBNAIL_BATCH, render_thumbnails};

pub const MAX_PDF_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_PDF_PAGES: usize = 10_000;
pub const DEFAULT_DPI: u32 = 150;
pub const MIN_DPI: u32 = 1;
pub const MAX_DPI: u32 = 1200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PdfMode {
    Layers,
    Separate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PdfBackground {
    #[default]
    White,
    Transparent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PdfCanvas {
    #[default]
    FirstPage,
    FitAllPages,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PdfInfo {
    pub pages: usize,
    pub sizes: Vec<(f32, f32)>,
}

#[derive(Clone, Debug)]
pub struct PdfSource {
    data: PdfData,
    render_lock: Arc<Mutex<()>>,
    pub info: PdfInfo,
}

#[derive(Clone, Default)]
pub struct PdfControl(Arc<AtomicBool>);

#[derive(Debug)]
pub struct PdfCanceled;

impl std::fmt::Display for PdfCanceled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PDF import canceled.")
    }
}
impl std::error::Error for PdfCanceled {}

impl PdfControl {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn canceled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        if self.canceled() {
            Err(PdfCanceled.into())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PdfProgress {
    Reading,
    Inspecting,
    Rendering {
        page: usize,
        done: usize,
        total: usize,
    },
    Preparing {
        done: usize,
        total: usize,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PdfOptions {
    pub path: PathBuf,
    pub pages: Vec<usize>,
    pub dpi: u32,
    pub mode: PdfMode,
    pub background: PdfBackground,
    pub canvas: PdfCanvas,
}

pub fn layered_canvas_size(sizes: &[(u32, u32)], policy: PdfCanvas) -> Result<(u32, u32)> {
    let mut canvas = *sizes
        .first()
        .ok_or_else(|| anyhow!("Choose at least one page."))?;
    if policy == PdfCanvas::FitAllPages {
        for size in sizes {
            canvas = (canvas.0.max(size.0), canvas.1.max(size.1));
        }
    }
    Document::validate_size(canvas.0, canvas.1).context(
        "The layered canvas is too large. Use First Page, lower the DPI, or choose fewer pages",
    )?;
    Ok(canvas)
}

pub fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

pub fn read_source(
    path: &Path,
    control: &PdfControl,
    progress: impl Fn(PdfProgress),
) -> Result<PdfSource> {
    control.check()?;
    progress(PdfProgress::Reading);
    control.check()?;
    let bytes = read_pdf(path, control)?;
    let data = PdfData::from(bytes);
    progress(PdfProgress::Inspecting);
    control.check()?;
    catch_unwind(AssertUnwindSafe(|| {
        let info = if worker::enabled() {
            worker::inspect(&data, control)?
        } else {
            let pdf = load_pdf(data.clone())?;
            info_from_pdf(&pdf, control)?
        };
        control.check()?;
        Ok(PdfSource {
            data,
            info,
            render_lock: Default::default(),
        })
    }))
    .map_err(|_| anyhow!("The PDF could not be read."))?
}

impl PdfSource {
    fn acquire(&self, control: &PdfControl) -> Result<std::sync::MutexGuard<'_, ()>> {
        loop {
            control.check()?;
            match self.render_lock.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(anyhow!("The PDF renderer is unavailable."));
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
            }
        }
    }
}

pub fn render_pages(
    source: &PdfSource,
    options: &PdfOptions,
    control: &PdfControl,
    progress: impl Fn(PdfProgress),
) -> Result<Vec<RgbaImage>> {
    control.check()?;
    let _guard = source.acquire(control)?;
    control.check()?;
    catch_unwind(AssertUnwindSafe(|| {
        if worker::enabled() {
            worker::pages(source, options, control, &progress)
        } else {
            render_data(source.data.clone(), options, control, &progress)
        }
    }))
    .map_err(|_| anyhow!("The PDF could not be rendered."))?
}

pub fn scaled_page_size(size: (f32, f32), dpi: u32) -> (u32, u32) {
    // Match hayro's f32 scale and truncated integer pixmap dimensions exactly.
    let scale = dpi as f32 / 72.0;
    ((size.0 * scale) as u32, (size.1 * scale) as u32)
}

pub fn validate_options(info: &PdfInfo, options: &PdfOptions) -> Result<Vec<(u32, u32)>> {
    ensure!(!info.sizes.is_empty(), "The PDF has no pages.");
    ensure!(
        info.pages == info.sizes.len() && info.pages <= MAX_PDF_PAGES,
        "The PDF page information is invalid."
    );
    ensure!(
        (MIN_DPI..=MAX_DPI).contains(&options.dpi),
        "Enter a resolution from {MIN_DPI} to {MAX_DPI} DPI."
    );
    validate_page_numbers(&options.pages, info.pages)?;
    let mut sizes = Vec::with_capacity(options.pages.len());
    let mut total_bytes = 0u64;
    for &page in &options.pages {
        let index = page - 1;
        ensure!(
            info.sizes[index].0.is_finite()
                && info.sizes[index].1.is_finite()
                && info.sizes[index].0 > 0.
                && info.sizes[index].1 > 0.,
            "Page {} has invalid dimensions.",
            index + 1
        );
        let size = scaled_page_size(info.sizes[index], options.dpi);
        ensure!(
            (1..=u32::from(u16::MAX)).contains(&size.0)
                && (1..=u32::from(u16::MAX)).contains(&size.1),
            "Page {page} at {} DPI is outside the renderer's pixel limits. Choose another resolution.",
            options.dpi
        );
        Document::validate_size(size.0, size.1)
            .with_context(|| format!("Page {page} at {} DPI is too large", options.dpi))?;
        let bytes = u64::from(size.0)
            .saturating_mul(u64::from(size.1))
            .saturating_mul(4);
        total_bytes = total_bytes.saturating_add(bytes);
        ensure!(
            total_bytes <= MAX_STACK_BYTES,
            "The selected pages need too much memory. Use a lower resolution or fewer pages."
        );
        sizes.push(size);
    }
    if options.mode == PdfMode::Layers {
        layered_canvas_size(&sizes, options.canvas)?;
    }
    Ok(sizes)
}

fn read_pdf(path: &Path, control: &PdfControl) -> Result<Vec<u8>> {
    control.check()?;
    let path = path.canonicalize().context("The file could not be found")?;
    ensure!(path.is_file(), "Choose a PDF file");
    let metadata = std::fs::metadata(&path).context("The file could not be opened")?;
    ensure!(
        metadata.len() <= MAX_PDF_BYTES,
        "The PDF is larger than {} MB.",
        MAX_PDF_BYTES / (1024 * 1024)
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut file = File::open(&path)
        .context("The file could not be opened")?
        .take(MAX_PDF_BYTES + 1);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        control.check()?;
        let read = file.read(&mut chunk).context("The PDF could not be read")?;
        if read == 0 {
            break;
        }
        ensure!(
            bytes.len() as u64 + read as u64 <= MAX_PDF_BYTES,
            "The PDF is larger than {} MB.",
            MAX_PDF_BYTES / (1024 * 1024)
        );
        bytes.extend_from_slice(&chunk[..read]);
    }
    control.check()?;
    ensure!(
        bytes.len() as u64 <= MAX_PDF_BYTES,
        "The PDF is larger than {} MB.",
        MAX_PDF_BYTES / (1024 * 1024)
    );
    ensure!(
        bytes.windows(5).take(1024).any(|window| window == b"%PDF-"),
        "Choose a PDF file"
    );
    Ok(bytes)
}

fn load_pdf(bytes: impl Into<PdfData>) -> Result<Pdf> {
    let pdf = catch_unwind(AssertUnwindSafe(|| Pdf::new(bytes)))
        .map_err(|_| anyhow!("The PDF could not be read."))?;
    pdf.map_err(|error| {
        let detail = format!("{error:?}");
        let lower = detail.to_ascii_lowercase();
        if lower.contains("encrypt") || lower.contains("decrypt") || lower.contains("password") {
            anyhow!("Encrypted PDFs are not supported yet.")
        } else {
            anyhow!("The PDF could not be read: {detail}")
        }
    })
}

fn info_from_pdf(pdf: &Pdf, control: &PdfControl) -> Result<PdfInfo> {
    control.check()?;
    let pages = pdf.pages();
    control.check()?;
    ensure!(!pages.is_empty(), "The PDF has no pages.");
    ensure!(
        pages.len() <= MAX_PDF_PAGES,
        "The PDF has more than {MAX_PDF_PAGES} pages."
    );
    Ok(PdfInfo {
        pages: pages.len(),
        sizes: pages
            .iter()
            .map(|page| {
                control.check()?;
                Ok(page.render_dimensions())
            })
            .collect::<Result<_>>()?,
    })
}

fn render_data(
    data: PdfData,
    options: &PdfOptions,
    control: &PdfControl,
    progress: &impl Fn(PdfProgress),
) -> Result<Vec<RgbaImage>> {
    let mut pages = Vec::new();
    render_each(data, options, control, progress, |_, image| {
        pages.push(image);
        Ok(())
    })?;
    Ok(pages)
}

fn render_each(
    data: PdfData,
    options: &PdfOptions,
    control: &PdfControl,
    progress: &impl Fn(PdfProgress),
    mut emit: impl FnMut(usize, RgbaImage) -> Result<()>,
) -> Result<()> {
    control.check()?;
    progress(PdfProgress::Inspecting);
    control.check()?;
    let pdf = load_pdf(data)?;
    let info = info_from_pdf(&pdf, control)?;
    let expected = validate_options(&info, options)?;
    let pages = pdf.pages();
    let cache = RenderCache::new();
    let render_settings = RenderSettings::default();
    let interpreter_settings = InterpreterSettings::default();
    let scale = options.dpi as f32 / 72.0;
    let mut total_bytes = 0u64;

    for (index, &page_number) in options.pages.iter().enumerate() {
        let page = &pages[page_number - 1];
        control.check()?;
        progress(PdfProgress::Rendering {
            page: page_number,
            done: index,
            total: expected.len(),
        });
        control.check()?;
        let pixmap = catch_unwind(AssertUnwindSafe(|| {
            render(
                page,
                &cache,
                &interpreter_settings,
                &render_settings,
                &PixmapSettings {
                    x_scale: scale,
                    y_scale: scale,
                    bg_color: match options.background {
                        PdfBackground::White => WHITE,
                        PdfBackground::Transparent => TRANSPARENT,
                    },
                },
            )
        }))
        .map_err(|_| anyhow!("Page {page_number} could not be rendered."))?;
        control.check()?;
        let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        ensure!(
            (width, height) == expected[index],
            "Page {page_number} did not render at the requested dimensions."
        );
        Document::validate_size(width, height)
            .with_context(|| format!("Page {page_number} at {} DPI is too large", options.dpi))?;
        let bytes = pixmap.take_rgba8(ImageAlphaType::Alpha);
        let image = RgbaImage::from_raw(width, height, bytes)
            .with_context(|| format!("Page {page_number} could not be rendered."))?;
        total_bytes = total_bytes.saturating_add(u64::from(width) * u64::from(height) * 4);
        ensure!(
            total_bytes <= MAX_STACK_BYTES,
            "The selected pages need too much memory. Use a lower resolution or fewer pages."
        );
        emit(page_number, image)?;
        progress(PdfProgress::Rendering {
            page: page_number,
            done: index + 1,
            total: expected.len(),
        });
    }
    control.check()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_use_snapshot_bytes_bounded_pixels_and_the_chosen_background() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preview.pdf");
        let data = test_pdf_with(
            &[(12., 16.), (100_000., 100_000.), (1., 2000.)],
            "/ExtGState << /GS1 << /Type /ExtGState /ca 0.5 /CA 0.5 >> >>",
            |width, height| {
                format!(
                    "q /GS1 gs 1 0 0 rg 0 0 {} {} re f Q\n1 1 1 rg {} {} {} {} re f\n",
                    width / 2.,
                    height / 2.,
                    width * 0.75,
                    height * 0.75,
                    width / 4.,
                    height / 4.
                )
            },
        );
        std::fs::write(&path, data).unwrap();
        let source = read_source(&path, &PdfControl::default(), |_| {}).unwrap();
        std::fs::remove_file(&path).unwrap();
        let transparent = render_thumbnails(
            &source,
            &[1, 2, 3],
            PdfBackground::Transparent,
            &PdfControl::default(),
        )
        .unwrap();
        assert_eq!(
            transparent
                .iter()
                .map(|thumbnail| thumbnail.page)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        for thumbnail in &transparent {
            let pixels = thumbnail.pixels.as_ref().unwrap();
            assert!(pixels.width() >= 1 && pixels.height() >= 1);
            assert!(pixels.width() <= 144 && pixels.height() <= 144);
            assert_eq!(pixels.width().max(pixels.height()), 144);
        }
        let first = transparent[0].pixels.as_ref().unwrap();
        assert_eq!(first.dimensions(), (108, 144));
        assert_eq!(first.get_pixel(0, 0).0, [0; 4]);
        let red = first.get_pixel(20, 110).0;
        assert_eq!(&red[..3], &[255, 0, 0]);
        assert!(red[3].abs_diff(128) <= 1);
        assert_eq!(first.get_pixel(90, 10).0, [255; 4]);
        let white =
            render_thumbnails(&source, &[1], PdfBackground::White, &PdfControl::default()).unwrap();
        assert_eq!(
            white[0].pixels.as_ref().unwrap().get_pixel(0, 0).0,
            [255; 4]
        );
        let options = PdfOptions {
            path,
            pages: vec![2],
            dpi: 72,
            mode: PdfMode::Separate,
            background: PdfBackground::White,
            canvas: PdfCanvas::FirstPage,
        };
        assert!(validate_options(&source.info, &options).is_err());
    }

    #[test]
    fn thumbnail_batches_reject_bad_requests_and_keep_renderer_page_size_fallbacks() {
        let data = test_pdf(&[(10., 10.), (0., 10.), (10., 10.)]);
        let parsed = load_pdf(data.clone()).unwrap();
        let source = PdfSource {
            data: data.into(),
            info: info_from_pdf(&parsed, &PdfControl::default()).unwrap(),
            render_lock: Default::default(),
        };
        let thumbnails = render_thumbnails(
            &source,
            &[1, 2, 3],
            PdfBackground::White,
            &PdfControl::default(),
        )
        .unwrap();
        assert!(thumbnails[0].pixels.is_ok());
        // hayro normalizes a degenerate MediaBox to a valid fallback page.
        // Preview that reported geometry rather than inventing a different size.
        assert!(source.info.sizes[1].0 > 0. && source.info.sizes[1].1 > 0.);
        let fallback = thumbnails[1].pixels.as_ref().unwrap();
        assert_eq!(fallback.width().max(fallback.height()), 144);
        assert!(thumbnails[2].pixels.is_ok());
        for pages in [&[][..], &[0], &[4], &[1, 1], &[3, 1], &[1, 2, 3, 4]] {
            assert!(
                render_thumbnails(&source, pages, PdfBackground::White, &PdfControl::default())
                    .is_err()
            );
        }
    }

    #[test]
    fn queued_preview_and_import_cancellation_respect_shared_source_render_access() {
        let data = test_pdf(&[(10., 10.)]);
        let parsed = load_pdf(data.clone()).unwrap();
        let source = PdfSource {
            data: data.into(),
            info: info_from_pdf(&parsed, &PdfControl::default()).unwrap(),
            render_lock: Default::default(),
        };
        for thumbnail in [true, false] {
            let worker_source = source.clone();
            assert!(Arc::ptr_eq(&source.render_lock, &worker_source.render_lock));
            let guard = source.render_lock.lock().unwrap();
            let control = PdfControl::default();
            let worker_control = control.clone();
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                sender.send(()).unwrap();
                if thumbnail {
                    render_thumbnails(&worker_source, &[1], PdfBackground::White, &worker_control)
                        .map(|_| ())
                } else {
                    let options = PdfOptions {
                        path: "queued.pdf".into(),
                        pages: vec![1],
                        dpi: 72,
                        mode: PdfMode::Separate,
                        background: PdfBackground::White,
                        canvas: PdfCanvas::FirstPage,
                    };
                    render_pages(&worker_source, &options, &worker_control, |_| {}).map(|_| ())
                }
            });
            receiver.recv().unwrap();
            control.cancel();
            drop(guard);
            assert!(worker.join().unwrap().err().unwrap().is::<PdfCanceled>());
        }
    }

    #[test]
    fn noncontiguous_render_preflights_only_selected_pages_and_reports_source_numbers() {
        let source = test_pdf(&[(12., 10.), (100_000., 100_000.), (14., 14.), (20., 16.)]);
        let options = PdfOptions {
            path: "subset.pdf".into(),
            pages: parse_page_selection("4, 1, 4", 4).unwrap(),
            dpi: 72,
            mode: PdfMode::Layers,
            background: PdfBackground::White,
            canvas: PdfCanvas::FitAllPages,
        };
        let info = PdfInfo {
            pages: 4,
            sizes: vec![(12., 10.), (100_000., 100_000.), (14., 14.), (20., 16.)],
        };
        assert_eq!(
            validate_options(&info, &options).unwrap(),
            vec![(12, 10), (20, 16)]
        );
        let progress = std::cell::RefCell::new(Vec::new());
        let pages = render_data(
            source.clone().into(),
            &options,
            &PdfControl::default(),
            &|stage| progress.borrow_mut().push(stage),
        )
        .unwrap();
        assert_eq!(
            pages.iter().map(RgbaImage::dimensions).collect::<Vec<_>>(),
            vec![(12, 10), (20, 16)]
        );
        assert!(
            pages
                .iter()
                .all(|page| page.get_pixel(5, 5).0 == [255, 0, 0, 255])
        );
        let rendered = progress
            .into_inner()
            .into_iter()
            .filter_map(|stage| match stage {
                PdfProgress::Rendering { page, done, total } => Some((page, done, total)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered, vec![(1, 0, 2), (1, 1, 2), (4, 1, 2), (4, 2, 2)]);
        let selected_bad = PdfOptions {
            pages: vec![2],
            ..options.clone()
        };
        assert!(validate_options(&info, &selected_bad).is_err());
        let control = PdfControl::default();
        let canceled = render_data(source.into(), &options, &control, &|stage| {
            if matches!(
                stage,
                PdfProgress::Rendering {
                    page: 4,
                    done: 1,
                    ..
                }
            ) {
                control.cancel();
            }
        });
        assert!(canceled.err().unwrap().is::<PdfCanceled>());
    }

    #[test]
    fn transparent_and_white_backgrounds_preserve_real_pdf_alpha_and_painted_white() {
        let data = test_pdf_with(
            &[(16., 16.)],
            "/ExtGState << /GS1 << /Type /ExtGState /ca 0.5 /CA 0.5 >> >>",
            |_, _| "q /GS1 gs 1 0 0 rg 2 2 6 6 re f Q\n1 1 1 rg 10 10 4 4 re f\n".into(),
        );
        let mut options = PdfOptions {
            path: "alpha.pdf".into(),
            pages: (1..=1).collect(),
            dpi: 72,
            mode: PdfMode::Separate,
            background: PdfBackground::Transparent,
            canvas: PdfCanvas::FirstPage,
        };
        let transparent = render_data(
            data.clone().into(),
            &options,
            &PdfControl::default(),
            &|_| {},
        )
        .unwrap()
        .remove(0);
        options.background = PdfBackground::White;
        let white = render_data(data.into(), &options, &PdfControl::default(), &|_| {})
            .unwrap()
            .remove(0);
        assert_eq!(transparent.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(white.get_pixel(0, 0).0, [255; 4]);
        let red = transparent.get_pixel(4, 12).0;
        assert_eq!(&red[..3], &[255, 0, 0]);
        assert!(red[3].abs_diff(128) <= 1);
        let flattened = white.get_pixel(4, 12).0;
        assert_eq!((flattened[0], flattened[3]), (255, 255));
        assert!(flattened[1].abs_diff(127) <= 1 && flattened[2].abs_diff(127) <= 1);
        // Transparent import does not remove an actual white rectangle in the PDF.
        assert_eq!(transparent.get_pixel(12, 4).0, [255; 4]);
        assert_eq!(white.get_pixel(12, 4).0, [255; 4]);
    }

    #[test]
    fn fit_all_pages_uses_both_maximum_edges_and_rejects_an_oversized_combination() {
        assert_eq!(
            layered_canvas_size(&[(3, 8), (10, 2), (5, 5)], PdfCanvas::FitAllPages).unwrap(),
            (10, 8)
        );
        assert_eq!(
            layered_canvas_size(&[(3, 8), (10, 2)], PdfCanvas::FirstPage).unwrap(),
            (3, 8)
        );
        assert!(layered_canvas_size(&[], PdfCanvas::FitAllPages).is_err());
        let info = PdfInfo {
            pages: 2,
            sizes: vec![(16_000., 1.), (1., 16_000.)],
        };
        let mut options = PdfOptions {
            path: "mixed.pdf".into(),
            pages: (1..=2).collect(),
            dpi: 72,
            mode: PdfMode::Layers,
            background: PdfBackground::White,
            canvas: PdfCanvas::FirstPage,
        };
        assert!(validate_options(&info, &options).is_ok());
        options.canvas = PdfCanvas::FitAllPages;
        let error = validate_options(&info, &options).unwrap_err().to_string();
        assert!(error.contains("layered canvas"));
        options.mode = PdfMode::Separate;
        assert!(validate_options(&info, &options).is_ok());
        options.mode = PdfMode::Layers;
        options.pages = vec![2];
        assert_eq!(
            validate_options(&info, &options).unwrap(),
            vec![(1, 16_000)]
        );
    }

    #[test]
    fn source_snapshot_survives_replacement_and_removal_of_the_original_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source.pdf");
        std::fs::write(&path, test_pdf(&[(12., 18.), (20., 10.)])).unwrap();
        let stages = std::cell::RefCell::new(Vec::new());
        let source = read_source(&path, &PdfControl::default(), |stage| {
            stages.borrow_mut().push(stage)
        })
        .unwrap();
        assert_eq!(
            *stages.borrow(),
            [PdfProgress::Reading, PdfProgress::Inspecting]
        );
        assert_eq!(source.info.pages, 2);
        std::fs::write(&path, test_pdf(&[(50., 40.)])).unwrap();
        std::fs::remove_file(&path).unwrap();
        let options = PdfOptions {
            path,
            pages: (1..=2).collect(),
            dpi: 72,
            mode: PdfMode::Separate,
            background: PdfBackground::White,
            canvas: PdfCanvas::FirstPage,
        };
        let pages = render_pages(&source, &options, &PdfControl::default(), |_| {}).unwrap();
        assert_eq!(
            pages.iter().map(RgbaImage::dimensions).collect::<Vec<_>>(),
            [(12, 18), (20, 10)]
        );
        assert_eq!(pages[0].get_pixel(1, 1).0, [255, 0, 0, 255]);
    }

    #[test]
    fn canceled_reads_and_inspection_return_cancellation_without_a_source() {
        let control = PdfControl::default();
        control.cancel();
        assert!(
            read_source(Path::new("missing.pdf"), &control, |_| {})
                .unwrap_err()
                .is::<PdfCanceled>()
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source.pdf");
        // A valid header but malformed body proves inspection never parses it
        // after the cancellation checkpoint.
        std::fs::write(&path, b"%PDF-1.4\nmalformed").unwrap();
        for stop in [PdfProgress::Reading, PdfProgress::Inspecting] {
            let control = PdfControl::default();
            let result = read_source(&path, &control, |stage| {
                if stage == stop {
                    control.cancel();
                }
            });
            assert!(result.unwrap_err().is::<PdfCanceled>());
        }
    }

    #[test]
    fn cancellation_before_after_and_between_pages_discards_the_entire_render() {
        let source = PdfSource {
            data: test_pdf(&[(12., 18.), (20., 10.)]).into(),
            render_lock: Default::default(),
            info: PdfInfo {
                pages: 2,
                sizes: vec![(12., 18.), (20., 10.)],
            },
        };
        let options = PdfOptions {
            path: "source.pdf".into(),
            pages: (1..=2).collect(),
            dpi: 72,
            mode: PdfMode::Layers,
            background: PdfBackground::White,
            canvas: PdfCanvas::FirstPage,
        };
        for stop_done in [0, 1, 2] {
            let control = PdfControl::default();
            let progress = std::cell::RefCell::new(Vec::new());
            let result = render_pages(&source, &options, &control, |stage| {
                progress.borrow_mut().push(stage);
                if matches!(stage, PdfProgress::Rendering { done, .. } if done == stop_done) {
                    control.cancel();
                }
            });
            assert!(result.unwrap_err().is::<PdfCanceled>());
            if stop_done < 2 {
                assert!(
                    !progress
                        .borrow()
                        .iter()
                        .any(|stage| matches!(stage, PdfProgress::Rendering { page: 2, .. }))
                );
            }
        }
    }

    #[test]
    fn renderer_limits_and_invalid_probe_dimensions_are_rejected_before_rendering() {
        let options = PdfOptions {
            path: "test.pdf".into(),
            pages: (1..=1).collect(),
            dpi: 72,
            mode: PdfMode::Layers,
            background: PdfBackground::White,
            canvas: PdfCanvas::FirstPage,
        };
        for size in [
            (f32::NAN, 10.),
            (f32::INFINITY, 10.),
            (-1., 10.),
            (0.1, 10.),
            (65_536., 1.),
        ] {
            assert!(
                validate_options(
                    &PdfInfo {
                        pages: 1,
                        sizes: vec![size]
                    },
                    &options
                )
                .is_err()
            );
        }
        assert!(
            validate_options(
                &PdfInfo {
                    pages: 2,
                    sizes: vec![(10., 10.)]
                },
                &PdfOptions {
                    pages: vec![1, 2],
                    ..options.clone()
                }
            )
            .is_err()
        );
        assert!(
            validate_options(
                &PdfInfo {
                    pages: 1,
                    sizes: vec![(16_384., 1.)]
                },
                &options
            )
            .is_ok()
        );
        assert_eq!(scaled_page_size((72.5, 73.5), 72), (72, 73));
        let pages = render_data(
            test_pdf(&[(72.5, 73.5)]).into(),
            &options,
            &PdfControl::default(),
            &|_| {},
        )
        .unwrap();
        assert_eq!(pages[0].dimensions(), (72, 73));
    }

    #[test]
    fn representative_pdf_fixtures_cover_geometry_fonts_alpha_images_and_rejections() {
        let data = include_bytes!("../fixtures/pdf/geometry-fonts-transparency-images.pdf");
        let pdf = load_pdf(data.to_vec()).unwrap();
        let info = info_from_pdf(&pdf, &PdfControl::default()).unwrap();
        assert_eq!(info.sizes, vec![(60., 80.), (100., 100.)]);
        let source = PdfSource {
            data: data.to_vec().into(),
            info,
            render_lock: Default::default(),
        };
        let options = PdfOptions {
            path: PathBuf::from("fixture.pdf"),
            pages: vec![1, 2],
            dpi: 72,
            mode: PdfMode::Layers,
            background: PdfBackground::Transparent,
            canvas: PdfCanvas::FitAllPages,
        };
        let pages = render_pages(&source, &options, &PdfControl::default(), |_| {}).unwrap();
        assert_eq!(pages[0].dimensions(), (60, 80));
        assert_eq!(pages[1].dimensions(), (100, 100));
        assert!(pages[0].pixels().any(|pixel| pixel[3] == 0));
        assert!(
            pages[0]
                .pixels()
                .any(|pixel| pixel[3] > 0 && pixel[0] > 200 && pixel[1] < 20)
        );
        assert!(
            pages[1]
                .pixels()
                .any(|pixel| pixel[3] > 0 && pixel[3] < 255)
        );
        assert!(
            pages[1]
                .pixels()
                .any(|pixel| pixel[0] > 200 && pixel[1] < 20 && pixel[3] == 255)
        );
        for malformed in [
            include_bytes!("../fixtures/pdf/malformed-truncated.pdf").as_slice(),
            include_bytes!("../fixtures/pdf/encrypted-rejected.pdf").as_slice(),
        ] {
            assert!(
                load_pdf(malformed.to_vec())
                    .and_then(|pdf| info_from_pdf(&pdf, &PdfControl::default()))
                    .is_err()
            );
        }
    }

    fn add_object(body: &mut String, offsets: &mut Vec<usize>, object: String) {
        offsets.push(body.len());
        body.push_str(&object);
        body.push('\n');
    }

    fn test_pdf(pages: &[(f32, f32)]) -> Vec<u8> {
        test_pdf_with(pages, "", |width, height| {
            format!("1 0 0 rg\n0 0 {width} {height} re\nf\n")
        })
    }

    fn test_pdf_with(
        pages: &[(f32, f32)],
        resources: &str,
        content: impl Fn(f32, f32) -> String,
    ) -> Vec<u8> {
        let mut body = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        let page_ids: Vec<usize> = (0..pages.len()).map(|index| 3 + 2 * index).collect();
        let kids = page_ids
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        add_object(
            &mut body,
            &mut offsets,
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".into(),
        );
        add_object(
            &mut body,
            &mut offsets,
            format!(
                "2 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {} >>\nendobj\n",
                pages.len()
            ),
        );
        for (index, (width, height)) in pages.iter().enumerate() {
            let page_id = 3 + 2 * index;
            let content_id = 4 + 2 * index;
            add_object(
                &mut body,
                &mut offsets,
                format!(
                    "{page_id} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << {resources} >> /Contents {content_id} 0 R >>\nendobj\n"
                ),
            );
            let content = content(*width, *height);
            add_object(
                &mut body,
                &mut offsets,
                format!(
                    "{content_id} 0 obj\n<< /Length {} >>\nstream\n{content}endstream\nendobj\n",
                    content.len()
                ),
            );
        }
        let xref_at = body.len();
        let count = offsets.len() + 1;
        body.push_str(&format!("xref\n0 {count}\n0000000000 65535 f \n"));
        for offset in &offsets {
            body.push_str(&format!("{offset:010} 00000 n \n"));
        }
        body.push_str(&format!(
            "trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
        ));
        body.into_bytes()
    }

    #[test]
    fn probe_reports_page_count_and_page_sizes() {
        let info = info_from_pdf(
            &load_pdf(test_pdf(&[(100., 200.), (50., 25.)])).unwrap(),
            &PdfControl::default(),
        )
        .unwrap();
        assert_eq!(info.pages, 2);
        assert!((info.sizes[0].0 - 100.).abs() < 0.01);
        assert!((info.sizes[0].1 - 200.).abs() < 0.01);
        assert!((info.sizes[1].0 - 50.).abs() < 0.01);
        assert!((info.sizes[1].1 - 25.).abs() < 0.01);
    }

    #[test]
    fn rendering_scales_pages_and_honors_the_page_range() {
        let options = PdfOptions {
            path: PathBuf::from("test.pdf"),
            pages: (2..=2).collect(),
            dpi: 144,
            mode: PdfMode::Separate,
            background: PdfBackground::White,
            canvas: PdfCanvas::FirstPage,
        };
        let pages = render_data(
            test_pdf(&[(72., 72.), (72., 144.)]).into(),
            &options,
            &PdfControl::default(),
            &|_| {},
        )
        .unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].dimensions(), (144, 288));
        assert_eq!(pages[0].get_pixel(10, 10).0, [255, 0, 0, 255]);
    }

    #[test]
    fn invalid_ranges_and_resolutions_are_rejected() {
        let info = PdfInfo {
            pages: 2,
            sizes: vec![(72., 72.), (72., 72.)],
        };
        let options = |first, last, dpi| PdfOptions {
            path: PathBuf::from("test.pdf"),
            pages: (first..=last).collect(),
            dpi,
            mode: PdfMode::Layers,
            background: PdfBackground::White,
            canvas: PdfCanvas::FirstPage,
        };
        assert!(validate_options(&info, &options(1, 2, 150)).is_ok());
        assert!(validate_options(&info, &options(0, 1, 150)).is_err());
        assert!(validate_options(&info, &options(2, 1, 150)).is_err());
        assert!(validate_options(&info, &options(1, 3, 150)).is_err());
        assert!(validate_options(&info, &options(1, 1, 0)).is_err());
    }

    #[test]
    fn oversized_pages_are_rejected_before_rendering() {
        let info = PdfInfo {
            pages: 1,
            sizes: vec![(100_000., 100_000.)],
        };
        let options = PdfOptions {
            path: PathBuf::from("test.pdf"),
            pages: (1..=1).collect(),
            dpi: 72,
            mode: PdfMode::Layers,
            background: PdfBackground::White,
            canvas: PdfCanvas::FirstPage,
        };
        assert!(validate_options(&info, &options).is_err());
    }

    #[test]
    fn malformed_and_non_pdf_bytes_are_rejected() {
        assert!(load_pdf(b"not a pdf".to_vec()).is_err());
        assert!(load_pdf(test_pdf(&[(10., 10.)])).is_ok());
        assert!(is_pdf_path(Path::new("page.PDF")));
        assert!(!is_pdf_path(Path::new("page.png")));
    }
}
