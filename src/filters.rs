use image::{Rgba, RgbaImage};

pub const MAX_RADIUS: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterStop {
    Unchanged,
    TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    if radius == 0 {
        0
    } else {
        (f64::from(radius) * 3.0).ceil() as u32
    }
}

pub fn shadow_dimensions(width: u32, height: u32, radius: u32) -> Option<(u32, u32)> {
    let pad = kernel_support(radius);
    Some((
        width.checked_add(pad)?.checked_add(pad)?,
        height.checked_add(pad)?.checked_add(pad)?,
    ))
}

pub fn gaussian_blur(source: &RgbaImage, radius: u32) -> RgbaImage {
    if radius == 0 {
        return source.clone();
    }
    let kernel = kernel(radius);
    let support = (kernel.len() / 2) as i32;
    let (width, height) = source.dimensions();
    let mut horizontal = vec![[0.0; 4]; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            horizontal[(y * width + x) as usize] = convolve_row(source, &kernel, support, x, y);
        }
    }
    let mut output = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let sample = convolve_buffer(&horizontal, width, height, &kernel, support, x, y);
            output.put_pixel(x, y, unpremultiply(sample));
        }
    }
    output
}

pub fn sharpen(source: &RgbaImage, radius: u32, amount: u8) -> RgbaImage {
    if radius == 0 || amount == 0 {
        return source.clone();
    }
    let blurred = gaussian_blur(source, radius);
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
    for (x, y, pixel) in source.enumerate_pixels() {
        let alpha = (u32::from(pixel.0[3]) * u32::from(opacity) + 50) / 100;
        if alpha == 0 {
            continue;
        }
        image.put_pixel(
            x + pad,
            y + pad,
            Rgba([color.0[0], color.0[1], color.0[2], alpha as u8]),
        );
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
    match spec {
        FilterSpec::Blur { radius } => mix_covered(
            &reduced,
            &gaussian_blur(&reduced, scaled(radius)),
            width,
            height,
            &coverage,
            constrain,
        ),
        FilterSpec::Sharpen { radius, amount } => mix_covered(
            &reduced,
            &sharpen(&reduced, scaled(radius), amount),
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
) -> RgbaImage {
    let Some(shadow) = render_shadow(source, radius, opacity, color) else {
        return source.clone();
    };
    let origin = (-shadow.pad + offset.0, -shadow.pad + offset.1);
    let left = origin.0.min(0);
    let top = origin.1.min(0);
    let right = (origin.0 + shadow.pixels.width() as i32).max(source.width() as i32);
    let bottom = (origin.1 + shadow.pixels.height() as i32).max(source.height() as i32);
    let mut image = RgbaImage::new((right - left) as u32, (bottom - top) as u32);
    blit(
        &mut image,
        &shadow.pixels,
        origin.0 - left,
        origin.1 - top,
        false,
    );
    blit(&mut image, source, -left, -top, true);
    image
}

fn blit(destination: &mut RgbaImage, source: &RgbaImage, origin_x: i32, origin_y: i32, over: bool) {
    for (x, y, pixel) in source.enumerate_pixels() {
        let dx = origin_x + x as i32;
        let dy = origin_y + y as i32;
        if dx < 0 || dy < 0 {
            continue;
        }
        let (dx, dy) = (dx as u32, dy as u32);
        if dx >= destination.width() || dy >= destination.height() {
            continue;
        }
        let pixel = if over {
            source_over(*destination.get_pixel(dx, dy), *pixel)
        } else {
            *pixel
        };
        destination.put_pixel(dx, dy, pixel);
    }
}

fn source_over(destination: Rgba<u8>, source: Rgba<u8>) -> Rgba<u8> {
    let source_alpha = u32::from(source.0[3]);
    let destination_alpha = u32::from(destination.0[3]);
    if source_alpha == 0 {
        return destination;
    }
    let out_alpha = source_alpha + destination_alpha * (255 - source_alpha) / 255;
    if out_alpha == 0 {
        return Rgba([0, 0, 0, 0]);
    }
    let channel = |src: u8, dst: u8| {
        ((u32::from(src) * source_alpha
            + u32::from(dst) * destination_alpha * (255 - source_alpha) / 255)
            / out_alpha) as u8
    };
    Rgba([
        channel(source.0[0], destination.0[0]),
        channel(source.0[1], destination.0[1]),
        channel(source.0[2], destination.0[2]),
        out_alpha as u8,
    ])
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

fn convolve_buffer(
    buffer: &[[f64; 4]],
    width: u32,
    height: u32,
    kernel: &[f64],
    support: i32,
    x: u32,
    y: u32,
) -> [f64; 4] {
    let mut sample = [0.0; 4];
    for (index, weight) in kernel.iter().enumerate() {
        let sy = (y as i32 + index as i32 - support).clamp(0, height as i32 - 1) as u32;
        let pixel = buffer[(sy * width + x) as usize];
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
    if sample[3] <= 0.5 {
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

fn blend(before: Rgba<u8>, after: Rgba<u8>, coverage: u8) -> Rgba<u8> {
    if coverage == 0 {
        return before;
    }
    if coverage == 255 {
        return after;
    }
    let mix = |from: u8, to: u8| {
        ((u32::from(from) * u32::from(255 - coverage) + u32::from(to) * u32::from(coverage) + 127)
            / 255) as u8
    };
    Rgba([
        mix(before.0[0], after.0[0]),
        mix(before.0[1], after.0[1]),
        mix(before.0[2], after.0[2]),
        mix(before.0[3], after.0[3]),
    ])
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
        assert!(apply_covered(&source, &sharpen(&source, 1, 50), |_, _| 255, plain).is_none());
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
        let sharpened = sharpen(&source, 1, 50);
        assert_eq!(sharpened.get_pixel(2, 2).0, [157, 157, 157, 255]);
        assert_eq!(sharpened.get_pixel(3, 2).0, [98, 98, 98, 255]);
        let mut edge = RgbaImage::from_pixel(3, 1, Rgba([0, 255, 0, 0]));
        edge.put_pixel(1, 0, Rgba([255, 0, 0, 255]));
        let sharpened = sharpen(&edge, 1, 50);
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
        let sharpened = sharpen(&field, 1, 50);
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
