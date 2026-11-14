//! Single-page raster PDF export. DPI changes physical size, never pixel count.

use std::io::Write;

use anyhow::{Result, ensure};
use flate2::{Compression, write::ZlibEncoder};
use image::RgbaImage;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};

pub const DEFAULT_DPI: u32 = 300;
pub const MIN_DPI: u32 = 1;
pub const MAX_DPI: u32 = 1200;

pub fn page_size(size: (u32, u32), dpi: u32) -> Result<(f32, f32)> {
    ensure!(
        (MIN_DPI..=MAX_DPI).contains(&dpi),
        "Enter a resolution from {MIN_DPI} to {MAX_DPI} DPI."
    );
    ensure!(size.0 > 0 && size.1 > 0, "The canvas is empty.");
    // Keep within the standard 200-inch page limit without viewer-specific scaling.
    let minimum = size.0.max(size.1).div_ceil(200);
    ensure!(
        dpi >= minimum,
        "This canvas needs at least {minimum} DPI to fit within the PDF page size limit."
    );
    Ok((
        size.0 as f32 * 72. / dpi as f32,
        size.1 as f32 * 72. / dpi as f32,
    ))
}

pub fn write(mut writer: impl Write, pixels: &RgbaImage, dpi: u32) -> Result<()> {
    let (width, height) = page_size(pixels.dimensions(), dpi)?;
    // Compress one RGB row at a time instead of allocating another full raster.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let mut row = Vec::with_capacity(pixels.width() as usize * 3);
    for pixels in pixels.rows() {
        row.clear();
        for pixel in pixels {
            row.extend_from_slice(&crate::document::rgb_over_white(*pixel));
        }
        encoder.write_all(&row)?;
    }
    let encoded = encoder.finish()?;
    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let pages = Ref::new(2);
    let page = Ref::new(3);
    let image = Ref::new(4);
    let contents = Ref::new(5);
    let profile = Ref::new(6);
    let image_name = Name(b"Canvas");
    pdf.catalog(catalog).pages(pages);
    pdf.pages(pages).kids([page]).count(1);
    let mut page_writer = pdf.page(page);
    page_writer.parent(pages);
    page_writer.media_box(Rect::new(0., 0., width, height));
    page_writer.contents(contents);
    page_writer.resources().x_objects().pair(image_name, image);
    page_writer.finish();

    let mut image_writer = pdf.image_xobject(image, &encoded);
    image_writer.width(pixels.width().try_into()?);
    image_writer.height(pixels.height().try_into()?);
    image_writer.bits_per_component(8);
    image_writer.color_space().icc_based(profile);
    image_writer.filter(Filter::FlateDecode);
    image_writer.interpolate(false);
    image_writer.finish();
    let srgb = crate::color_management::srgb_profile()?;
    let mut icc = pdf.icc_profile(profile, &srgb);
    icc.n(3);
    icc.alternate().device_rgb();
    icc.finish();

    let mut content = Content::new();
    content.save_state();
    content.transform([width, 0., 0., height, 0., 0.]);
    content.x_object(image_name);
    content.restore_state();
    pdf.stream(contents, &content.finish());
    writer.write_all(&pdf.finish())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{PdfBackground, PdfCanvas, PdfControl, PdfMode, PdfOptions};
    use hayro::hayro_syntax::object::{Array, Name as PdfName, Object};

    #[test]
    fn resolution_sets_physical_size_and_rejects_invalid_or_oversized_pages() {
        assert_eq!(page_size((1200, 600), 300).unwrap(), (288., 144.));
        assert_eq!(page_size((1200, 600), 150).unwrap(), (576., 288.));
        for dpi in [0, 1201, u32::MAX] {
            assert!(page_size((1, 1), dpi).is_err());
        }
        assert!(page_size((0, 1), 300).is_err());
        assert!(page_size((16_384, 1), 81).is_err());
        assert!(page_size((16_384, 1), 82).is_ok());
        assert_eq!(page_size((200, 200), 1).unwrap(), (14_400., 14_400.));
    }

    #[test]
    fn exported_pdf_has_one_page_and_lossless_upright_pixels_over_white() {
        let pixels = RgbaImage::from_fn(60, 40, |x, y| {
            image::Rgba(match (x, y) {
                (0, 0) => [255, 0, 0, 255],
                (59, 0) => [0, 255, 0, 255],
                (0, 39) => [0, 0, 255, 255],
                (59, 39) => [0, 0, 0, 255],
                (2, 1) => [53, 171, 229, 128],
                _ => [19, 70, 140, 0],
            })
        });
        for dpi in [72, 300, 1200] {
            let mut bytes = Vec::new();
            write(&mut bytes, &pixels, dpi).unwrap();
            let pdf = super::super::load_pdf(bytes.clone()).unwrap();
            let info = super::super::info_from_pdf(&pdf, &PdfControl::default()).unwrap();
            assert_eq!(info.pages, 1);
            assert_eq!(info.sizes[0], page_size(pixels.dimensions(), dpi).unwrap());
            let images: Vec<_> = pdf
                .objects()
                .into_iter()
                .filter_map(|object| match object {
                    Object::Stream(stream)
                        if stream
                            .dict()
                            .get::<PdfName<'_>>(b"Subtype")
                            .is_some_and(|name| name.as_ref() == b"Image") =>
                    {
                        Some(stream)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].dict().get::<i32>(b"Width"), Some(60));
            assert_eq!(images[0].dict().get::<i32>(b"Height"), Some(40));
            let expected: Vec<u8> = pixels
                .pixels()
                .flat_map(|pixel| crate::document::rgb_over_white(*pixel))
                .collect();
            assert_eq!(images[0].decoded_data().unwrap().as_ref(), expected);
            let color_space = images[0].dict().get::<Array<'_>>(b"ColorSpace").unwrap();
            let mut entries = color_space.iter::<Object<'_>>();
            assert!(
                matches!(entries.next(), Some(Object::Name(name)) if name.as_ref() == b"ICCBased")
            );
            let Some(Object::Stream(profile)) = entries.next() else {
                panic!("missing embedded color profile");
            };
            assert_eq!(profile.dict().get::<i32>(b"N"), Some(3));
            assert_eq!(
                profile.decoded_data().unwrap().as_ref(),
                crate::color_management::srgb_profile().unwrap()
            );
            // Render at 72 DPI for pixel-exact verification (300 DPI page geometry
            // is checked above; tiny page rounding is renderer-dependent).
            if dpi != 72 {
                continue;
            }
            let options = PdfOptions {
                path: "export.pdf".into(),
                pages: vec![1],
                dpi,
                mode: PdfMode::Separate,
                background: PdfBackground::Transparent,
                canvas: PdfCanvas::FirstPage,
            };
            let rendered =
                super::super::render_data(bytes.into(), &options, &PdfControl::default(), &|_| {})
                    .unwrap()
                    .remove(0);
            assert_eq!(rendered.dimensions(), pixels.dimensions());
            for (actual, original) in rendered.pixels().zip(pixels.pixels()) {
                let expected = crate::document::rgb_over_white(*original);
                assert_eq!(actual[3], 255);
                for (actual, expected) in actual.0.into_iter().zip(expected) {
                    assert!(actual.abs_diff(expected) <= 1, "{actual} != {expected}");
                }
            }
        }
    }
}
