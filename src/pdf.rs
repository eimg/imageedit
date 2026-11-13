use std::{
    fs::File,
    io::Read,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc,
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

use crate::document::{Document, MAX_LAYERS, MAX_STACK_BYTES};

pub const MAX_PDF_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_PDF_PAGES: usize = 10_000;
pub const DEFAULT_DPI: u32 = 150;
pub const MIN_DPI: u32 = 1;
pub const MAX_DPI: u32 = 1200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfMode {
    Layers,
    Separate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdfBackground {
    #[default]
    White,
    Transparent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdfCanvas {
    #[default]
    FirstPage,
    FitAllPages,
}

#[derive(Clone, Debug)]
pub struct PdfInfo {
    pub pages: usize,
    pub sizes: Vec<(f32, f32)>,
}

#[derive(Clone, Debug)]
pub struct PdfSource {
    data: PdfData,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug)]
pub struct PdfOptions {
    pub path: PathBuf,
    pub first: usize,
    pub last: usize,
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
        let pdf = load_pdf(data.clone())?;
        let info = info_from_pdf(&pdf, control)?;
        control.check()?;
        Ok(PdfSource { data, info })
    }))
    .map_err(|_| anyhow!("The PDF could not be read."))?
}

pub fn render_pages(
    source: &PdfSource,
    options: &PdfOptions,
    control: &PdfControl,
    progress: impl Fn(PdfProgress),
) -> Result<Vec<RgbaImage>> {
    control.check()?;
    catch_unwind(AssertUnwindSafe(|| {
        render_data(source.data.clone(), options, control, &progress)
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
    ensure!(
        options.first >= 1 && options.last >= options.first && options.last <= info.pages,
        "Choose pages between 1 and {}.",
        info.pages
    );
    let count = options.last - options.first + 1;
    ensure!(
        count <= MAX_LAYERS,
        "Choose at most {MAX_LAYERS} pages at a time."
    );

    let mut sizes = Vec::with_capacity(count);
    let mut total_bytes = 0u64;
    for index in options.first - 1..options.last {
        ensure!(
            info.sizes[index].0.is_finite()
                && info.sizes[index].1.is_finite()
                && info.sizes[index].0 > 0.
                && info.sizes[index].1 > 0.,
            "Page {} has invalid dimensions.",
            index + 1
        );
        let size = scaled_page_size(info.sizes[index], options.dpi);
        let page = index + 1;
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
    let mut rendered = Vec::with_capacity(expected.len());
    let mut total_bytes = 0u64;

    for (index, page) in pages[options.first - 1..options.last].iter().enumerate() {
        let page_number = options.first + index;
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
        rendered.push(image);
        progress(PdfProgress::Rendering {
            page: page_number,
            done: index + 1,
            total: expected.len(),
        });
    }
    control.check()?;
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_and_white_backgrounds_preserve_real_pdf_alpha_and_painted_white() {
        let data = test_pdf_with(
            &[(16., 16.)],
            "/ExtGState << /GS1 << /Type /ExtGState /ca 0.5 /CA 0.5 >> >>",
            |_, _| "q /GS1 gs 1 0 0 rg 2 2 6 6 re f Q\n1 1 1 rg 10 10 4 4 re f\n".into(),
        );
        let mut options = PdfOptions {
            path: "alpha.pdf".into(),
            first: 1,
            last: 1,
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
            first: 1,
            last: 2,
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
        options.first = 2;
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
            first: 1,
            last: 2,
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
            info: PdfInfo {
                pages: 2,
                sizes: vec![(12., 18.), (20., 10.)],
            },
        };
        let options = PdfOptions {
            path: "source.pdf".into(),
            first: 1,
            last: 2,
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
            first: 1,
            last: 1,
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
                    last: 2,
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
            first: 2,
            last: 2,
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
            first,
            last,
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
            first: 1,
            last: 1,
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
