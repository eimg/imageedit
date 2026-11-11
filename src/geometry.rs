use image::RgbaImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentCrop {
    // Every sampled pixel is transparent or outside the canvas.
    Empty,
    // The content already fills the layer or the canvas.
    Unchanged,
    Crop(PixelBox),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    Top,
    Bottom,
    HorizontalCenter,
    VerticalCenter,
    Both,
}

pub struct LayerSample<'a> {
    pub offset: (i32, i32),
    pub pixels: &'a RgbaImage,
    pub visible: bool,
    pub opacity: u8,
}

pub fn aligned_offset(
    offset: (i32, i32),
    layer_width: u32,
    layer_height: u32,
    canvas_width: u32,
    canvas_height: u32,
    alignment: Alignment,
) -> (i32, i32) {
    let clamp = |value: i64| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let center = |canvas: u32, layer: u32| clamp((i64::from(canvas) - i64::from(layer)) / 2);
    let edge = |canvas: u32, layer: u32| clamp(i64::from(canvas) - i64::from(layer));
    match alignment {
        Alignment::Left => (0, offset.1),
        Alignment::Right => (edge(canvas_width, layer_width), offset.1),
        Alignment::Top => (offset.0, 0),
        Alignment::Bottom => (offset.0, edge(canvas_height, layer_height)),
        Alignment::HorizontalCenter => (center(canvas_width, layer_width), offset.1),
        Alignment::VerticalCenter => (offset.0, center(canvas_height, layer_height)),
        Alignment::Both => (
            center(canvas_width, layer_width),
            center(canvas_height, layer_height),
        ),
    }
}

pub fn nonzero_alpha_bounds(pixels: &RgbaImage) -> Option<PixelBox> {
    let (width, height) = pixels.dimensions();
    let (mut min_x, mut min_y) = (width, height);
    let (mut max_x, mut max_y) = (0, 0);
    let mut found = false;
    for (x, y, pixel) in pixels.enumerate_pixels() {
        if pixel.0[3] == 0 {
            continue;
        }
        found = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    found.then(|| PixelBox {
        x: min_x,
        y: min_y,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
    })
}

pub fn layer_content_crop(pixels: &RgbaImage) -> ContentCrop {
    let Some(bounds) = nonzero_alpha_bounds(pixels) else {
        return ContentCrop::Empty;
    };
    let (width, height) = pixels.dimensions();
    if bounds.x == 0 && bounds.y == 0 && bounds.width == width && bounds.height == height {
        ContentCrop::Unchanged
    } else {
        ContentCrop::Crop(bounds)
    }
}

// Copies the part of the layer that already lies on the canvas into a canvas-sized image.
// Pixels outside the canvas are dropped. An exact fit at the origin needs no change.
pub fn layer_to_image_size(
    offset: (i32, i32),
    pixels: &RgbaImage,
    canvas_width: u32,
    canvas_height: u32,
) -> Option<RgbaImage> {
    if offset == (0, 0) && pixels.dimensions() == (canvas_width, canvas_height) {
        return None;
    }
    let mut image = RgbaImage::new(canvas_width, canvas_height);
    let (width, height) = pixels.dimensions();
    for y in 0..height {
        for x in 0..width {
            let canvas_x = i64::from(offset.0) + i64::from(x);
            let canvas_y = i64::from(offset.1) + i64::from(y);
            if canvas_x < 0
                || canvas_y < 0
                || canvas_x >= i64::from(canvas_width)
                || canvas_y >= i64::from(canvas_height)
            {
                continue;
            }
            image.put_pixel(canvas_x as u32, canvas_y as u32, *pixels.get_pixel(x, y));
        }
    }
    Some(image)
}

// The canvas rectangle covering visible, nonzero-alpha pixels. Hidden layers and
// pixels that sit off the canvas do not enlarge it.
pub fn image_content_crop(layers: &[LayerSample<'_>], width: u32, height: u32) -> ContentCrop {
    let (mut min_x, mut min_y) = (width, height);
    let (mut max_x, mut max_y) = (0, 0);
    let mut found = false;
    for layer in layers {
        if !layer.visible || layer.opacity == 0 {
            continue;
        }
        let (layer_width, layer_height) = layer.pixels.dimensions();
        for y in 0..layer_height {
            for x in 0..layer_width {
                if layer.pixels.get_pixel(x, y).0[3] == 0 {
                    continue;
                }
                let canvas_x = i64::from(layer.offset.0) + i64::from(x);
                let canvas_y = i64::from(layer.offset.1) + i64::from(y);
                if canvas_x < 0
                    || canvas_y < 0
                    || canvas_x >= i64::from(width)
                    || canvas_y >= i64::from(height)
                {
                    continue;
                }
                let (canvas_x, canvas_y) = (canvas_x as u32, canvas_y as u32);
                found = true;
                min_x = min_x.min(canvas_x);
                min_y = min_y.min(canvas_y);
                max_x = max_x.max(canvas_x);
                max_y = max_y.max(canvas_y);
            }
        }
    }
    if !found {
        return ContentCrop::Empty;
    }
    let bounds = PixelBox {
        x: min_x,
        y: min_y,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
    };
    if bounds.x == 0 && bounds.y == 0 && bounds.width == width && bounds.height == height {
        ContentCrop::Unchanged
    } else {
        ContentCrop::Crop(bounds)
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn pixel(alpha: u8) -> RgbaImage {
        RgbaImage::from_pixel(1, 1, Rgba([9, 8, 7, alpha]))
    }

    #[test]
    fn alignment_edges_and_centers_use_the_canvas() {
        assert_eq!(
            aligned_offset((1, 2), 4, 6, 10, 20, Alignment::Left),
            (0, 2)
        );
        assert_eq!(
            aligned_offset((1, 2), 4, 6, 10, 20, Alignment::Right),
            (6, 2)
        );
        assert_eq!(aligned_offset((1, 2), 4, 6, 10, 20, Alignment::Top), (1, 0));
        assert_eq!(
            aligned_offset((1, 2), 4, 6, 10, 20, Alignment::Bottom),
            (1, 14)
        );
        assert_eq!(
            aligned_offset((1, 2), 4, 6, 10, 20, Alignment::HorizontalCenter),
            (3, 2)
        );
        assert_eq!(
            aligned_offset((1, 2), 4, 6, 10, 20, Alignment::VerticalCenter),
            (1, 7)
        );
        assert_eq!(
            aligned_offset((1, 2), 4, 6, 10, 20, Alignment::Both),
            (3, 7)
        );
    }

    #[test]
    fn nonzero_alpha_includes_partial_pixels_and_rejects_a_clear_image() {
        let mut image = RgbaImage::new(4, 3);
        image.put_pixel(1, 0, Rgba([0, 0, 0, 1]));
        image.put_pixel(3, 2, Rgba([0, 0, 0, 255]));
        assert_eq!(
            nonzero_alpha_bounds(&image),
            Some(PixelBox {
                x: 1,
                y: 0,
                width: 3,
                height: 3,
            })
        );
        assert_eq!(
            layer_content_crop(&RgbaImage::new(2, 2)),
            ContentCrop::Empty
        );
        assert_eq!(layer_content_crop(&pixel(255)), ContentCrop::Unchanged);
    }

    #[test]
    fn layer_to_image_size_keeps_on_canvas_pixels_and_drops_the_rest() {
        let mut image = RgbaImage::new(3, 2);
        image.put_pixel(0, 0, Rgba([9, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([1, 0, 0, 255]));
        image.put_pixel(2, 1, Rgba([2, 0, 0, 255]));
        let fitted = layer_to_image_size((-1, 0), &image, 2, 2).unwrap();
        assert_eq!(fitted.get_pixel(0, 0).0, [1, 0, 0, 255]);
        assert_eq!(fitted.get_pixel(1, 1).0, [2, 0, 0, 255]);
        assert_eq!(fitted.get_pixel(1, 0).0, [0, 0, 0, 0]);
        assert!(layer_to_image_size((0, 0), &pixel(255), 1, 1).is_none());
    }

    #[test]
    fn image_content_ignores_hidden_clear_and_off_canvas_pixels() {
        let mut visible = RgbaImage::new(3, 2);
        visible.put_pixel(1, 0, Rgba([0, 0, 0, 200]));
        let mut hidden = RgbaImage::new(1, 1);
        hidden.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        let layers = [
            LayerSample {
                offset: (1, 1),
                pixels: &visible,
                visible: true,
                opacity: 255,
            },
            LayerSample {
                offset: (0, 0),
                pixels: &hidden,
                visible: false,
                opacity: 255,
            },
            LayerSample {
                offset: (-2, 0),
                pixels: &pixel(255),
                visible: true,
                opacity: 255,
            },
        ];
        assert_eq!(
            image_content_crop(&layers, 8, 8),
            ContentCrop::Crop(PixelBox {
                x: 2,
                y: 1,
                width: 1,
                height: 1,
            })
        );
        assert_eq!(
            image_content_crop(
                &[LayerSample {
                    offset: (0, 0),
                    pixels: &RgbaImage::new(2, 2),
                    visible: true,
                    opacity: 255,
                }],
                4,
                4
            ),
            ContentCrop::Empty
        );
    }
}
