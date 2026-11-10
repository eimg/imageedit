use image::{Rgba, RgbaImage, imageops::FilterType};

use crate::document::{MAX_IMAGE_BYTES, MAX_NEW_EDGE};
use crate::preview::PixelRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    // 90 degrees counter-clockwise.
    Left,
    // 90 degrees clockwise.
    Right,
    Half,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirror {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resample {
    Nearest,
    Bilinear,
    Bicubic,
}

// Where the existing canvas stays put when the canvas grows or shrinks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    NorthWest,
    North,
    NorthEast,
    West,
    Center,
    East,
    SouthWest,
    South,
    SouthEast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    Rgb,
    Grayscale,
}

impl ColorMode {
    pub fn constrain(self, color: Rgba<u8>) -> Rgba<u8> {
        match self {
            Self::Rgb => color,
            Self::Grayscale => gray_pixel(color),
        }
    }

    pub fn constrain_image(self, image: RgbaImage) -> RgbaImage {
        match self {
            Self::Rgb => image,
            Self::Grayscale => grayscale(image),
        }
    }
}

pub fn canvas_allowed(width: u32, height: u32) -> bool {
    (1..=MAX_NEW_EDGE).contains(&width)
        && (1..=MAX_NEW_EDGE).contains(&height)
        && u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4)
            <= MAX_IMAGE_BYTES
}

pub fn turned_size(width: u32, height: u32, turn: Turn) -> (u32, u32) {
    match turn {
        Turn::Half => (width, height),
        Turn::Left | Turn::Right => (height, width),
    }
}

pub fn turn_pixels(pixels: &RgbaImage, turn: Turn) -> RgbaImage {
    match turn {
        Turn::Right => image::imageops::rotate90(pixels),
        Turn::Left => image::imageops::rotate270(pixels),
        Turn::Half => image::imageops::rotate180(pixels),
    }
}

pub fn mirror_pixels(pixels: &RgbaImage, mirror: Mirror) -> RgbaImage {
    match mirror {
        Mirror::Horizontal => image::imageops::flip_horizontal(pixels),
        Mirror::Vertical => image::imageops::flip_vertical(pixels),
    }
}

fn fit_offset(x: i64, y: i64) -> Option<(i32, i32)> {
    Some((i32::try_from(x).ok()?, i32::try_from(y).ok()?))
}

pub fn turned_offset(
    offset: (i32, i32),
    layer: (u32, u32),
    canvas: (u32, u32),
    turn: Turn,
) -> Option<(i32, i32)> {
    let (ox, oy) = (i64::from(offset.0), i64::from(offset.1));
    let (lw, lh) = (i64::from(layer.0), i64::from(layer.1));
    let (width, height) = (i64::from(canvas.0), i64::from(canvas.1));
    let (x, y) = match turn {
        Turn::Right => (height - oy - lh, ox),
        Turn::Left => (oy, width - ox - lw),
        Turn::Half => (width - ox - lw, height - oy - lh),
    };
    fit_offset(x, y)
}

pub fn mirrored_offset(
    offset: (i32, i32),
    layer: (u32, u32),
    canvas: (u32, u32),
    mirror: Mirror,
) -> Option<(i32, i32)> {
    let (ox, oy) = (i64::from(offset.0), i64::from(offset.1));
    let (lw, lh) = (i64::from(layer.0), i64::from(layer.1));
    let (width, height) = (i64::from(canvas.0), i64::from(canvas.1));
    let (x, y) = match mirror {
        Mirror::Horizontal => (width - ox - lw, oy),
        Mirror::Vertical => (ox, height - oy - lh),
    };
    fit_offset(x, y)
}

// Keeps the layer's center, rounding half pixels away from zero.
pub fn centered_offset(offset: (i32, i32), old: (u32, u32), new: (u32, u32)) -> (i32, i32) {
    let place = |origin: i32, before: u32, after: u32| {
        let value = f64::from(origin) + f64::from(before) / 2.0 - f64::from(after) / 2.0;
        value
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    };
    (place(offset.0, old.0, new.0), place(offset.1, old.1, new.1))
}

pub fn anchor_delta(old: (u32, u32), new: (u32, u32), anchor: Anchor) -> (i32, i32) {
    let place = |before: u32, after: u32, at: u8| match at {
        0 => 0,
        1 => i64::from(after) / 2 - i64::from(before) / 2,
        _ => i64::from(after) - i64::from(before),
    };
    let (x, y) = match anchor {
        Anchor::NorthWest => (0, 0),
        Anchor::North => (1, 0),
        Anchor::NorthEast => (2, 0),
        Anchor::West => (0, 1),
        Anchor::Center => (1, 1),
        Anchor::East => (2, 1),
        Anchor::SouthWest => (0, 2),
        Anchor::South => (1, 2),
        Anchor::SouthEast => (2, 2),
    };
    (place(old.0, new.0, x) as i32, place(old.1, new.1, y) as i32)
}

pub fn turn_rect(rect: PixelRect, canvas: (u32, u32), turn: Turn) -> PixelRect {
    let (x, y) = (rect.x, rect.y);
    let (w, h) = (i64::from(rect.width), i64::from(rect.height));
    let (width, height) = (i64::from(canvas.0), i64::from(canvas.1));
    let (nx, ny, nw, nh) = match turn {
        Turn::Right => (height - (y + h), x, h, w),
        Turn::Left => (y, width - (x + w), h, w),
        Turn::Half => (width - (x + w), height - (y + h), w, h),
    };
    PixelRect::new(nx, ny, nw as u32, nh as u32)
}

pub fn mirror_rect(rect: PixelRect, canvas: (u32, u32), mirror: Mirror) -> PixelRect {
    let (x, y) = (rect.x, rect.y);
    let (w, h) = (i64::from(rect.width), i64::from(rect.height));
    let (width, height) = (i64::from(canvas.0), i64::from(canvas.1));
    let (nx, ny) = match mirror {
        Mirror::Horizontal => (width - (x + w), y),
        Mirror::Vertical => (x, height - (y + h)),
    };
    PixelRect::new(nx, ny, rect.width, rect.height)
}

pub fn translate_rect(rect: PixelRect, delta: (i32, i32)) -> PixelRect {
    PixelRect::new(
        rect.x + i64::from(delta.0),
        rect.y + i64::from(delta.1),
        rect.width,
        rect.height,
    )
}

pub fn scale_rect(rect: PixelRect, old: (u32, u32), new: (u32, u32)) -> PixelRect {
    let (x, width) = scale_span(rect.x, rect.width, old.0, new.0);
    let (y, height) = scale_span(rect.y, rect.height, old.1, new.1);
    PixelRect::new(x, y, width, height)
}

// The layer's new origin and pixel size when the canvas scale is applied to its edges.
pub fn scale_placement(
    offset: (i32, i32),
    layer: (u32, u32),
    old: (u32, u32),
    new: (u32, u32),
) -> Option<((i32, i32), (u32, u32))> {
    let (x, width) = scale_span(i64::from(offset.0), layer.0, old.0, new.0);
    let (y, height) = scale_span(i64::from(offset.1), layer.1, old.1, new.1);
    Some((
        (i32::try_from(x).ok()?, i32::try_from(y).ok()?),
        (width.max(1), height.max(1)),
    ))
}

pub fn scale_pixels(pixels: &RgbaImage, width: u32, height: u32, resample: Resample) -> RgbaImage {
    if pixels.dimensions() == (width, height) {
        return pixels.clone();
    }
    match resample {
        Resample::Nearest => image::imageops::resize(pixels, width, height, FilterType::Nearest),
        Resample::Bilinear | Resample::Bicubic => {
            let filter = match resample {
                Resample::Bicubic => FilterType::CatmullRom,
                _ => FilterType::Triangle,
            };
            let scaled = image::imageops::resize(&premultiply(pixels), width, height, filter);
            unpremultiply(&scaled)
        }
    }
}

pub fn gray_pixel(color: Rgba<u8>) -> Rgba<u8> {
    let [r, g, b, a] = color.0;
    let gray = ((u16::from(r) * 77 + u16::from(g) * 150 + u16::from(b) * 29 + 128) >> 8) as u8;
    Rgba([gray, gray, gray, a])
}

pub fn grayscale(mut image: RgbaImage) -> RgbaImage {
    for pixel in image.pixels_mut() {
        *pixel = gray_pixel(*pixel);
    }
    image
}

fn scale_span(start: i64, length: u32, old: u32, new: u32) -> (i64, u32) {
    let end = start.saturating_add(i64::from(length));
    let map = |value: i64| value.saturating_mul(i64::from(new)) / i64::from(old);
    let mapped = map(start);
    let width = map(end).saturating_sub(mapped);
    (mapped, u32::try_from(width).unwrap_or(u32::MAX))
}

fn premultiply(pixels: &RgbaImage) -> RgbaImage {
    let mut image = pixels.clone();
    for pixel in image.pixels_mut() {
        let alpha = u16::from(pixel.0[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    image
}

fn unpremultiply(pixels: &RgbaImage) -> RgbaImage {
    let mut image = pixels.clone();
    for pixel in image.pixels_mut() {
        let alpha = u16::from(pixel.0[3]);
        if alpha == 0 {
            pixel.0 = [0, 0, 0, 0];
            continue;
        }
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> RgbaImage {
        let mut image = RgbaImage::new(3, 1);
        image.put_pixel(0, 0, Rgba([1, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([2, 0, 0, 128]));
        image.put_pixel(2, 0, Rgba([3, 0, 0, 0]));
        image
    }

    #[test]
    fn turning_an_odd_canvas_keeps_asymmetric_pixels_and_off_canvas_offsets() {
        let turned = turn_pixels(&row(), Turn::Right);
        assert_eq!(turned.dimensions(), (1, 3));
        assert_eq!(turned.get_pixel(0, 0).0, [1, 0, 0, 255]);
        assert_eq!(turned.get_pixel(0, 2).0, [3, 0, 0, 0]);
        assert_eq!(
            turned_offset((-1, 0), (3, 1), (5, 3), Turn::Right),
            Some((2, -1))
        );
        let selection = PixelRect::new(0, 0, 1, 2);
        assert_eq!(
            turn_rect(selection, (5, 3), Turn::Right),
            PixelRect::new(1, 0, 2, 1)
        );
    }

    #[test]
    fn a_layer_turn_keeps_its_center_on_an_even_size() {
        assert_eq!(centered_offset((0, 0), (2, 1), (1, 2)), (1, -1));
        assert_eq!(centered_offset((4, -2), (3, 3), (3, 3)), (4, -2));
    }

    #[test]
    fn flipping_mirrors_pixels_and_the_canvas_edges() {
        let flipped = mirror_pixels(&row(), Mirror::Horizontal);
        assert_eq!(flipped.get_pixel(0, 0).0, [3, 0, 0, 0]);
        assert_eq!(flipped.get_pixel(2, 0).0, [1, 0, 0, 255]);
        assert_eq!(
            mirrored_offset((-1, 2), (3, 1), (5, 4), Mirror::Horizontal),
            Some((3, 2))
        );
    }

    #[test]
    fn canvas_anchors_slide_content_without_resampling() {
        assert_eq!(anchor_delta((4, 2), (6, 2), Anchor::East), (2, 0));
        assert_eq!(anchor_delta((4, 2), (6, 4), Anchor::Center), (1, 1));
        assert_eq!(anchor_delta((4, 2), (2, 2), Anchor::NorthWest), (0, 0));
    }

    #[test]
    fn nearest_scale_duplicates_pixels_and_edges_scale_with_the_canvas() {
        let scaled = scale_pixels(&row(), 6, 1, Resample::Nearest);
        assert_eq!(scaled.get_pixel(0, 0).0, [1, 0, 0, 255]);
        assert_eq!(scaled.get_pixel(2, 0).0, [2, 0, 0, 128]);
        assert_eq!(scaled.get_pixel(5, 0).0, [3, 0, 0, 0]);
        assert_eq!(
            scale_placement((-1, 0), (2, 1), (4, 2), (8, 2)),
            Some(((-2, 0), (4, 1)))
        );
        assert_eq!(
            scale_rect(PixelRect::new(1, 0, 2, 1), (4, 2), (8, 4)),
            PixelRect::new(2, 0, 4, 2)
        );
    }

    #[test]
    fn bilinear_scale_keeps_the_color_of_a_transparent_edge() {
        let mut image = RgbaImage::new(2, 1);
        image.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([0, 0, 0, 0]));
        let scaled = scale_pixels(&image, 3, 1, Resample::Bilinear);
        let middle = scaled.get_pixel(1, 0).0;
        assert!(middle[3] > 0 && middle[3] < 255);
        let red = u16::from(middle[0]) * 255 / u16::from(middle[3]);
        assert!(red > 200, "{middle:?}");
        assert_eq!(middle[1], 0);
    }

    #[test]
    fn grayscale_keeps_alpha_and_matches_every_channel() {
        let color = gray_pixel(Rgba([10, 20, 30, 64]));
        assert_eq!(color.0[0], color.0[1]);
        assert_eq!(color.0[1], color.0[2]);
        assert_eq!(color.0[3], 64);
        assert_eq!(gray_pixel(color).0, color.0);
        assert!(!canvas_allowed(0, 4));
        assert!(canvas_allowed(8192, 8192));
        assert!(!canvas_allowed(16_384, 16_384));
    }
}
