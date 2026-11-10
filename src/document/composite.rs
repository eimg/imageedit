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
            for x in 0..width {
                let source = *layer.pixels.get_pixel(src_x + x, src_y + y);
                let destination = canvas.get_pixel_mut(dst_x + x, dst_y + y);
                match layer.blend_mode {
                    BlendMode::Normal => {
                        *destination = source_over(*destination, source, layer.opacity)
                    }
                }
            }
        }
    }
    canvas
}

fn source_over(destination: Rgba<u8>, source: Rgba<u8>, opacity: u8) -> Rgba<u8> {
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
