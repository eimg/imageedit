use image::{Rgba, RgbaImage};

pub const MAX_RADIUS: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterStop {
    Unchanged,
    TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FilterSpec {
    Blur {
        radius: u32,
    },
    Sharpen {
        radius: u32,
        amount: u8,
    },
    Shadow {
        radius: u32,
        offset: (i32, i32),
        opacity: u8,
        #[serde(with = "crate::settings::rgba")]
        color: Rgba<u8>,
    },
}

impl FilterSpec {
    pub fn is_identity(self) -> bool {
        match self {
            Self::Blur { radius } => radius == 0,
            Self::Sharpen { radius, amount } => radius == 0 || amount == 0,
            Self::Shadow { opacity, .. } => opacity == 0,
        }
    }
}

pub struct Filtered {
    pub pixels: RgbaImage,
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

pub struct ShadowImage {
    pub pixels: RgbaImage,
    pub pad: i32,
}

pub fn kernel_support(radius: u32) -> u32 {
    radius.min(MAX_RADIUS) * 3
}

pub fn shadow_dimensions(width: u32, height: u32, radius: u32) -> Option<(u32, u32)> {
    let pad = kernel_support(radius);
    Some((
        width.checked_add(pad)?.checked_add(pad)?,
        height.checked_add(pad)?.checked_add(pad)?,
    ))
}

pub fn gaussian_blur(source: &RgbaImage, radius: u32) -> RgbaImage {
    gaussian_blur_alpha(source, radius, false)
}

pub(crate) fn gaussian_blur_alpha(
    source: &RgbaImage,
    radius: u32,
    alpha_locked: bool,
) -> RgbaImage {
    if radius == 0 {
        return source.clone();
    }
    let (width, height) = source.dimensions();
    if width == 0 || height == 0 {
        return source.clone();
    }
    let kernel = kernel(radius);
    let support = (kernel.len() / 2) as i32;
    let rows = height.min(kernel.len() as u32);
    let band_width = width.min(128);
    let mut horizontal = vec![[0.0; 4]; (band_width * rows) as usize];
    let mut output = RgbaImage::new(width, height);
    for left in (0..width).step_by(band_width as usize) {
        let columns = band_width.min(width - left);
        let mut next_row = 0;
        for y in 0..height {
            let needed = (y + support as u32).min(height - 1);
            while next_row <= needed {
                for x in 0..columns {
                    horizontal[((next_row % rows) * band_width + x) as usize] =
                        convolve_row(source, &kernel, support, left + x, next_row);
                }
                next_row += 1;
            }
            for x in 0..columns {
                let mut sample = [0.0; 4];
                for (index, weight) in kernel.iter().enumerate() {
                    let sy = (y as i32 + index as i32 - support).clamp(0, height as i32 - 1) as u32;
                    let pixel = horizontal[((sy % rows) * band_width + x) as usize];
                    for channel in 0..4 {
                        sample[channel] += pixel[channel] * weight;
                    }
                }
                let pixel = if alpha_locked {
                    let before = *source.get_pixel(left + x, y);
                    if before[3] == 0 || sample[3] <= 0. {
                        before
                    } else {
                        // Recover color before alpha is rounded to a byte: a faint
                        // blurred silhouette can round to zero but still carry color.
                        let channel =
                            |i: usize| (sample[i] * 255. / sample[3]).round().clamp(0., 255.) as u8;
                        Rgba([channel(0), channel(1), channel(2), before[3]])
                    }
                } else {
                    unpremultiply(sample)
                };
                output.put_pixel(left + x, y, pixel);
            }
        }
    }
    output
}

pub(crate) fn sharpen_alpha(
    source: &RgbaImage,
    radius: u32,
    amount: u8,
    alpha_locked: bool,
) -> RgbaImage {
    if radius == 0 || amount == 0 {
        return source.clone();
    }
    let blurred = gaussian_blur_alpha(source, radius, alpha_locked);
    let factor = f64::from(amount.min(100)) / 100.0;
    let (width, height) = source.dimensions();
    RgbaImage::from_fn(width, height, |x, y| {
        let before = source.get_pixel(x, y).0;
        if before[3] == 0 {
            return Rgba(before);
        }
        let soft = blurred.get_pixel(x, y).0;
        let channel = |index: usize| {
            let original = f64::from(before[index]);
            let blurred = f64::from(soft[index]);
            (original + (original - blurred) * factor)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Rgba([channel(0), channel(1), channel(2), before[3]])
    })
}

pub fn render_shadow(
    source: &RgbaImage,
    radius: u32,
    opacity: u8,
    color: Rgba<u8>,
) -> Option<ShadowImage> {
    let opacity = opacity.min(100);
    if opacity == 0 {
        return None;
    }
    let (width, height) = source.dimensions();
    let (shadow_width, shadow_height) = shadow_dimensions(width, height, radius)?;
    let pad = kernel_support(radius);
    let mut image = RgbaImage::new(shadow_width, shadow_height);
    let mut visible = false;
    for (x, y, pixel) in source.enumerate_pixels() {
        let alpha = (u32::from(pixel.0[3]) * u32::from(opacity) + 50) / 100;
        if alpha == 0 {
            continue;
        }
        visible = true;
        image.put_pixel(
            x + pad,
            y + pad,
            Rgba([color.0[0], color.0[1], color.0[2], alpha as u8]),
        );
    }
    if !visible {
        return None;
    }
    Some(ShadowImage {
        pixels: gaussian_blur(&image, radius),
        pad: pad as i32,
    })
}

pub fn apply_covered(
    source: &RgbaImage,
    filtered: &RgbaImage,
    coverage: impl Fn(u32, u32) -> u8,
    constrain: impl Fn(Rgba<u8>) -> Rgba<u8>,
) -> Option<Filtered> {
    let (width, height) = source.dimensions();
    let mut image = None;
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let amount = coverage(x, y);
            if amount == 0 {
                continue;
            }
            let before = *source.get_pixel(x, y);
            let after = blend(before, constrain(*filtered.get_pixel(x, y)), amount);
            if before == after {
                continue;
            }
            image
                .get_or_insert_with(|| source.clone())
                .put_pixel(x, y, after);
            bounds = Some(match bounds {
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x), bottom.max(y))
                }
                None => (x, y, x, y),
            });
        }
    }
    let (left, top, right, bottom) = bounds?;
    Some(Filtered {
        pixels: image?,
        left,
        top,
        right,
        bottom,
    })
}

pub fn preview(
    source: &RgbaImage,
    spec: FilterSpec,
    max_edge: u32,
    coverage: impl Fn(u32, u32) -> u8,
    constrain: impl Fn(Rgba<u8>) -> Rgba<u8>,
    alpha_locked: bool,
) -> RgbaImage {
    let (width, height) = source.dimensions();
    let (preview_width, preview_height) = crate::adjustments::preview_size(width, height, max_edge);
    let reduced = RgbaImage::from_fn(preview_width, preview_height, |x, y| {
        *source.get_pixel(
            sample_coord(x, preview_width, width),
            sample_coord(y, preview_height, height),
        )
    });
    let scale = f64::from(preview_width.max(preview_height)) / f64::from(width.max(height).max(1));
    let scaled = |radius: u32| (f64::from(radius) * scale).round() as u32;
    let constrain = &constrain;
    let original_coverage = &coverage;
    let coverage = |x, y| {
        if alpha_locked && source.get_pixel(x, y)[3] == 0 {
            0
        } else {
            original_coverage(x, y)
        }
    };
    match spec {
        FilterSpec::Blur { radius } => mix_covered(
            &reduced,
            &gaussian_blur_alpha(&reduced, scaled(radius), alpha_locked),
            width,
            height,
            &coverage,
            constrain,
        ),
        FilterSpec::Sharpen { radius, amount } => mix_covered(
            &reduced,
            &sharpen_alpha(&reduced, scaled(radius), amount, alpha_locked),
            width,
            height,
            &coverage,
            constrain,
        ),
        FilterSpec::Shadow {
            radius,
            offset,
            opacity,
            color,
        } => {
            let image = composite_shadow(
                &reduced,
                scaled(radius),
                (
                    (f64::from(offset.0) * scale).round() as i32,
                    (f64::from(offset.1) * scale).round() as i32,
                ),
                opacity,
                constrain(color),
                max_edge,
            );
            RgbaImage::from_fn(image.width(), image.height(), |x, y| {
                constrain(*image.get_pixel(x, y))
            })
        }
    }
}

fn mix_covered(
    source: &RgbaImage,
    filtered: &RgbaImage,
    full_width: u32,
    full_height: u32,
    coverage: &impl Fn(u32, u32) -> u8,
    constrain: &impl Fn(Rgba<u8>) -> Rgba<u8>,
) -> RgbaImage {
    let (preview_width, preview_height) = source.dimensions();
    apply_covered(
        source,
        filtered,
        |x, y| {
            coverage(
                sample_coord(x, preview_width, full_width),
                sample_coord(y, preview_height, full_height),
            )
        },
        constrain,
    )
    .map(|filtered| filtered.pixels)
    .unwrap_or_else(|| source.clone())
}

fn composite_shadow(
    source: &RgbaImage,
    radius: u32,
    offset: (i32, i32),
    opacity: u8,
    color: Rgba<u8>,
    max_edge: u32,
) -> RgbaImage {
    let Some(shadow) = render_shadow(source, radius, opacity, color) else {
        return source.clone();
    };
    let origin = (
        i64::from(offset.0) - i64::from(shadow.pad),
        i64::from(offset.1) - i64::from(shadow.pad),
    );
    let left = origin.0.min(0);
    let top = origin.1.min(0);
    let right = (origin.0 + i64::from(shadow.pixels.width())).max(i64::from(source.width()));
    let bottom = (origin.1 + i64::from(shadow.pixels.height())).max(i64::from(source.height()));
    let width = (right - left) as f64;
    let height = (bottom - top) as f64;
    let scale = f64::from(max_edge.max(1)) / width.max(height);
    let scale = scale.min(1.0);
    let size = (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    );
    let at = |image: &RgbaImage, x: i64, y: i64| {
        if (0..i64::from(image.width())).contains(&x) && (0..i64::from(image.height())).contains(&y)
        {
            *image.get_pixel(x as u32, y as u32)
        } else {
            Rgba([0; 4])
        }
    };
    RgbaImage::from_fn(size.0, size.1, |x, y| {
        let cx = left + ((f64::from(x) + 0.5) * width / f64::from(size.0)).floor() as i64;
        let cy = top + ((f64::from(y) + 0.5) * height / f64::from(size.1)).floor() as i64;
        crate::document::source_over(
            at(&shadow.pixels, cx - origin.0, cy - origin.1),
            at(source, cx, cy),
            255,
        )
    })
}

fn kernel(radius: u32) -> Vec<f64> {
    let sigma = f64::from(radius.max(1));
    let support = kernel_support(radius.max(1)) as i32;
    let mut weights = Vec::with_capacity((support * 2 + 1) as usize);
    let mut total = 0.0;
    for offset in -support..=support {
        let distance = f64::from(offset);
        let weight = (-distance * distance / (2.0 * sigma * sigma)).exp();
        weights.push(weight);
        total += weight;
    }
    for weight in &mut weights {
        *weight /= total;
    }
    weights
}

fn convolve_row(source: &RgbaImage, kernel: &[f64], support: i32, x: u32, y: u32) -> [f64; 4] {
    let width = source.width() as i32;
    let mut sample = [0.0; 4];
    for (index, weight) in kernel.iter().enumerate() {
        let sx = (x as i32 + index as i32 - support).clamp(0, width - 1) as u32;
        let pixel = premultiply(*source.get_pixel(sx, y));
        for channel in 0..4 {
            sample[channel] += pixel[channel] * weight;
        }
    }
    sample
}

fn premultiply(pixel: Rgba<u8>) -> [f64; 4] {
    let [red, green, blue, alpha] = pixel.0;
    let scale = f64::from(alpha) / 255.0;
    [
        f64::from(red) * scale,
        f64::from(green) * scale,
        f64::from(blue) * scale,
        f64::from(alpha),
    ]
}

fn unpremultiply(sample: [f64; 4]) -> Rgba<u8> {
    if sample[3].round() <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let scale = 255.0 / sample[3];
    let channel = |value: f64| (value * scale).round().clamp(0.0, 255.0) as u8;
    Rgba([
        channel(sample[0]),
        channel(sample[1]),
        channel(sample[2]),
        sample[3].round().clamp(0.0, 255.0) as u8,
    ])
}

pub(crate) fn blend(before: Rgba<u8>, after: Rgba<u8>, coverage: u8) -> Rgba<u8> {
    if coverage == 0 {
        return before;
    }
    if coverage == 255 {
        return after;
    }
    let before = premultiply(before);
    let after = premultiply(after);
    let fraction = f64::from(coverage) / 255.0;
    unpremultiply(std::array::from_fn(|i| {
        before[i] * (1.0 - fraction) + after[i] * fraction
    }))
}

fn sample_coord(index: u32, preview: u32, source: u32) -> u32 {
    if preview == 0 || source == 0 {
        return 0;
    }
    ((u64::from(index) * u64::from(source)) / u64::from(preview)).min(u64::from(source - 1)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(pixel: Rgba<u8>) -> Rgba<u8> {
        pixel
    }

    #[test]
    fn a_constant_image_and_a_zero_radius_stay_unchanged() {
        let source = RgbaImage::from_pixel(5, 5, Rgba([10, 20, 30, 255]));
        assert!(FilterSpec::Blur { radius: 0 }.is_identity());
        assert!(apply_covered(&source, &gaussian_blur(&source, 0), |_, _| 255, plain).is_none());
        assert!(apply_covered(&source, &gaussian_blur(&source, 1), |_, _| 255, plain).is_none());
        assert!(
            FilterSpec::Sharpen {
                radius: 2,
                amount: 0
            }
            .is_identity()
        );
        assert!(
            apply_covered(
                &source,
                &sharpen_alpha(&source, 1, 50, false),
                |_, _| 255,
                plain
            )
            .is_none()
        );
    }

    #[test]
    fn an_impulse_spreads_symmetrically_and_ignores_transparent_color() {
        let mut source = RgbaImage::from_pixel(7, 7, Rgba([0, 255, 0, 0]));
        source.put_pixel(3, 3, Rgba([255, 0, 0, 255]));
        let blurred = gaussian_blur(&source, 1);
        assert_eq!(blurred.get_pixel(3, 3).0, [255, 0, 0, 41]);
        assert_eq!(blurred.get_pixel(4, 3).0, [255, 0, 0, 25]);
        assert_eq!(blurred.get_pixel(4, 4).0, [255, 0, 0, 15]);
        assert_eq!(blurred.get_pixel(2, 3).0, blurred.get_pixel(4, 3).0);
        assert_eq!(blurred.get_pixel(0, 0).0[1], 0);
    }

    #[test]
    fn sharpen_raises_the_impulse_and_lowers_its_neighbor() {
        let mut source = RgbaImage::from_pixel(5, 5, Rgba([100, 100, 100, 255]));
        source.put_pixel(2, 2, Rgba([140, 140, 140, 255]));
        let sharpened = sharpen_alpha(&source, 1, 50, false);
        assert_eq!(sharpened.get_pixel(2, 2).0, [157, 157, 157, 255]);
        assert_eq!(sharpened.get_pixel(3, 2).0, [98, 98, 98, 255]);
        let mut edge = RgbaImage::from_pixel(3, 1, Rgba([0, 255, 0, 0]));
        edge.put_pixel(1, 0, Rgba([255, 0, 0, 255]));
        let sharpened = sharpen_alpha(&edge, 1, 50, false);
        assert_eq!(sharpened.get_pixel(1, 0).0[1], 0);
        assert_eq!(sharpened.get_pixel(1, 0).0[3], 255);
        assert_eq!(sharpened.get_pixel(0, 0).0, [0, 255, 0, 0]);
    }

    #[test]
    fn coverage_limits_the_write_and_mixes_a_partial_pixel() {
        let mut source = RgbaImage::from_pixel(3, 1, Rgba([0, 0, 0, 255]));
        source.put_pixel(1, 0, Rgba([255, 0, 0, 255]));
        let blurred = gaussian_blur(&source, 1);
        let covered = apply_covered(
            &source,
            &blurred,
            |x, _| if x == 1 { 255 } else { 0 },
            plain,
        )
        .unwrap();
        assert_eq!(covered.pixels.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_ne!(covered.pixels.get_pixel(1, 0).0, source.get_pixel(1, 0).0);
        assert_eq!(covered.left, 1);
        assert_eq!(covered.right, 1);

        let mut field = RgbaImage::from_pixel(5, 5, Rgba([100, 100, 100, 255]));
        field.put_pixel(2, 2, Rgba([140, 140, 140, 255]));
        let sharpened = sharpen_alpha(&field, 1, 50, false);
        let mixed = apply_covered(
            &field,
            &sharpened,
            |x, y| if x == 2 && y == 2 { 128 } else { 0 },
            plain,
        )
        .unwrap();
        assert_eq!(mixed.pixels.get_pixel(3, 2).0, field.get_pixel(3, 2).0);
        assert_eq!(mixed.pixels.get_pixel(2, 2).0, [149, 149, 149, 255]);
    }

    #[test]
    fn soft_filter_edges_ignore_hidden_rgb_and_shadow_previews_are_bounded() {
        let mixed = blend(Rgba([0, 255, 0, 0]), Rgba([255, 0, 0, 200]), 128);
        assert_eq!(mixed.0, [255, 0, 0, 100]);
        let source = RgbaImage::from_pixel(8, 8, Rgba([255; 4]));
        let preview = preview(
            &source,
            FilterSpec::Shadow {
                radius: 1,
                offset: (10000, -10000),
                opacity: 75,
                color: Rgba([0, 0, 0, 255]),
            },
            64,
            |_, _| 255,
            plain,
            false,
        );
        assert!(preview.width() <= 64 && preview.height() <= 64);
        assert!(render_shadow(&RgbaImage::new(8, 8), 1, 75, Rgba([0, 0, 0, 255])).is_none());
    }

    #[test]
    fn banded_blur_matches_a_direct_gaussian_across_band_and_ring_boundaries() {
        let source = RgbaImage::from_fn(133, 29, |x, y| {
            Rgba([
                (x * 7 % 256) as u8,
                (y * 19) as u8,
                17,
                ((x + y) * 23 % 256) as u8,
            ])
        });
        let weights = kernel(2);
        let support = weights.len() as i32 / 2;
        let actual = gaussian_blur(&source, 2);
        for (x, y) in [(0, 0), (127, 13), (128, 20), (132, 28)] {
            let mut expected = [0.0; 4];
            for (i, &weight) in weights.iter().enumerate() {
                let sy =
                    (y as i32 + i as i32 - support).clamp(0, source.height() as i32 - 1) as u32;
                let horizontal = convolve_row(&source, &weights, support, x, sy);
                for channel in 0..4 {
                    expected[channel] += horizontal[channel] * weight;
                }
            }
            assert_eq!(*actual.get_pixel(x, y), unpremultiply(expected));
        }
    }

    #[test]
    fn a_shadow_follows_alpha_offset_and_refuses_an_impossible_size() {
        assert!(
            FilterSpec::Shadow {
                radius: 4,
                offset: (0, 0),
                opacity: 0,
                color: Rgba([0, 0, 0, 255]),
            }
            .is_identity()
        );
        assert!(shadow_dimensions(u32::MAX, 1, 1).is_none());
        let mut source = RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 0]));
        source.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let shadow = render_shadow(&source, 0, 100, Rgba([0, 0, 40, 10])).unwrap();
        assert_eq!(shadow.pad, 0);
        assert_eq!(shadow.pixels.get_pixel(0, 0).0, [0, 0, 40, 255]);
        assert_eq!(shadow.pixels.get_pixel(1, 0).0, [0, 0, 0, 0]);
        let shadow = render_shadow(&source, 1, 50, Rgba([0, 0, 0, 255])).unwrap();
        assert_eq!(shadow.pad, 3);
        assert_eq!(shadow.pixels.dimensions(), (8, 7));
        assert!(shadow.pixels.get_pixel(3, 3).0[3] > 0);
        assert!(shadow.pixels.get_pixel(4, 3).0[3] > 0);
        assert_eq!(shadow.pixels.get_pixel(3, 3).0[0], 0);
    }
}
