use std::{
    fs::File,
    io::Read,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, anyhow, ensure};
use hayro::{
    PixmapSettings, RenderCache, RenderSettings,
    hayro_interpret::InterpreterSettings,
    hayro_syntax::Pdf,
    render,
    vello_cpu::{color::palette::css::WHITE, peniko::ImageAlphaType},
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

#[derive(Clone, Debug)]
pub struct PdfInfo {
    pub pages: usize,
    pub sizes: Vec<(f32, f32)>,
}

#[derive(Clone, Debug)]
pub struct PdfOptions {
    pub path: PathBuf,
    pub first: usize,
    pub last: usize,
    pub dpi: u32,
    pub mode: PdfMode,
}

pub fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

pub fn read_info(path: &Path) -> Result<PdfInfo> {
    let bytes = read_pdf(path)?;
    let pdf = load_pdf(bytes)?;
    info_from_pdf(&pdf)
}

pub fn render_pages(options: &PdfOptions) -> Result<Vec<RgbaImage>> {
    let bytes = read_pdf(&options.path)?;
    render_bytes(bytes, options)
}

pub fn scaled_page_size(size: (f32, f32), dpi: u32) -> (u32, u32) {
    let scale = f64::from(dpi) / 72.0;
    let width = (f64::from(size.0) * scale).ceil().max(1.0);
    let height = (f64::from(size.1) * scale).ceil().max(1.0);
    (
        width.min(f64::from(u32::MAX)) as u32,
        height.min(f64::from(u32::MAX)) as u32,
    )
}

pub fn validate_options(info: &PdfInfo, options: &PdfOptions) -> Result<Vec<(u32, u32)>> {
    ensure!(!info.sizes.is_empty(), "The PDF has no pages.");
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
        let size = scaled_page_size(info.sizes[index], options.dpi);
        let page = index + 1;
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
    Ok(sizes)
}

fn read_pdf(path: &Path) -> Result<Vec<u8>> {
    let path = path.canonicalize().context("The file could not be found")?;
    ensure!(path.is_file(), "Choose a PDF file");
    let metadata = std::fs::metadata(&path).context("The file could not be opened")?;
    ensure!(
        metadata.len() <= MAX_PDF_BYTES,
        "The PDF is larger than {} MB.",
        MAX_PDF_BYTES / (1024 * 1024)
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(&path)
        .context("The file could not be opened")?
        .take(MAX_PDF_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("The PDF could not be read")?;
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

fn load_pdf(bytes: Vec<u8>) -> Result<Pdf> {
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

fn info_from_pdf(pdf: &Pdf) -> Result<PdfInfo> {
    let pages = pdf.pages();
    ensure!(!pages.is_empty(), "The PDF has no pages.");
    ensure!(
        pages.len() <= MAX_PDF_PAGES,
        "The PDF has more than {MAX_PDF_PAGES} pages."
    );
    Ok(PdfInfo {
        pages: pages.len(),
        sizes: pages.iter().map(|page| page.render_dimensions()).collect(),
    })
}

fn render_bytes(bytes: Vec<u8>, options: &PdfOptions) -> Result<Vec<RgbaImage>> {
    let pdf = load_pdf(bytes)?;
    let info = info_from_pdf(&pdf)?;
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
        let pixmap = catch_unwind(AssertUnwindSafe(|| {
            render(
                page,
                &cache,
                &interpreter_settings,
                &render_settings,
                &PixmapSettings {
                    x_scale: scale,
                    y_scale: scale,
                    bg_color: WHITE,
                },
            )
        }))
        .map_err(|_| anyhow!("Page {page_number} could not be rendered."))?;
        let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
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
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_object(body: &mut String, offsets: &mut Vec<usize>, object: String) {
        offsets.push(body.len());
        body.push_str(&object);
        body.push('\n');
    }

    fn test_pdf(pages: &[(f32, f32)]) -> Vec<u8> {
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
                    "{page_id} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << >> /Contents {content_id} 0 R >>\nendobj\n"
                ),
            );
            let content = format!("1 0 0 rg\n0 0 {width} {height} re\nf\n");
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
        let info =
            info_from_pdf(&load_pdf(test_pdf(&[(100., 200.), (50., 25.)])).unwrap()).unwrap();
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
        };
        let pages = render_bytes(test_pdf(&[(72., 72.), (72., 144.)]), &options).unwrap();
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
