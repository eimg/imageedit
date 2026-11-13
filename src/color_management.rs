//! Raster imports are normalized to the editor's sRGB working space. Export
//! labels those pixels with sRGB; a source profile is never attached to edited
//! pixels that no longer use its coordinates.
use anyhow::{Context, Result, ensure};
use image::RgbaImage;
use moxcms::{ColorProfile, DataColorSpace, Layout};
const MAX_ICC_BYTES: usize = 4 * 1024 * 1024;

pub fn to_srgb(pixels: RgbaImage, profile: &[u8]) -> Result<RgbaImage> {
    ensure!(
        pixels.width() > 0 && pixels.height() > 0,
        "The image is empty"
    );
    ensure!(
        profile.len() <= MAX_ICC_BYTES,
        "The embedded color profile is too large"
    );
    let source =
        ColorProfile::new_from_slice(profile).context("The embedded color profile is invalid")?;
    let destination = ColorProfile::new_srgb();
    let mut output = RgbaImage::new(pixels.width(), pixels.height());
    match source.color_space {
        DataColorSpace::Rgb => {
            let transform = source
                .create_transform_8bit(Layout::Rgba, &destination, Layout::Rgba, Default::default())
                .context("The embedded RGB color profile is not supported")?;
            // Row-sized conversions keep scratch memory independent of image height.
            for (input, output) in pixels.as_raw().chunks(pixels.width() as usize * 4).zip(
                output
                    .as_flat_samples_mut()
                    .samples
                    .chunks_mut(pixels.width() as usize * 4),
            ) {
                transform
                    .transform(input, output)
                    .context("Color conversion failed")?;
            }
        }
        DataColorSpace::Gray => {
            let transform = source
                .create_transform_8bit(
                    Layout::GrayAlpha,
                    &destination,
                    Layout::Rgba,
                    Default::default(),
                )
                .context("The embedded grayscale profile is not supported")?;
            let mut row = vec![0u8; pixels.width() as usize * 2];
            for (input, output) in pixels.as_raw().chunks(pixels.width() as usize * 4).zip(
                output
                    .as_flat_samples_mut()
                    .samples
                    .chunks_mut(pixels.width() as usize * 4),
            ) {
                for (rgba, gray) in input
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(row.as_chunks_mut::<2>().0.iter_mut())
                {
                    gray.copy_from_slice(&[rgba[0], rgba[3]]);
                }
                transform
                    .transform(&row, output)
                    .context("Color conversion failed")?;
            }
        }
        _ => anyhow::bail!(
            "The embedded color profile is not RGB or grayscale. Convert this image to sRGB before opening it."
        ),
    }
    // Preserve alpha while still converting the hidden RGB beneath clear pixels.
    for (source, output) in pixels.pixels().zip(output.pixels_mut()) {
        output[3] = source[3];
    }
    Ok(output)
}

pub fn srgb_profile() -> Result<Vec<u8>> {
    ColorProfile::new_srgb()
        .encode()
        .context("Could not encode the output color profile")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn srgb_is_stable_and_display_p3_is_converted_without_changing_alpha() {
        let pixels = RgbaImage::from_fn(8, 1, |x, _| Rgba([160, 80, 120, x as u8 * 35]));
        let srgb = to_srgb(pixels.clone(), &srgb_profile().unwrap()).unwrap();
        for (a, b) in pixels.pixels().zip(srgb.pixels()) {
            for c in 0..3 {
                assert!((i16::from(a[c]) - i16::from(b[c])).abs() <= 1);
            }
            assert_eq!(a[3], b[3]);
        }
        let converted = to_srgb(
            pixels.clone(),
            &ColorProfile::new_display_p3().encode().unwrap(),
        )
        .unwrap();
        assert_ne!(converted, pixels);
        for (a, b) in pixels.pixels().zip(converted.pixels()) {
            assert_eq!(a[3], b[3]);
        }
        assert!(to_srgb(pixels, b"not an ICC profile").is_err());
    }
    #[test]
    fn grayscale_profile_converts_to_neutral_srgb_and_preserves_alpha() {
        let image = RgbaImage::from_pixel(2, 1, Rgba([128, 128, 128, 37]));
        let profile = ColorProfile::new_gray_with_gamma(1.8).encode().unwrap();
        let output = to_srgb(image, &profile).unwrap();
        for pixel in output.pixels() {
            assert_eq!(pixel[0], pixel[1]);
            assert_eq!(pixel[1], pixel[2]);
            assert_eq!(pixel[3], 37);
        }
    }
}
