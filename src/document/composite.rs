use image::{Rgba, RgbaImage};

use super::content::{BlendMode, DocumentContent, PixelBounds, RasterLayer};

pub(super) fn flatten(content: &DocumentContent) -> RgbaImage {
    let bounds = PixelBounds {
        left: 0,
        top: 0,
        right: i64::from(content.width),
        bottom: i64::from(content.height),
    };
    flatten_region(&content.layers, bounds)
}

// Composites layers bottom to top into an image covering `bounds` in canvas coordinates.
pub(super) fn flatten_region<'a>(
    layers: impl IntoIterator<Item = &'a RasterLayer>,
    bounds: PixelBounds,
) -> RgbaImage {
    flatten_with(layers, bounds, |layer, x, y| *layer.pixels.get_pixel(x, y))
}

pub(super) fn flatten_with<'a>(
    layers: impl IntoIterator<Item = &'a RasterLayer>,
    bounds: PixelBounds,
    sample: impl Fn(&RasterLayer, u32, u32) -> Rgba<u8>,
) -> RgbaImage {
    flatten_checked(layers, bounds, sample, &|| Ok(())).expect("infallible composition")
}

pub(super) fn flatten_checked<'a>(
    layers: impl IntoIterator<Item = &'a RasterLayer>,
    bounds: PixelBounds,
    sample: impl Fn(&RasterLayer, u32, u32) -> Rgba<u8>,
    check: &impl Fn() -> anyhow::Result<()>,
) -> anyhow::Result<RgbaImage> {
    check()?;
    let (width, height) = bounds.dimensions();
    let mut canvas = RgbaImage::new(width, height);
    for layer in layers {
        if !layer.visible || layer.opacity == 0 {
            continue;
        }
        let original = layer.bounds();
        let Some(clipped) = original.intersect(bounds) else {
            continue;
        };
        let (src_x, src_y) = original.local_origin(clipped);
        let (dst_x, dst_y) = bounds.local_origin(clipped);
        let (width, height) = clipped.dimensions();
        for y in 0..height {
            if y.is_multiple_of(32) {
                check()?;
            }
            for x in 0..width {
                let source =
                    layer.masked(sample(layer, src_x + x, src_y + y), src_x + x, src_y + y);
                let destination = canvas.get_pixel_mut(dst_x + x, dst_y + y);
                *destination = blend(*destination, source, layer.opacity, layer.blend_mode);
            }
        }
    }
    check()?;
    Ok(canvas)
}

pub(crate) fn source_over(destination: Rgba<u8>, source: Rgba<u8>, opacity: u8) -> Rgba<u8> {
    let source_alpha = u64::from(source[3]) * u64::from(opacity);
    if source_alpha == 0 {
        return destination;
    }
    if source_alpha == 255 * 255 {
        return source;
    }

    // Keep effective source alpha as a fraction of 255² until the final rounding.
    let source_weight = source_alpha * 255;
    let dest_weight = u64::from(destination[3]) * (255 * 255 - source_alpha);
    let total = source_weight + dest_weight;
    let channel = |i| {
        let sum = u64::from(source[i]) * source_weight + u64::from(destination[i]) * dest_weight;
        ((sum + total / 2) / total) as u8
    };
    Rgba([
        channel(0),
        channel(1),
        channel(2),
        ((total + 255 * 255 / 2) / (255 * 255)) as u8,
    ])
}

/// W3C source-over blending with straight RGB and independent layer opacity.
/// Blend colors contribute only where both source and backdrop have coverage.
pub(crate) fn blend(
    destination: Rgba<u8>,
    source: Rgba<u8>,
    opacity: u8,
    mode: BlendMode,
) -> Rgba<u8> {
    if mode == BlendMode::Normal {
        return source_over(destination, source, opacity);
    }
    let sa = f64::from(source[3]) * f64::from(opacity) / (255. * 255.);
    if sa == 0. {
        return destination;
    }
    let da = f64::from(destination[3]) / 255.;
    let alpha = sa + da * (1. - sa);
    let channel = |i| {
        let s = f64::from(source[i]) / 255.;
        let d = f64::from(destination[i]) / 255.;
        let mixed = match mode {
            BlendMode::Normal => s,
            BlendMode::Multiply => s * d,
            BlendMode::Screen => s + d - s * d,
            BlendMode::Overlay => {
                if d <= 0.5 {
                    2. * s * d
                } else {
                    1. - 2. * (1. - s) * (1. - d)
                }
            }
        };
        (((sa * (1. - da) * s + sa * da * mixed + (1. - sa) * da * d) / alpha) * 255.)
            .round()
            .clamp(0., 255.) as u8
    };
    Rgba([
        channel(0),
        channel(1),
        channel(2),
        (alpha * 255.).round() as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blend_modes_preserve_transparent_backdrops_and_zero_coverage() {
        for mode in [BlendMode::Multiply, BlendMode::Screen, BlendMode::Overlay] {
            let source = Rgba([180, 40, 90, 128]);
            assert_eq!(blend(Rgba([1, 2, 3, 0]), source, 255, mode), source);
            let backdrop = Rgba([35, 120, 220, 77]);
            assert_eq!(blend(backdrop, source, 0, mode), backdrop);
            assert_eq!(
                blend(backdrop, Rgba([255, 255, 255, 0]), 255, mode),
                backdrop
            );
        }
    }
    #[test]
    fn blend_modes_match_known_opaque_and_partial_alpha_results() {
        let d = Rgba([64, 128, 192, 255]);
        let s = Rgba([128, 128, 128, 255]);
        assert_eq!(
            blend(d, s, 255, BlendMode::Multiply),
            Rgba([32, 64, 96, 255])
        );
        assert_eq!(
            blend(d, s, 255, BlendMode::Screen),
            Rgba([160, 192, 224, 255])
        );
        assert_eq!(
            blend(d, s, 255, BlendMode::Overlay),
            Rgba([64, 128, 192, 255])
        );
        let partial = blend(
            Rgba([0, 0, 255, 128]),
            Rgba([255, 0, 0, 128]),
            128,
            BlendMode::Multiply,
        );
        assert_eq!(partial, Rgba([51, 0, 153, 160]));
    }
}
