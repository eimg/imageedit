use std::sync::Arc;

use image::{Rgba, RgbaImage};

use crate::preview::PixelRect;

// A canvas-space selection. Unrestricted, empty, full, a rectangle, and its inverse stay
// allocation-free. A combined or feathered selection stores coverage inside the canvas.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Selection {
    // No mask: every layer pixel can be edited, including pixels that sit off the canvas.
    #[default]
    Unrestricted,
    // A mask that selects nothing. Writes do not change pixels.
    Empty,
    // Every pixel of the canvas, and none of the off-canvas layer pixels.
    Full,
    Rect(PixelRect),
    // The canvas except this rectangle.
    Inverse(PixelRect),
    Mask(Mask),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionOp {
    Replace,
    Add,
    Subtract,
    Intersect,
}

// Coverage is 0 or 255 for a hard mask. Feather stores the values between.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

impl Selection {
    pub fn rectangle(start: (u32, u32), end: (u32, u32), width: u32, height: u32) -> Self {
        let left = i64::from(start.0.min(end.0));
        let top = i64::from(start.1.min(end.1));
        let right = i64::from(start.0.max(end.0)) + 1;
        let bottom = i64::from(start.1.max(end.1)) + 1;
        Self::from_rect(
            PixelRect::new(left, top, (right - left) as u32, (bottom - top) as u32),
            width,
            height,
        )
    }

    pub fn invert(self, width: u32, height: u32) -> Self {
        match self {
            Self::Unrestricted | Self::Full => Self::Empty,
            Self::Empty => Self::Full,
            Self::Rect(rect) => {
                if covers_canvas(rect, width, height) {
                    Self::Empty
                } else {
                    Self::Inverse(rect)
                }
            }
            Self::Inverse(hole) => Self::from_rect(hole, width, height),
            Self::Mask(mask) => {
                if mask.is_solid() {
                    return Self::Rect(mask.bounds()).invert(width, height);
                }
                invert_mask(&mask, width, height)
            }
        }
    }

    pub fn covers(&self, x: i64, y: i64, width: u32, height: u32) -> bool {
        let inside = x >= 0 && y >= 0 && x < i64::from(width) && y < i64::from(height);
        match self {
            Self::Unrestricted => true,
            Self::Empty => false,
            Self::Full => inside,
            Self::Rect(rect) => inside && contains(*rect, x, y),
            Self::Inverse(hole) => inside && !contains(*hole, x, y),
            Self::Mask(mask) => inside && mask.coverage(x as i32, y as i32) > 0,
        }
    }

    pub fn coverage(&self, x: i64, y: i64, width: u32, height: u32) -> u8 {
        if !self.covers(x, y, width, height) {
            return 0;
        }
        match self {
            Self::Mask(mask) => mask.coverage(x as i32, y as i32),
            _ => 255,
        }
    }

    // The crop command uses this only when the selected pixels have a smaller bounding box
    // than the canvas. Unrestricted and empty selections have no crop.
    pub fn crop_bounds(&self, width: u32, height: u32) -> Option<PixelRect> {
        let bounds = self.bounds(width, height)?;
        (bounds.width < width || bounds.height < height).then_some(bounds)
    }

    // Moves a rectangle or its hole onto a new canvas. Full and unrestricted stay as they are.
    pub fn moved_box(self, rect: PixelRect, width: u32, height: u32) -> Self {
        match self {
            Self::Unrestricted | Self::Empty => self,
            Self::Full => Self::Full,
            Self::Rect(_) => Self::from_rect(rect, width, height),
            Self::Inverse(_) => match clip(rect, width, height) {
                Some(hole) if covers_canvas(hole, width, height) => Self::Empty,
                Some(hole) => Self::Inverse(hole),
                None => Self::Full,
            },
            Self::Mask(_) => self,
        }
    }

    pub fn rebase(self, origin_x: i64, origin_y: i64, width: u32, height: u32) -> Self {
        match self {
            Self::Unrestricted | Self::Empty => self,
            Self::Full => Self::Full,
            Self::Rect(rect) => {
                Self::from_rect(translate(rect, -origin_x, -origin_y), width, height)
            }
            Self::Inverse(hole) => {
                let hole = translate(hole, -origin_x, -origin_y);
                match clip(hole, width, height) {
                    Some(hole) if covers_canvas(hole, width, height) => Self::Empty,
                    Some(hole) => Self::Inverse(hole),
                    None => Self::Full,
                }
            }
            Self::Mask(mask) => mask.translated(-origin_x, -origin_y, width, height),
        }
    }

    pub fn contours(&self, width: u32, height: u32) -> Vec<PixelRect> {
        match self {
            Self::Unrestricted | Self::Empty => Vec::new(),
            Self::Full => vec![PixelRect::new(0, 0, width, height)],
            Self::Rect(rect) => vec![*rect],
            Self::Inverse(hole) => vec![PixelRect::new(0, 0, width, height), *hole],
            Self::Mask(_) => Vec::new(),
        }
    }

    // Boundary segments of a dense mask, in canvas pixels. Each segment is (x0, y0, x1, y1).
    pub fn outline_edges(&self) -> Vec<(i32, i32, i32, i32)> {
        match self {
            Self::Mask(mask) => mask.outline(),
            _ => Vec::new(),
        }
    }

    fn from_rect(rect: PixelRect, width: u32, height: u32) -> Self {
        match clip(rect, width, height) {
            Some(rect) if covers_canvas(rect, width, height) => Self::Full,
            Some(rect) => Self::Rect(rect),
            None => Self::Empty,
        }
    }

    fn bounds(&self, width: u32, height: u32) -> Option<PixelRect> {
        match self {
            Self::Unrestricted | Self::Empty => None,
            Self::Full => Some(PixelRect::new(0, 0, width, height)),
            Self::Rect(rect) => clip(*rect, width, height),
            Self::Inverse(hole) => inverse_bounds(*hole, width, height),
            Self::Mask(mask) => clip(mask.bounds(), width, height),
        }
    }

    pub fn combine(self, incoming: Self, op: SelectionOp, width: u32, height: u32) -> Self {
        match op {
            SelectionOp::Replace => incoming,
            SelectionOp::Add => match self {
                Self::Unrestricted | Self::Empty => incoming,
                Self::Full => Self::Full,
                current => match incoming {
                    Self::Empty | Self::Unrestricted => current,
                    Self::Full => Self::Full,
                    incoming => raster_combine(current, incoming, op, width, height),
                },
            },
            SelectionOp::Subtract => match self {
                Self::Empty => Self::Empty,
                Self::Unrestricted => incoming.invert(width, height),
                current => match incoming {
                    Self::Empty | Self::Unrestricted => current,
                    incoming => raster_combine(current, incoming, op, width, height),
                },
            },
            SelectionOp::Intersect => match self {
                Self::Empty => Self::Empty,
                Self::Unrestricted | Self::Full => incoming,
                current => match incoming {
                    Self::Empty => Self::Empty,
                    Self::Unrestricted | Self::Full => current,
                    incoming => raster_combine(current, incoming, op, width, height),
                },
            },
        }
    }

    pub fn polygon(points: &[(i32, i32)], width: u32, height: u32) -> Self {
        polygon_selection(points, width, height)
    }

    pub fn flood(
        image: &RgbaImage,
        x: u32,
        y: u32,
        tolerance: u8,
        offset: (i32, i32),
        canvas: (u32, u32),
    ) -> Self {
        flood_selection(image, x, y, tolerance, offset, canvas)
    }

    pub fn grow(self, radius: u32, width: u32, height: u32) -> Self {
        reshape(self, radius, width, height, Reshape::Grow)
    }

    pub fn shrink(self, radius: u32, width: u32, height: u32) -> Self {
        reshape(self, radius, width, height, Reshape::Shrink)
    }

    pub fn feather(self, radius: u32, width: u32, height: u32) -> Self {
        reshape(self, radius, width, height, Reshape::Feather)
    }

    // Moves each covered pixel. Turns and flips use this; scaling samples separately.
    pub fn map_mask(self, width: u32, height: u32, map: impl Fn(i32, i32) -> (i32, i32)) -> Self {
        let Self::Mask(mask) = self else {
            return self;
        };
        let mut points = Vec::new();
        for y in 0..mask.height {
            for x in 0..mask.width {
                let coverage = mask.pixels[(y * mask.width + x) as usize];
                if coverage == 0 {
                    continue;
                }
                let (mx, my) = map(mask.x + x as i32, mask.y + y as i32);
                points.push((mx, my, coverage));
            }
        }
        selection_from_points(&points, width, height)
    }

    pub fn scale_mask(self, old: (u32, u32), new: (u32, u32)) -> Self {
        let Self::Mask(mask) = self else {
            return self;
        };
        if old.0 == 0 || old.1 == 0 || new.0 == 0 || new.1 == 0 {
            return Self::Empty;
        }
        let (x, width) = scaled_span(i64::from(mask.x), mask.width, old.0, new.0);
        let (y, height) = scaled_span(i64::from(mask.y), mask.height, old.1, new.1);
        if width == 0 || height == 0 {
            return Self::Empty;
        }
        let mut pixels = vec![0u8; width as usize * height as usize];
        for row in 0..height {
            for column in 0..width {
                let source_x = mask.x
                    + ((i64::from(column) * i64::from(mask.width)) / i64::from(width)) as i32;
                let source_y =
                    mask.y + ((i64::from(row) * i64::from(mask.height)) / i64::from(height)) as i32;
                pixels[(row * width + column) as usize] = mask.coverage(source_x, source_y);
            }
        }
        finish_mask(x, y, width, height, pixels, new.0, new.1)
    }
}

impl Mask {
    fn coverage(&self, x: i32, y: i32) -> u8 {
        let Some(local_x) = x.checked_sub(self.x) else {
            return 0;
        };
        let Some(local_y) = y.checked_sub(self.y) else {
            return 0;
        };
        if local_x < 0
            || local_y < 0
            || local_x as u32 >= self.width
            || local_y as u32 >= self.height
        {
            return 0;
        }
        self.pixels[(local_y as u32 * self.width + local_x as u32) as usize]
    }

    fn bounds(&self) -> PixelRect {
        PixelRect::new(
            i64::from(self.x),
            i64::from(self.y),
            self.width,
            self.height,
        )
    }

    fn is_solid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.pixels.iter().all(|pixel| *pixel == 255)
    }

    fn translated(self, dx: i64, dy: i64, width: u32, height: u32) -> Selection {
        let Some(x) = i64::from(self.x).checked_add(dx) else {
            return Selection::Empty;
        };
        let Some(y) = i64::from(self.y).checked_add(dy) else {
            return Selection::Empty;
        };
        let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
            return Selection::Empty;
        };
        finish_mask(
            x,
            y,
            self.width,
            self.height,
            self.pixels.to_vec(),
            width,
            height,
        )
    }

    fn outline(&self) -> Vec<(i32, i32, i32, i32)> {
        let mut edges = Vec::new();
        let selected = |x: i32, y: i32| self.coverage(x, y) >= 128;
        for y in 0..self.height as i32 {
            let canvas_y = self.y + y;
            coalesce(self.x, self.width, |x| {
                let canvas_x = self.x + x;
                selected(canvas_x, canvas_y) && !selected(canvas_x, canvas_y - 1)
            })
            .into_iter()
            .for_each(|(start, end)| edges.push((start, canvas_y, end, canvas_y)));
            coalesce(self.x, self.width, |x| {
                let canvas_x = self.x + x;
                selected(canvas_x, canvas_y) && !selected(canvas_x, canvas_y + 1)
            })
            .into_iter()
            .for_each(|(start, end)| edges.push((start, canvas_y + 1, end, canvas_y + 1)));
        }
        for x in 0..self.width as i32 {
            let canvas_x = self.x + x;
            coalesce(self.y, self.height, |y| {
                let canvas_y = self.y + y;
                selected(canvas_x, canvas_y) && !selected(canvas_x - 1, canvas_y)
            })
            .into_iter()
            .for_each(|(start, end)| edges.push((canvas_x, start, canvas_x, end)));
            coalesce(self.y, self.height, |y| {
                let canvas_y = self.y + y;
                selected(canvas_x, canvas_y) && !selected(canvas_x + 1, canvas_y)
            })
            .into_iter()
            .for_each(|(start, end)| edges.push((canvas_x + 1, start, canvas_x + 1, end)));
        }
        edges
    }
}

fn coalesce(origin: i32, length: u32, mut on: impl FnMut(i32) -> bool) -> Vec<(i32, i32)> {
    let mut runs = Vec::new();
    let mut index = 0i32;
    while index < length as i32 {
        if !on(index) {
            index += 1;
            continue;
        }
        let start = index;
        while index < length as i32 && on(index) {
            index += 1;
        }
        runs.push((origin + start, origin + index));
    }
    runs
}

fn translate(rect: PixelRect, dx: i64, dy: i64) -> PixelRect {
    PixelRect::new(rect.x + dx, rect.y + dy, rect.width, rect.height)
}

fn contains(rect: PixelRect, x: i64, y: i64) -> bool {
    x >= rect.x && y >= rect.y && x < rect.right() && y < rect.bottom()
}

fn covers_canvas(rect: PixelRect, width: u32, height: u32) -> bool {
    rect.x <= 0
        && rect.y <= 0
        && rect.right() >= i64::from(width)
        && rect.bottom() >= i64::from(height)
}

fn clip(rect: PixelRect, width: u32, height: u32) -> Option<PixelRect> {
    let right = rect.right().clamp(0, i64::from(width));
    let bottom = rect.bottom().clamp(0, i64::from(height));
    let x = rect.x.clamp(0, i64::from(width));
    let y = rect.y.clamp(0, i64::from(height));
    (x < right && y < bottom).then_some(PixelRect::new(
        x,
        y,
        (right - x) as u32,
        (bottom - y) as u32,
    ))
}

// The complement of a rectangle is the whole canvas unless that rectangle is a full-width
// or full-height band. A band leaves the rows or columns outside it.
fn inverse_bounds(hole: PixelRect, width: u32, height: u32) -> Option<PixelRect> {
    let Some(hole) = clip(hole, width, height) else {
        return Some(PixelRect::new(0, 0, width, height));
    };
    if covers_canvas(hole, width, height) {
        return None;
    }
    let spans_width = hole.x == 0 && hole.right() >= i64::from(width);
    let spans_height = hole.y == 0 && hole.bottom() >= i64::from(height);
    // A band in the middle leaves selected pixels on both sides, so the box is the whole canvas.
    let (x, right) = if spans_height && hole.x == 0 {
        (hole.right(), i64::from(width))
    } else if spans_height && hole.right() >= i64::from(width) {
        (0, hole.x)
    } else {
        (0, i64::from(width))
    };
    let (y, bottom) = if spans_width && hole.y == 0 {
        (hole.bottom(), i64::from(height))
    } else if spans_width && hole.bottom() >= i64::from(height) {
        (0, hole.y)
    } else {
        (0, i64::from(height))
    };
    (x < right && y < bottom).then_some(PixelRect::new(
        x,
        y,
        (right - x) as u32,
        (bottom - y) as u32,
    ))
}

enum Reshape {
    Grow,
    Shrink,
    Feather,
}

fn raster_combine(
    current: Selection,
    incoming: Selection,
    op: SelectionOp,
    width: u32,
    height: u32,
) -> Selection {
    let Some(bounds) = combine_bounds(&current, &incoming, op, width, height) else {
        return Selection::Empty;
    };
    let mut pixels = vec![0u8; bounds.width as usize * bounds.height as usize];
    for row in 0..bounds.height {
        for column in 0..bounds.width {
            let x = bounds.x as i32 + column as i32;
            let y = bounds.y as i32 + row as i32;
            let left = current.coverage(i64::from(x), i64::from(y), width, height);
            let right = incoming.coverage(i64::from(x), i64::from(y), width, height);
            pixels[(row * bounds.width + column) as usize] = match op {
                SelectionOp::Add => left.max(right),
                SelectionOp::Intersect => left.min(right),
                SelectionOp::Subtract => ((u16::from(left) * u16::from(255 - right)) / 255) as u8,
                SelectionOp::Replace => right,
            };
        }
    }
    finish_mask(
        bounds.x as i32,
        bounds.y as i32,
        bounds.width,
        bounds.height,
        pixels,
        width,
        height,
    )
}

fn combine_bounds(
    current: &Selection,
    incoming: &Selection,
    op: SelectionOp,
    width: u32,
    height: u32,
) -> Option<PixelRect> {
    let left = current.bounds(width, height);
    let right = incoming.bounds(width, height);
    match op {
        SelectionOp::Replace => right,
        SelectionOp::Add => match (left, right) {
            (Some(left), Some(right)) => Some(union_rect(left, right)),
            (Some(bounds), None) | (None, Some(bounds)) => Some(bounds),
            (None, None) => None,
        },
        SelectionOp::Subtract => left,
        SelectionOp::Intersect => match (left, right) {
            (Some(left), Some(right)) => intersect_rect(left, right),
            _ => None,
        },
    }
}

fn intersect_rect(left: PixelRect, right: PixelRect) -> Option<PixelRect> {
    let x = left.x.max(right.x);
    let y = left.y.max(right.y);
    let right_edge = left.right().min(right.right());
    let bottom = left.bottom().min(right.bottom());
    (x < right_edge && y < bottom).then_some(PixelRect::new(
        x,
        y,
        (right_edge - x) as u32,
        (bottom - y) as u32,
    ))
}

fn union_rect(left: PixelRect, right: PixelRect) -> PixelRect {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    let right_edge = left.right().max(right.right());
    let bottom = left.bottom().max(right.bottom());
    PixelRect::new(x, y, (right_edge - x) as u32, (bottom - y) as u32)
}

fn polygon_selection(points: &[(i32, i32)], width: u32, height: u32) -> Selection {
    if points.len() < 3 || width == 0 || height == 0 {
        return Selection::Empty;
    }
    let vertices: Vec<(f64, f64)> = points
        .iter()
        .map(|(x, y)| (f64::from(*x) + 0.5, f64::from(*y) + 0.5))
        .collect();
    let min_x = points.iter().map(|point| point.0).min().unwrap_or(0).max(0);
    let max_x = points
        .iter()
        .map(|point| point.0)
        .max()
        .unwrap_or(0)
        .min(width as i32 - 1);
    let min_y = points.iter().map(|point| point.1).min().unwrap_or(0).max(0);
    let max_y = points
        .iter()
        .map(|point| point.1)
        .max()
        .unwrap_or(0)
        .min(height as i32 - 1);
    if min_x > max_x || min_y > max_y {
        return Selection::Empty;
    }
    let buf_w = (max_x - min_x + 1) as u32;
    let buf_h = (max_y - min_y + 1) as u32;
    let mut pixels = vec![0u8; buf_w as usize * buf_h as usize];
    let mut edges = Vec::new();
    for index in 0..vertices.len() {
        let (x0, y0) = vertices[index];
        let (x1, y1) = vertices[(index + 1) % vertices.len()];
        if (y1 - y0).abs() < 1e-9 {
            continue;
        }
        let dir = if y1 > y0 { 1 } else { -1 };
        if y0 < y1 {
            edges.push((y0, y1, x0, x1, dir));
        } else {
            edges.push((y1, y0, x1, x0, dir));
        }
    }
    for y in min_y..=max_y {
        let scan = f64::from(y) + 0.5;
        let mut hits = Vec::new();
        for &(y0, y1, x0, x1, dir) in &edges {
            if scan >= y0 && scan < y1 {
                let t = (scan - y0) / (y1 - y0);
                hits.push((x0 + t * (x1 - x0), dir));
            }
        }
        hits.sort_by(|left, right| {
            left.0
                .partial_cmp(&right.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut wind = 0;
        let mut from = None;
        for (x, dir) in hits {
            let next = wind + dir;
            if wind == 0 && next != 0 {
                from = Some(x);
            } else if wind != 0
                && next == 0
                && let Some(left) = from.take()
            {
                fill_span(&mut pixels, min_x, min_y, buf_w, y, left, x);
            }
            wind = next;
        }
    }
    finish_mask(min_x, min_y, buf_w, buf_h, pixels, width, height)
}

fn fill_span(
    pixels: &mut [u8],
    origin_x: i32,
    origin_y: i32,
    width: u32,
    y: i32,
    left: f64,
    right: f64,
) {
    let start = (left - 0.5).ceil() as i32;
    let end = (right - 0.5).ceil() as i32;
    let row = (y - origin_y) as u32;
    for x in start..end {
        if x < origin_x {
            continue;
        }
        let column = (x - origin_x) as u32;
        if column >= width {
            break;
        }
        pixels[(row * width + column) as usize] = 255;
    }
}

fn flood_selection(
    image: &RgbaImage,
    x: u32,
    y: u32,
    tolerance: u8,
    offset: (i32, i32),
    canvas: (u32, u32),
) -> Selection {
    if x >= image.width() || y >= image.height() {
        return Selection::Empty;
    }
    let target = *image.get_pixel(x, y);
    let (width, height) = image.dimensions();
    let mut seen = vec![false; width as usize * height as usize];
    let mut stack = vec![(x, y)];
    seen[(y * width + x) as usize] = true;
    let mut points = Vec::new();
    while let Some((px, py)) = stack.pop() {
        if !within_tolerance(*image.get_pixel(px, py), target, tolerance) {
            continue;
        }
        let canvas_x = offset.0 + px as i32;
        let canvas_y = offset.1 + py as i32;
        if canvas_x >= 0
            && canvas_y >= 0
            && (canvas_x as u32) < canvas.0
            && (canvas_y as u32) < canvas.1
        {
            points.push((canvas_x, canvas_y, 255));
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next_x = px as i32 + dx;
            let next_y = py as i32 + dy;
            if next_x < 0 || next_y < 0 || next_x as u32 >= width || next_y as u32 >= height {
                continue;
            }
            let index = (next_y as u32 * width + next_x as u32) as usize;
            if seen[index] {
                continue;
            }
            seen[index] = true;
            stack.push((next_x as u32, next_y as u32));
        }
    }
    selection_from_points(&points, canvas.0, canvas.1)
}

pub(crate) fn within_tolerance(pixel: Rgba<u8>, target: Rgba<u8>, tolerance: u8) -> bool {
    pixel
        .0
        .iter()
        .zip(target.0)
        .all(|(channel, expected)| channel.abs_diff(expected) <= tolerance)
}

fn reshape(selection: Selection, radius: u32, width: u32, height: u32, kind: Reshape) -> Selection {
    if radius == 0 {
        return selection;
    }
    if matches!(selection, Selection::Unrestricted | Selection::Empty) {
        return selection;
    }
    if matches!(selection, Selection::Full) && matches!(kind, Reshape::Grow) {
        return selection;
    }
    let Some(bounds) = selection.bounds(width, height) else {
        return selection;
    };
    let pad = radius as i32;
    let left = (bounds.x as i32 - pad).max(0);
    let top = (bounds.y as i32 - pad).max(0);
    let right = (bounds.right() as i32 + pad).min(width as i32);
    let bottom = (bounds.bottom() as i32 + pad).min(height as i32);
    if left >= right || top >= bottom {
        return Selection::Empty;
    }
    let buf_w = (right - left) as u32;
    let buf_h = (bottom - top) as u32;
    let mut source = vec![0u8; buf_w as usize * buf_h as usize];
    for row in 0..buf_h {
        for column in 0..buf_w {
            let coverage = selection.coverage(
                i64::from(left + column as i32),
                i64::from(top + row as i32),
                width,
                height,
            );
            source[(row * buf_w + column) as usize] = match kind {
                Reshape::Feather => coverage,
                Reshape::Grow | Reshape::Shrink => u8::from(coverage > 0),
            };
        }
    }
    let sums = integral(&source, buf_w, buf_h);
    let area = u64::from(radius * 2 + 1) * u64::from(radius * 2 + 1);
    let mut output = vec![0u8; source.len()];
    for row in 0..buf_h as i32 {
        for column in 0..buf_w as i32 {
            let sum = window_sum(&sums, buf_w, column, row, pad, buf_w as i32, buf_h as i32);
            output[(row as u32 * buf_w + column as u32) as usize] = match kind {
                Reshape::Grow => u8::from(sum > 0) * 255,
                Reshape::Shrink => u8::from(sum == area) * 255,
                Reshape::Feather => ((sum + area / 2) / area).min(255) as u8,
            };
        }
    }
    finish_mask(left, top, buf_w, buf_h, output, width, height)
}

fn integral(grid: &[u8], width: u32, height: u32) -> Vec<u64> {
    let width = width as usize;
    let stride = width + 1;
    let mut sums = vec![0u64; stride * (height as usize + 1)];
    for y in 0..height as usize {
        for x in 0..width {
            let value = u64::from(grid[y * width + x]);
            sums[(y + 1) * stride + x + 1] =
                value + sums[y * stride + x + 1] + sums[(y + 1) * stride + x]
                    - sums[y * stride + x];
        }
    }
    sums
}

fn window_sum(
    sums: &[u64],
    width: u32,
    x: i32,
    y: i32,
    radius: i32,
    buf_w: i32,
    buf_h: i32,
) -> u64 {
    let stride = width as usize + 1;
    let x0 = x.saturating_sub(radius).clamp(0, buf_w) as usize;
    let y0 = y.saturating_sub(radius).clamp(0, buf_h) as usize;
    let x1 = (x + radius + 1).clamp(0, buf_w) as usize;
    let y1 = (y + radius + 1).clamp(0, buf_h) as usize;
    if x0 >= x1 || y0 >= y1 {
        return 0;
    }
    sums[y1 * stride + x1] + sums[y0 * stride + x0]
        - sums[y0 * stride + x1]
        - sums[y1 * stride + x0]
}

fn selection_from_points(points: &[(i32, i32, u8)], width: u32, height: u32) -> Selection {
    let inside: Vec<_> = points
        .iter()
        .copied()
        .filter(|(x, y, coverage)| {
            *coverage > 0 && *x >= 0 && *y >= 0 && (*x as u32) < width && (*y as u32) < height
        })
        .collect();
    if inside.is_empty() {
        return Selection::Empty;
    }
    let min_x = inside.iter().map(|point| point.0).min().unwrap();
    let min_y = inside.iter().map(|point| point.1).min().unwrap();
    let max_x = inside.iter().map(|point| point.0).max().unwrap();
    let max_y = inside.iter().map(|point| point.1).max().unwrap();
    let buf_w = (max_x - min_x + 1) as u32;
    let buf_h = (max_y - min_y + 1) as u32;
    let mut pixels = vec![0u8; buf_w as usize * buf_h as usize];
    for (x, y, coverage) in inside {
        let index = ((y - min_y) as u32 * buf_w + (x - min_x) as u32) as usize;
        pixels[index] = pixels[index].max(coverage);
    }
    finish_mask(min_x, min_y, buf_w, buf_h, pixels, width, height)
}

fn finish_mask(
    mut x: i32,
    mut y: i32,
    mut width: u32,
    mut height: u32,
    mut pixels: Vec<u8>,
    canvas_w: u32,
    canvas_h: u32,
) -> Selection {
    if width == 0 || height == 0 || canvas_w == 0 || canvas_h == 0 {
        return Selection::Empty;
    }
    if x < 0 {
        let skip = (-x) as u32;
        if skip >= width {
            return Selection::Empty;
        }
        pixels = crop_buffer(&pixels, width, height, skip, 0, width - skip, height);
        width -= skip;
        x = 0;
    }
    if y < 0 {
        let skip = (-y) as u32;
        if skip >= height {
            return Selection::Empty;
        }
        pixels = crop_buffer(&pixels, width, height, 0, skip, width, height - skip);
        height -= skip;
        y = 0;
    }
    if x as u32 >= canvas_w || y as u32 >= canvas_h {
        return Selection::Empty;
    }
    let keep_w = width.min(canvas_w - x as u32);
    let keep_h = height.min(canvas_h - y as u32);
    if keep_w != width || keep_h != height {
        pixels = crop_buffer(&pixels, width, height, 0, 0, keep_w, keep_h);
        width = keep_w;
        height = keep_h;
    }
    let (left, top, right, bottom) = opaque_bounds(&pixels, width, height);
    if left >= right || top >= bottom {
        return Selection::Empty;
    }
    if left != 0 || top != 0 || right != width || bottom != height {
        pixels = crop_buffer(
            &pixels,
            width,
            height,
            left,
            top,
            right - left,
            bottom - top,
        );
        x += left as i32;
        y += top as i32;
        width = right - left;
        height = bottom - top;
    }
    if pixels.iter().all(|pixel| *pixel == 255) {
        return Selection::from_rect(
            PixelRect::new(i64::from(x), i64::from(y), width, height),
            canvas_w,
            canvas_h,
        );
    }
    Selection::Mask(Mask {
        x,
        y,
        width,
        height,
        pixels: pixels.into(),
    })
}

fn opaque_bounds(pixels: &[u8], width: u32, height: u32) -> (u32, u32, u32, u32) {
    let mut left = width;
    let mut top = height;
    let mut right = 0;
    let mut bottom = 0;
    for y in 0..height {
        for x in 0..width {
            if pixels[(y * width + x) as usize] == 0 {
                continue;
            }
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    (left, top, right, bottom)
}

fn crop_buffer(
    pixels: &[u8],
    width: u32,
    _height: u32,
    x: u32,
    y: u32,
    crop_w: u32,
    crop_h: u32,
) -> Vec<u8> {
    let mut cropped = vec![0u8; crop_w as usize * crop_h as usize];
    for row in 0..crop_h {
        let start = ((y + row) * width + x) as usize;
        let end = start + crop_w as usize;
        cropped[(row * crop_w) as usize..(row * crop_w + crop_w) as usize]
            .copy_from_slice(&pixels[start..end]);
    }
    cropped
}

fn scaled_span(start: i64, length: u32, old: u32, new: u32) -> (i32, u32) {
    let end = start.saturating_add(i64::from(length));
    let map = |value: i64| value.saturating_mul(i64::from(new)) / i64::from(old);
    let mapped = map(start);
    let width = map(end).saturating_sub(mapped).max(0);
    (
        i32::try_from(mapped).unwrap_or(if mapped < 0 { i32::MIN } else { i32::MAX }),
        u32::try_from(width).unwrap_or(u32::MAX),
    )
}

fn invert_mask(mask: &Mask, width: u32, height: u32) -> Selection {
    let mut pixels = vec![255u8; width as usize * height as usize];
    for y in 0..height {
        for x in 0..width {
            let coverage = mask.coverage(x as i32, y as i32);
            pixels[(y * width + x) as usize] = 255 - coverage;
        }
    }
    finish_mask(0, 0, width, height, pixels, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_clips_to_the_canvas_and_collapses_when_it_covers_it() {
        assert_eq!(
            Selection::rectangle((1, 2), (4, 6), 10, 10),
            Selection::Rect(PixelRect::new(1, 2, 4, 5))
        );
        assert_eq!(Selection::rectangle((0, 0), (9, 9), 10, 8), Selection::Full);
    }

    #[test]
    fn invert_uses_the_empty_and_full_cases() {
        assert_eq!(Selection::Unrestricted.invert(4, 4), Selection::Empty);
        assert_eq!(Selection::Empty.invert(4, 4), Selection::Full);
        assert_eq!(Selection::Full.invert(4, 4), Selection::Empty);
        let rect = Selection::Rect(PixelRect::new(1, 1, 2, 2));
        assert_eq!(
            rect.clone().invert(8, 8),
            Selection::Inverse(PixelRect::new(1, 1, 2, 2))
        );
        assert_eq!(rect.clone().invert(8, 8).invert(8, 8), rect);
    }

    #[test]
    fn coverage_stops_at_the_rectangle_and_allows_every_pixel_when_unrestricted() {
        let rect = Selection::Rect(PixelRect::new(2, 1, 2, 1));
        assert!(rect.covers(2, 1, 6, 4));
        assert!(!rect.covers(4, 1, 6, 4));
        assert!(!rect.covers(-1, 0, 6, 4));
        assert!(Selection::Unrestricted.covers(-4, 9, 6, 4));
        assert!(!Selection::Full.covers(-1, 0, 6, 4));
        assert!(Selection::Full.covers(0, 0, 6, 4));
        assert!(!Selection::Empty.covers(0, 0, 6, 4));
        let inverse = Selection::Inverse(PixelRect::new(2, 1, 2, 1));
        assert!(!inverse.covers(2, 1, 6, 4));
        assert!(inverse.covers(0, 0, 6, 4));
    }

    #[test]
    fn crop_bounds_reject_unrestricted_empty_and_the_whole_canvas() {
        assert_eq!(Selection::Unrestricted.crop_bounds(8, 8), None);
        assert_eq!(Selection::Empty.crop_bounds(8, 8), None);
        assert_eq!(Selection::Full.crop_bounds(8, 8), None);
        assert_eq!(
            Selection::Rect(PixelRect::new(1, 2, 3, 4)).crop_bounds(8, 8),
            Some(PixelRect::new(1, 2, 3, 4))
        );
        assert_eq!(
            Selection::Inverse(PixelRect::new(0, 0, 8, 3)).crop_bounds(8, 8),
            Some(PixelRect::new(0, 3, 8, 5))
        );
        assert_eq!(
            Selection::Inverse(PixelRect::new(2, 0, 2, 8)).crop_bounds(8, 8),
            None
        );
    }

    #[test]
    fn crop_rebases_a_rectangle_and_drops_it_when_the_crop_misses_it() {
        let rect = Selection::Rect(PixelRect::new(4, 2, 3, 2));
        assert_eq!(rect.clone().rebase(4, 2, 3, 2), Selection::Full);
        assert_eq!(
            rect.clone().rebase(4, 1, 3, 2),
            Selection::Rect(PixelRect::new(0, 1, 3, 1))
        );
        assert_eq!(rect.rebase(0, 0, 4, 2), Selection::Empty);
        assert_eq!(
            Selection::Inverse(PixelRect::new(1, 1, 2, 2)).rebase(4, 0, 4, 4),
            Selection::Full
        );
    }

    #[test]
    fn polygon_winding_agrees_in_both_directions_and_clips_to_the_canvas() {
        let clockwise = [(1, 1), (5, 1), (1, 5)];
        let mut counter = clockwise;
        counter.reverse();
        let left = Selection::polygon(&clockwise, 8, 8);
        let right = Selection::polygon(&counter, 8, 8);
        assert!(left.covers(2, 2, 8, 8));
        assert!(!left.covers(4, 4, 8, 8));
        assert_eq!(left.covers(2, 2, 8, 8), right.covers(2, 2, 8, 8));
        assert_eq!(left.covers(4, 4, 8, 8), right.covers(4, 4, 8, 8));
        let clipped = Selection::polygon(&[(-2, 1), (2, 1), (2, -2)], 4, 4);
        assert!(clipped.covers(1, 0, 4, 4));
        assert!(!clipped.covers(3, 3, 4, 4));
    }

    #[test]
    fn flood_ignores_corner_touches_and_honors_tolerance_endpoints() {
        let mut image = RgbaImage::new(2, 2);
        image.put_pixel(0, 0, Rgba([9, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
        image.put_pixel(0, 1, Rgba([0, 0, 0, 255]));
        image.put_pixel(1, 1, Rgba([9, 0, 0, 255]));
        let region = Selection::flood(&image, 0, 0, 0, (0, 0), (2, 2));
        assert!(region.covers(0, 0, 2, 2));
        assert!(!region.covers(1, 1, 2, 2));

        let mut row = RgbaImage::new(3, 1);
        row.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        row.put_pixel(1, 0, Rgba([0, 0, 0, 254]));
        row.put_pixel(2, 0, Rgba([10, 0, 0, 255]));
        let exact = Selection::flood(&row, 0, 0, 0, (1, 2), (6, 4));
        assert!(exact.covers(1, 2, 6, 4));
        assert!(!exact.covers(2, 2, 6, 4));
        let alpha = Selection::flood(&row, 0, 0, 1, (0, 0), (3, 1));
        assert!(alpha.covers(1, 0, 3, 1));
        assert!(!alpha.covers(2, 0, 3, 1));
        let wide = Selection::flood(&row, 0, 0, 10, (0, 0), (3, 1));
        assert!(wide.covers(2, 0, 3, 1));
        assert!(!Selection::flood(&row, 0, 0, 0, (-1, 0), (2, 1)).covers(0, 0, 2, 1));
    }

    #[test]
    fn mask_combination_and_morphology_stay_inside_the_canvas() {
        let left = Selection::Rect(PixelRect::new(0, 0, 2, 2));
        let right = Selection::Rect(PixelRect::new(1, 0, 2, 1));
        let added = left.clone().combine(right.clone(), SelectionOp::Add, 4, 3);
        assert!(added.covers(0, 1, 4, 3));
        assert!(added.covers(2, 0, 4, 3));
        assert!(!added.covers(3, 0, 4, 3));
        let removed = left
            .clone()
            .combine(right.clone(), SelectionOp::Subtract, 4, 3);
        assert!(removed.covers(0, 1, 4, 3));
        assert!(!removed.covers(1, 0, 4, 3));
        let shared = left.combine(right, SelectionOp::Intersect, 4, 3);
        assert_eq!(shared, Selection::Rect(PixelRect::new(1, 0, 1, 1)));

        let edge = Selection::Rect(PixelRect::new(0, 0, 1, 1));
        let grown = edge.clone().grow(1, 3, 3);
        assert!(grown.covers(1, 0, 3, 3));
        assert!(grown.covers(0, 1, 3, 3));
        assert!(!grown.covers(2, 2, 3, 3));
        let shrunk = Selection::Full.shrink(1, 4, 4);
        assert!(!shrunk.covers(0, 1, 4, 4));
        assert!(shrunk.covers(1, 1, 4, 4));
        let feathered = edge.feather(1, 3, 3);
        assert!(feathered.coverage(0, 0, 3, 3) < 255);
        assert!(feathered.coverage(0, 0, 3, 3) > 0);
        assert_eq!(feathered.coverage(2, 2, 3, 3), 0);
    }
}
