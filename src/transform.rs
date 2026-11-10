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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Warp {
    Rotate(f64),
    Flip(Mirror),
    Perspective([(f64, f64); 4]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarpError {
    Singular,
    Crossing,
    TooLarge,
}

impl WarpError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Singular => "That shape is too flat to transform.",
            Self::Crossing => "The corners cross. Move them so the edges do not intersect.",
            Self::TooLarge => "The transformed layer is too large.",
        }
    }
}

pub struct Placed {
    pub pixels: RgbaImage,
    pub offset: (i32, i32),
    pub bounds: (u32, u32),
}

pub fn layer_corners(offset: (i32, i32), size: (u32, u32)) -> [(f64, f64); 4] {
    let x = f64::from(offset.0);
    let y = f64::from(offset.1);
    let w = f64::from(size.0);
    let h = f64::from(size.1);
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedBox {
    pub offset: (i32, i32),
    pub size: (u32, u32),
}

pub fn warp_bounds(
    offset: (i32, i32),
    size: (u32, u32),
    warp: Warp,
) -> Result<PlacedBox, WarpError> {
    if size.0 == 0 || size.1 == 0 {
        return Err(WarpError::Singular);
    }
    let corners = match warp {
        Warp::Rotate(degrees) => {
            if !degrees.is_finite() {
                return Err(WarpError::Singular);
            }
            rotate_corners(layer_corners(offset, size), degrees)
        }
        Warp::Flip(_) => layer_corners(offset, size),
        Warp::Perspective(corners) => {
            validate_quad(corners)?;
            corners
        }
    };
    bounds_from_corners(corners)
}

// `max_edge` rasterizes a coarser preview into the same bounds. `None` writes every pixel.
pub fn warp_layer(
    pixels: &RgbaImage,
    offset: (i32, i32),
    warp: Warp,
    resample: Resample,
    max_edge: Option<u32>,
) -> Result<Placed, WarpError> {
    let size = pixels.dimensions();
    let PlacedBox {
        offset: placed_offset,
        size: bounds,
    } = warp_bounds(offset, size, warp)?;
    if matches!(warp, Warp::Flip(_)) {
        let mirror = match warp {
            Warp::Flip(mirror) => mirror,
            _ => Mirror::Horizontal,
        };
        return Ok(Placed {
            pixels: mirror_pixels(pixels, mirror),
            offset,
            bounds: size,
        });
    }
    if identity_warp(offset, size, warp) {
        return Ok(Placed {
            pixels: pixels.clone(),
            offset,
            bounds: size,
        });
    }
    let grid = output_grid(bounds, max_edge);
    let mut image = RgbaImage::new(grid.0, grid.1);
    let source = layer_corners(offset, size);
    let dest = match warp {
        Warp::Rotate(degrees) => rotate_corners(source, degrees),
        Warp::Perspective(corners) => corners,
        Warp::Flip(_) => source,
    };
    for y in 0..grid.1 {
        for x in 0..grid.0 {
            let canvas_x = f64::from(placed_offset.0)
                + (f64::from(x) + 0.5) * f64::from(bounds.0) / f64::from(grid.0);
            let canvas_y = f64::from(placed_offset.1)
                + (f64::from(y) + 0.5) * f64::from(bounds.1) / f64::from(grid.1);
            let Some((u, v)) = unmap_quad(dest, (canvas_x, canvas_y)) else {
                continue;
            };
            let sample_x = u * f64::from(size.0) - 0.5;
            let sample_y = v * f64::from(size.1) - 0.5;
            image.put_pixel(x, y, sample_pixel(pixels, sample_x, sample_y, resample));
        }
    }
    Ok(Placed {
        pixels: image,
        offset: placed_offset,
        bounds,
    })
}

pub fn map_quad(corners: [(f64, f64); 4], u: f64, v: f64) -> (f64, f64) {
    let (top_x, top_y) = lerp(corners[0], corners[1], u);
    let (bottom_x, bottom_y) = lerp(corners[3], corners[2], u);
    lerp((top_x, top_y), (bottom_x, bottom_y), v)
}

pub fn unmap_quad(corners: [(f64, f64); 4], point: (f64, f64)) -> Option<(f64, f64)> {
    let (u, v) = invert_bilinear(corners, point)?;
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return None;
    }
    let mapped = map_quad(corners, u, v);
    let error = (mapped.0 - point.0).hypot(mapped.1 - point.1);
    (error < 0.5).then_some((u, v))
}

fn identity_warp(offset: (i32, i32), size: (u32, u32), warp: Warp) -> bool {
    match warp {
        Warp::Rotate(degrees) => {
            let turns = degrees / 360.0;
            (turns - turns.round()).abs() < 1e-6
        }
        Warp::Flip(_) => false,
        Warp::Perspective(corners) => corners
            .iter()
            .zip(layer_corners(offset, size))
            .all(|(corner, source)| (corner.0 - source.0).hypot(corner.1 - source.1) < 1e-3),
    }
}

pub(crate) fn rotate_corners(corners: [(f64, f64); 4], degrees: f64) -> [(f64, f64); 4] {
    let center = (
        (corners[0].0 + corners[2].0) / 2.0,
        (corners[0].1 + corners[2].1) / 2.0,
    );
    let (sin, cos) = degrees.to_radians().sin_cos();
    corners.map(|corner| {
        let dx = corner.0 - center.0;
        let dy = corner.1 - center.1;
        (
            center.0 + dx * cos - dy * sin,
            center.1 + dx * sin + dy * cos,
        )
    })
}

fn validate_quad(corners: [(f64, f64); 4]) -> Result<(), WarpError> {
    if corners
        .iter()
        .any(|corner| !corner.0.is_finite() || !corner.1.is_finite())
    {
        return Err(WarpError::Singular);
    }
    for index in 0..4 {
        let edge = corners[index];
        let next = corners[(index + 1) % 4];
        if (next.0 - edge.0).hypot(next.1 - edge.1) < 1e-3 {
            return Err(WarpError::Singular);
        }
    }
    if quad_area(corners).abs() < 0.5 {
        return Err(WarpError::Singular);
    }
    let crosses = segments_cross(corners[0], corners[1], corners[2], corners[3])
        || segments_cross(corners[1], corners[2], corners[3], corners[0]);
    if crosses {
        Err(WarpError::Crossing)
    } else {
        Ok(())
    }
}

fn quad_area(corners: [(f64, f64); 4]) -> f64 {
    let mut sum = 0.0;
    for index in 0..4 {
        let current = corners[index];
        let next = corners[(index + 1) % 4];
        sum += current.0 * next.1 - next.0 * current.1;
    }
    sum * 0.5
}

fn segments_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let first = orient(a, b, c) * orient(a, b, d);
    let second = orient(c, d, a) * orient(c, d, b);
    first < 0.0 && second < 0.0
}

fn orient(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

fn bounds_from_corners(corners: [(f64, f64); 4]) -> Result<PlacedBox, WarpError> {
    let min_x = corners
        .iter()
        .map(|corner| corner.0)
        .fold(f64::INFINITY, f64::min);
    let min_y = corners
        .iter()
        .map(|corner| corner.1)
        .fold(f64::INFINITY, f64::min);
    let max_x = corners
        .iter()
        .map(|corner| corner.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = corners
        .iter()
        .map(|corner| corner.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let x = snap_min(min_x).ok_or(WarpError::TooLarge)?;
    let y = snap_min(min_y).ok_or(WarpError::TooLarge)?;
    let width = span_u32(max_x - min_x).ok_or(WarpError::TooLarge)?;
    let height = span_u32(max_y - min_y).ok_or(WarpError::TooLarge)?;
    if !canvas_allowed(width, height) {
        return Err(WarpError::TooLarge);
    }
    Ok(PlacedBox {
        offset: (x, y),
        size: (width, height),
    })
}

fn snap_min(value: f64) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let nearest = value.round();
    let snapped = if (value - nearest).abs() < 1e-4 {
        nearest
    } else {
        value.floor()
    };
    as_i32(snapped)
}

fn span_u32(length: f64) -> Option<u32> {
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let nearest = length.round();
    let snapped = if (length - nearest).abs() < 1e-4 {
        nearest
    } else {
        length.ceil()
    };
    if !(1.0..=f64::from(MAX_NEW_EDGE)).contains(&snapped) {
        return None;
    }
    Some(snapped as u32)
}

fn as_i32(value: f64) -> Option<i32> {
    if value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return None;
    }
    Some(value as i32)
}

fn output_grid(bounds: (u32, u32), max_edge: Option<u32>) -> (u32, u32) {
    let Some(max_edge) = max_edge else {
        return bounds;
    };
    let long = bounds.0.max(bounds.1);
    if long <= max_edge || long == 0 {
        return bounds;
    }
    let scale = f64::from(max_edge) / f64::from(long);
    (
        (f64::from(bounds.0) * scale).round().max(1.0) as u32,
        (f64::from(bounds.1) * scale).round().max(1.0) as u32,
    )
}

fn lerp(start: (f64, f64), end: (f64, f64), t: f64) -> (f64, f64) {
    (
        start.0 + (end.0 - start.0) * t,
        start.1 + (end.1 - start.1) * t,
    )
}

fn invert_bilinear(corners: [(f64, f64); 4], point: (f64, f64)) -> Option<(f64, f64)> {
    let a = corners[0];
    let b = corners[1];
    let d = corners[3];
    let e = (b.0 - a.0, b.1 - a.1);
    let f = (d.0 - a.0, d.1 - a.1);
    let g = (
        a.0 - b.0 + corners[2].0 - d.0,
        a.1 - b.1 + corners[2].1 - d.1,
    );
    let h = (point.0 - a.0, point.1 - a.1);
    let k2 = cross(g, f);
    let k1 = cross(e, f) + cross(h, g);
    let k0 = cross(h, e);
    let roots = if k2.abs() < 1e-8 {
        if k1.abs() < 1e-8 {
            return None;
        }
        [-k0 / k1, f64::NAN]
    } else {
        let discriminant = k1 * k1 - 4.0 * k2 * k0;
        if discriminant < 0.0 {
            return None;
        }
        let root = discriminant.sqrt();
        [(-k1 - root) / (2.0 * k2), (-k1 + root) / (2.0 * k2)]
    };
    let mut best: Option<(f64, f64, f64)> = None;
    for v in roots {
        if !v.is_finite() {
            continue;
        }
        let v = v.clamp(-0.001, 1.001);
        let denom_x = e.0 + g.0 * v;
        let denom_y = e.1 + g.1 * v;
        let u = if denom_x.abs() >= denom_y.abs() {
            if denom_x.abs() < 1e-8 {
                continue;
            }
            (h.0 - f.0 * v) / denom_x
        } else {
            if denom_y.abs() < 1e-8 {
                continue;
            }
            (h.1 - f.1 * v) / denom_y
        };
        if !u.is_finite() {
            continue;
        }
        let u = u.clamp(-0.001, 1.001);
        let mapped = map_quad(corners, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
        let error = (mapped.0 - point.0).hypot(mapped.1 - point.1);
        if best.is_none_or(|(_, _, best_error)| error < best_error) {
            best = Some((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0), error));
        }
    }
    best.map(|(u, v, _)| (u, v))
}

fn cross(left: (f64, f64), right: (f64, f64)) -> f64 {
    left.0 * right.1 - left.1 * right.0
}

fn sample_pixel(pixels: &RgbaImage, x: f64, y: f64, resample: Resample) -> Rgba<u8> {
    if !x.is_finite() || !y.is_finite() {
        return Rgba([0, 0, 0, 0]);
    }
    match resample {
        Resample::Nearest => {
            let sx = x.round();
            let sy = y.round();
            pixel_at(pixels, sx as i64, sy as i64).unwrap_or(Rgba([0, 0, 0, 0]))
        }
        Resample::Bilinear => unpremultiply_sample(bilinear(pixels, x, y)),
        Resample::Bicubic => unpremultiply_sample(bicubic(pixels, x, y)),
    }
}

fn bilinear(pixels: &RgbaImage, x: f64, y: f64) -> [f64; 4] {
    let x0 = x.floor();
    let y0 = y.floor();
    let tx = x - x0;
    let ty = y - y0;
    let mut mixed = [0.0; 4];
    for (iy, wy) in [(y0, 1.0 - ty), (y0 + 1.0, ty)] {
        for (ix, wx) in [(x0, 1.0 - tx), (x0 + 1.0, tx)] {
            let weight = wx * wy;
            let sample = premul(pixel_at(pixels, ix as i64, iy as i64));
            for (slot, sample) in mixed.iter_mut().zip(sample) {
                *slot += sample * weight;
            }
        }
    }
    mixed
}

fn bicubic(pixels: &RgbaImage, x: f64, y: f64) -> [f64; 4] {
    let x1 = x.floor();
    let y1 = y.floor();
    let tx = x - x1;
    let ty = y - y1;
    let mut rows = [[0.0; 4]; 4];
    for (row, iy) in (-1..=2).enumerate() {
        let samples = [
            premul(pixel_at(pixels, (x1 - 1.0) as i64, (y1 + iy as f64) as i64)),
            premul(pixel_at(pixels, x1 as i64, (y1 + iy as f64) as i64)),
            premul(pixel_at(pixels, (x1 + 1.0) as i64, (y1 + iy as f64) as i64)),
            premul(pixel_at(pixels, (x1 + 2.0) as i64, (y1 + iy as f64) as i64)),
        ];
        for (slot, channel) in rows[row].iter_mut().zip(0..4) {
            *slot = catmull(
                [
                    samples[0][channel],
                    samples[1][channel],
                    samples[2][channel],
                    samples[3][channel],
                ],
                tx,
            );
        }
    }
    let mut mixed = [0.0; 4];
    for (slot, channel) in mixed.iter_mut().zip(0..4) {
        *slot = catmull(
            [
                rows[0][channel],
                rows[1][channel],
                rows[2][channel],
                rows[3][channel],
            ],
            ty,
        );
    }
    mixed
}

fn catmull(samples: [f64; 4], t: f64) -> f64 {
    let a = -0.5 * samples[0] + 1.5 * samples[1] - 1.5 * samples[2] + 0.5 * samples[3];
    let b = samples[0] - 2.5 * samples[1] + 2.0 * samples[2] - 0.5 * samples[3];
    let c = -0.5 * samples[0] + 0.5 * samples[2];
    ((a * t + b) * t + c) * t + samples[1]
}

fn pixel_at(pixels: &RgbaImage, x: i64, y: i64) -> Option<Rgba<u8>> {
    let x = u32::try_from(x).ok()?;
    let y = u32::try_from(y).ok()?;
    (x < pixels.width() && y < pixels.height()).then(|| *pixels.get_pixel(x, y))
}

fn premul(pixel: Option<Rgba<u8>>) -> [f64; 4] {
    let Some(pixel) = pixel else {
        return [0.0; 4];
    };
    let alpha = f64::from(pixel.0[3]) / 255.0;
    [
        f64::from(pixel.0[0]) / 255.0 * alpha,
        f64::from(pixel.0[1]) / 255.0 * alpha,
        f64::from(pixel.0[2]) / 255.0 * alpha,
        alpha,
    ]
}

fn unpremultiply_sample(sample: [f64; 4]) -> Rgba<u8> {
    let alpha = sample[3].clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let scale = 255.0 / alpha;
    let byte = |value: f64| (value.clamp(0.0, alpha) * scale).round() as u8;
    Rgba([
        byte(sample[0]),
        byte(sample[1]),
        byte(sample[2]),
        (alpha * 255.0).round() as u8,
    ])
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

    #[test]
    fn rotate_90_matches_a_quarter_turn_and_keeps_the_center() {
        let mut image = RgbaImage::new(3, 1);
        image.put_pixel(0, 0, Rgba([10, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([20, 0, 0, 255]));
        image.put_pixel(2, 0, Rgba([30, 0, 0, 255]));
        let placed =
            warp_layer(&image, (0, 0), Warp::Rotate(90.0), Resample::Nearest, None).unwrap();
        assert_eq!(placed.offset, (1, -1));
        assert_eq!(placed.bounds, (1, 3));
        assert_eq!(placed.pixels.dimensions(), (1, 3));
        assert_eq!(placed.pixels.get_pixel(0, 0).0, [10, 0, 0, 255]);
        assert_eq!(placed.pixels.get_pixel(0, 1).0, [20, 0, 0, 255]);
        assert_eq!(placed.pixels.get_pixel(0, 2).0, [30, 0, 0, 255]);
        let center = |offset: (i32, i32), size: (u32, u32)| {
            (
                offset.0 as f64 + size.0 as f64 / 2.0,
                offset.1 as f64 + size.1 as f64 / 2.0,
            )
        };
        assert_eq!(center(placed.offset, placed.bounds), center((0, 0), (3, 1)));
    }

    #[test]
    fn perspective_maps_corners_and_keeps_a_straight_shift() {
        let corners = [(0.0, 0.0), (4.0, 1.0), (3.0, 4.0), (-1.0, 2.0)];
        let center = map_quad(corners, 0.5, 0.5);
        let (u, v) = unmap_quad(corners, center).unwrap();
        assert!((u - 0.5).abs() < 1e-6 && (v - 0.5).abs() < 1e-6);
        for (index, (su, sv)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .into_iter()
            .enumerate()
        {
            let point = map_quad(corners, su, sv);
            let (got_u, got_v) = unmap_quad(corners, point).unwrap();
            assert!(
                (got_u - su).abs() < 1e-6 && (got_v - sv).abs() < 1e-6,
                "{index}"
            );
            assert!((point.0 - corners[index].0).abs() < 1e-9);
        }

        let mut image = RgbaImage::new(2, 1);
        image.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([0, 0, 0, 0]));
        let shifted = layer_corners((2, 0), (2, 1));
        let placed = warp_layer(
            &image,
            (0, 0),
            Warp::Perspective(shifted),
            Resample::Nearest,
            None,
        )
        .unwrap();
        assert_eq!(placed.offset, (2, 0));
        assert_eq!(placed.bounds, (2, 1));
        assert_eq!(placed.pixels.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(placed.pixels.get_pixel(1, 0).0, [0, 0, 0, 0]);

        let filtered = sample_pixel(&image, 0.5, 0.0, Resample::Bilinear);
        assert_eq!(filtered.0[3], 128, "{filtered:?}");
        assert_eq!(filtered.0[0], 255);
        assert_eq!(filtered.0[1], 0);
        let solid = RgbaImage::from_pixel(4, 4, Rgba([0, 40, 0, 255]));
        assert_eq!(
            sample_pixel(&solid, 1.0, 1.0, Resample::Bicubic).0,
            [0, 40, 0, 255]
        );
    }

    #[test]
    fn perspective_rejects_a_bowtie_a_flat_quad_and_an_oversized_quad() {
        let bowtie = [(0.0, 0.0), (4.0, 1.0), (1.0, 0.0), (0.0, 3.0)];
        assert_eq!(
            warp_bounds((0, 0), (2, 2), Warp::Perspective(bowtie)),
            Err(WarpError::Crossing)
        );
        let flat = [(0.0, 0.0), (4.0, 0.0), (3.0, 0.0), (1.0, 0.0)];
        assert_eq!(
            warp_bounds((0, 0), (2, 2), Warp::Perspective(flat)),
            Err(WarpError::Singular)
        );
        let huge = [
            (0.0, 0.0),
            (20_000.0, 0.0),
            (20_000.0, 20_000.0),
            (0.0, 20_000.0),
        ];
        assert_eq!(
            warp_bounds((0, 0), (2, 2), Warp::Perspective(huge)),
            Err(WarpError::TooLarge)
        );
        let flipped = warp_layer(
            &RgbaImage::from_pixel(2, 1, Rgba([1, 2, 3, 255])),
            (4, -2),
            Warp::Flip(Mirror::Horizontal),
            Resample::Nearest,
            None,
        )
        .unwrap();
        assert_eq!(flipped.offset, (4, -2));
        assert_eq!(flipped.bounds, (2, 1));
    }
}
