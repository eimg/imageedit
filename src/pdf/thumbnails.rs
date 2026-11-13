use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::{Result, anyhow, ensure};
use hayro::{
    PixmapSettings, RenderCache, RenderSettings,
    hayro_interpret::InterpreterSettings,
    render,
    vello_cpu::{
        color::palette::css::{TRANSPARENT, WHITE},
        peniko::ImageAlphaType,
    },
};
use image::RgbaImage;

use super::{PdfBackground, PdfControl, PdfSource, load_pdf, validate_page_numbers};

pub const THUMBNAIL_EDGE: u32 = 144;
pub const THUMBNAIL_BATCH: usize = 3;

pub struct PdfThumbnail {
    pub page: usize,
    pub pixels: Result<RgbaImage, String>,
}

fn thumbnail_scales(size: (f32, f32)) -> Result<(f32, f32)> {
    ensure!(
        size.0.is_finite() && size.1.is_finite() && size.0 > 0. && size.1 > 0.,
        "This page has invalid dimensions."
    );
    let scale = f64::from(THUMBNAIL_EDGE) / f64::from(size.0.max(size.1));
    let axis = |length: f32| {
        let edge = (f64::from(length) * scale)
            .round()
            .clamp(1., f64::from(THUMBNAIL_EDGE));
        // hayro truncates f32 dimensions. A quarter pixel keeps even very thin
        // pages nonzero without rounding beyond the bounded thumbnail target.
        ((edge + 0.25) / f64::from(length)) as f32
    };
    let scales = (axis(size.0), axis(size.1));
    ensure!(
        scales.0.is_finite() && scales.1.is_finite() && scales.0 > 0. && scales.1 > 0.,
        "This page cannot be previewed at thumbnail size."
    );
    Ok(scales)
}

/// Render only one small browser batch from the retained source. Failed pages
/// keep their own error; cancellation and parser failures discard the batch.
pub fn render_thumbnails(
    source: &PdfSource,
    pages: &[usize],
    background: PdfBackground,
    control: &PdfControl,
) -> Result<Vec<PdfThumbnail>> {
    control.check()?;
    ensure!(
        pages.len() <= THUMBNAIL_BATCH,
        "Too many page previews requested."
    );
    validate_page_numbers(pages, source.info.pages)?;
    let _guard = source.acquire(control)?;
    if super::worker::enabled() {
        return super::worker::thumbnails(source, pages, background, control);
    }
    control.check()?;
    render_local(source, pages, background, control)
}

pub(super) fn render_local(
    source: &PdfSource,
    pages: &[usize],
    background: PdfBackground,
    control: &PdfControl,
) -> Result<Vec<PdfThumbnail>> {
    catch_unwind(AssertUnwindSafe(|| {
        let pdf = load_pdf(source.data.clone())?;
        control.check()?;
        ensure!(
            pdf.pages().len() == source.info.pages,
            "The PDF page information changed."
        );
        let cache = RenderCache::new();
        let interpreter = InterpreterSettings::default();
        let settings = RenderSettings::default();
        let mut thumbnails = Vec::with_capacity(pages.len());
        for &number in pages {
            control.check()?;
            let result = catch_unwind(AssertUnwindSafe(|| {
                let page = &pdf.pages()[number - 1];
                let (x_scale, y_scale) = thumbnail_scales(page.render_dimensions())?;
                let pixmap = render(
                    page,
                    &cache,
                    &interpreter,
                    &settings,
                    &PixmapSettings {
                        x_scale,
                        y_scale,
                        bg_color: match background {
                            PdfBackground::White => WHITE,
                            PdfBackground::Transparent => TRANSPARENT,
                        },
                    },
                );
                let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
                ensure!(
                    (1..=THUMBNAIL_EDGE).contains(&width) && (1..=THUMBNAIL_EDGE).contains(&height),
                    "The page preview exceeded its size limit."
                );
                RgbaImage::from_raw(width, height, pixmap.take_rgba8(ImageAlphaType::Alpha))
                    .ok_or_else(|| anyhow!("This page could not be previewed."))
            }))
            .unwrap_or_else(|_| Err(anyhow!("This page could not be previewed.")));
            control.check()?;
            thumbnails.push(PdfThumbnail {
                page: number,
                pixels: result.map_err(|error| error.to_string()),
            });
        }
        control.check()?;
        Ok(thumbnails)
    }))
    .map_err(|_| anyhow!("The PDF page previews could not be read."))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_scales_bound_large_small_and_thin_pages_without_collapsing_an_axis() {
        for size in [
            (612., 792.),
            (1., 1.),
            (100_000., 1.),
            (1., 100_000.),
            (f32::MAX, 1.),
        ] {
            let scales = thumbnail_scales(size).unwrap();
            let pixels = ((size.0 * scales.0) as u32, (size.1 * scales.1) as u32);
            assert!((1..=THUMBNAIL_EDGE).contains(&pixels.0));
            assert!((1..=THUMBNAIL_EDGE).contains(&pixels.1));
            assert_eq!(pixels.0.max(pixels.1), THUMBNAIL_EDGE);
        }
        for size in [
            (0., 1.),
            (-1., 1.),
            (1., f32::NAN),
            (f32::INFINITY, 1.),
            (f32::from_bits(1), 1.),
        ] {
            assert!(thumbnail_scales(size).is_err());
        }
    }
}
