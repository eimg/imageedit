use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

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
    // An axis-aligned ellipse inscribed in this rectangle.
    Ellipse(EllipseSelection),
    // The canvas except this rectangle.
    Inverse(PixelRect),
    Mask(Mask),
}

#[derive(Clone, Debug)]
pub struct EllipseSelection {
    rect: PixelRect,
    outline: Arc<OnceLock<Vec<MaskEdge>>>,
}

impl PartialEq for EllipseSelection {
    fn eq(&self, other: &Self) -> bool {
        self.rect == other.rect
    }
}

impl Eq for EllipseSelection {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionOp {
    Replace,
    Add,
    Subtract,
    Intersect,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionShape {
    #[default]
    Rectangle,
    Ellipse,
    Circle,
}

// Coverage is 0 or 255 for a hard mask. Feather stores the values between.
pub type MaskEdge = (i32, i32, i32, i32);

#[derive(Clone, Debug)]
pub struct Mask {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
    outline: Arc<OnceLock<Vec<MaskEdge>>>,
}

impl PartialEq for Mask {
    fn eq(&self, other: &Self) -> bool {
        self.bounds() == other.bounds()
            && (Arc::ptr_eq(&self.pixels, &other.pixels) || self.pixels == other.pixels)
    }
}

impl Eq for Mask {}

impl EllipseSelection {
    pub fn rect(&self) -> PixelRect {
        self.rect
    }

    fn contains(&self, x: i64, y: i64) -> bool {
        let rect = self.rect;
        if rect.width == 0 || rect.height == 0 {
            return false;
        }
        let cx = rect.x as f64 + rect.width as f64 / 2.;
        let cy = rect.y as f64 + rect.height as f64 / 2.;
        let rx = rect.width as f64 / 2.;
        let ry = rect.height as f64 / 2.;
        let dx = (x as f64 + 0.5 - cx) / rx;
        let dy = (y as f64 + 0.5 - cy) / ry;
        dx * dx + dy * dy <= 1.0 + 1e-9
    }

    fn row_span(&self, y: i64) -> Option<(i64, i64)> {
        let rect = self.rect;
        if rect.width == 0 || rect.height == 0 {
            return None;
        }
        let cx = rect.x as f64 + rect.width as f64 / 2.;
        let cy = rect.y as f64 + rect.height as f64 / 2.;
        let rx = rect.width as f64 / 2.;
        let ry = rect.height as f64 / 2.;
        let dy = ((y as f64 + 0.5 - cy) / ry).abs();
        if dy > 1.0 {
            return None;
        }
        let half = rx * (1.0 - dy * dy).max(0.0).sqrt();
        let mut start = (cx - half - 0.5).ceil() as i64;
        let mut end = (cx + half - 0.5).floor() as i64 + 1;
        start = start.max(rect.x);
        end = end.min(rect.right());
        while start < end && !self.contains(start, y) {
            start += 1;
        }
        while start < end && !self.contains(end - 1, y) {
            end -= 1;
        }
        if start >= end {
            return None;
        }
        while start > rect.x && self.contains(start - 1, y) {
            start -= 1;
        }
        while end < rect.right() && self.contains(end, y) {
            end += 1;
        }
        Some((start, end))
    }

    fn column_span(&self, x: i64) -> Option<(i64, i64)> {
        let rect = self.rect;
        if rect.width == 0 || rect.height == 0 {
            return None;
        }
        let cx = rect.x as f64 + rect.width as f64 / 2.;
        let cy = rect.y as f64 + rect.height as f64 / 2.;
        let rx = rect.width as f64 / 2.;
        let ry = rect.height as f64 / 2.;
        let dx = ((x as f64 + 0.5 - cx) / rx).abs();
        if dx > 1.0 {
            return None;
        }
        let half = ry * (1.0 - dx * dx).max(0.0).sqrt();
        let mut start = (cy - half - 0.5).ceil() as i64;
        let mut end = (cy + half - 0.5).floor() as i64 + 1;
        start = start.max(rect.y);
        end = end.min(rect.bottom());
        while start < end && !self.contains(x, start) {
            start += 1;
        }
        while start < end && !self.contains(x, end - 1) {
            end -= 1;
        }
        if start >= end {
            return None;
        }
        while start > rect.y && self.contains(x, start - 1) {
            start -= 1;
        }
        while end < rect.bottom() && self.contains(x, end) {
            end += 1;
        }
        Some((start, end))
    }

    fn outline(&self) -> Vec<MaskEdge> {
        let rect = self.rect;
        let mut edges = Vec::new();
        if rect.width == 0 || rect.height == 0 {
            return edges;
        }
        for y in rect.y..rect.bottom() {
            let Some(current) = self.row_span(y) else {
                continue;
            };
            for (start, end) in span_difference(current, self.row_span(y - 1)) {
                edges.push((start as i32, y as i32, end as i32, y as i32));
            }
            for (start, end) in span_difference(current, self.row_span(y + 1)) {
                edges.push((end as i32, (y + 1) as i32, start as i32, (y + 1) as i32));
            }
        }
        for x in rect.x..rect.right() {
            let Some(current) = self.column_span(x) else {
                continue;
            };
            for (start, end) in span_difference(current, self.column_span(x - 1)) {
                edges.push((x as i32, end as i32, x as i32, start as i32));
            }
            for (start, end) in span_difference(current, self.column_span(x + 1)) {
                edges.push(((x + 1) as i32, start as i32, (x + 1) as i32, end as i32));
            }
        }
        edges
    }
}

impl Selection {
    pub fn rectangle(start: (u32, u32), end: (u32, u32), width: u32, height: u32) -> Self {
        Self::from_rect(drag_rect(start, end), width, height)
    }

    pub fn ellipse(start: (u32, u32), end: (u32, u32), width: u32, height: u32) -> Self {
        Self::ellipse_rect(drag_rect(start, end), width, height)
    }

    pub fn circle(start: (u32, u32), end: (u32, u32), width: u32, height: u32) -> Self {
        Self::ellipse_rect(square_drag_rect(start, end), width, height)
    }

    pub fn circle_at(center: (u32, u32), radius: u32, width: u32, height: u32) -> Self {
        let radius = radius.max(1);
        let side = radius.saturating_mul(2);
        let rect = PixelRect::new(
            i64::from(center.0) - i64::from(radius),
            i64::from(center.1) - i64::from(radius),
            side,
            side,
        );
        Self::ellipse_rect(rect, width, height)
    }

    pub fn ellipse_rect(rect: PixelRect, width: u32, height: u32) -> Self {
        let Some(rect) = clip(rect, width, height) else {
            return Self::Empty;
        };
        Self::Ellipse(EllipseSelection {
            rect,
            outline: Arc::default(),
        })
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
            Self::Ellipse(ellipse) => raster_combine(
                Self::Full,
                Self::Ellipse(ellipse),
                SelectionOp::Subtract,
                width,
                height,
            ),
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
            Self::Ellipse(ellipse) => inside && ellipse.contains(x, y),
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
            Self::Ellipse(_) => Self::ellipse_rect(rect, width, height),
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
            Self::Ellipse(ellipse) => {
                Self::ellipse_rect(translate(ellipse.rect, -origin_x, -origin_y), width, height)
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

    pub fn resize_canvas(self, old: (u32, u32), new: (u32, u32), delta: (i32, i32)) -> Self {
        let (dx, dy) = (i64::from(delta.0), i64::from(delta.1));
        let translated_canvas =
            || Self::from_rect(PixelRect::new(dx, dy, old.0, old.1), new.0, new.1);
        match self {
            Self::Full => translated_canvas(),
            Self::Rect(rect) => Self::from_rect(translate(rect, dx, dy), new.0, new.1),
            Self::Ellipse(ellipse) => {
                Self::ellipse_rect(translate(ellipse.rect, dx, dy), new.0, new.1)
            }
            Self::Inverse(hole) => translated_canvas().combine(
                Self::from_rect(translate(hole, dx, dy), new.0, new.1),
                SelectionOp::Subtract,
                new.0,
                new.1,
            ),
            Self::Mask(mask) => mask.translated(dx, dy, new.0, new.1),
            other => other,
        }
    }

    pub fn contours(&self, width: u32, height: u32) -> Vec<PixelRect> {
        match self {
            Self::Unrestricted | Self::Empty => Vec::new(),
            Self::Full => vec![PixelRect::new(0, 0, width, height)],
            Self::Rect(rect) => vec![*rect],
            Self::Ellipse(_) => Vec::new(),
            Self::Inverse(hole) => vec![PixelRect::new(0, 0, width, height), *hole],
            Self::Mask(_) => Vec::new(),
        }
    }

    pub(crate) fn mask_storage(&self) -> Option<(*const u8, u64)> {
        let Self::Mask(mask) = self else {
            return None;
        };
        let outline_bytes = mask.outline.get().map_or(0, |edges| {
            edges.capacity() as u64 * std::mem::size_of::<MaskEdge>() as u64
        });
        Some((
            mask.pixels.as_ptr(),
            mask.pixels.len() as u64 + outline_bytes,
        ))
    }

    // Boundary segments of a dense mask, in canvas pixels. Each segment is (x0, y0, x1, y1).
    pub fn outline_edges(&self) -> &[MaskEdge] {
        match self {
            Self::Mask(mask) => mask.outline.get_or_init(|| mask.outline()),
            Self::Ellipse(ellipse) => ellipse.outline.get_or_init(|| ellipse.outline()),
            _ => &[],
        }
    }

    // Closed canvas polylines for stroking. Rectangles use their edges. A mask chains
    // its outline, so a hole is a second loop and spacing continues around each corner.
    pub fn stroke_polylines(&self, width: u32, height: u32) -> Vec<Vec<(f64, f64)>> {
        match self {
            Self::Mask(_) | Self::Ellipse(_) => chain_edges(self.outline_edges()),
            Self::Inverse(hole)
                if hole.x <= 0
                    || hole.y <= 0
                    || hole.right() >= i64::from(width)
                    || hole.bottom() >= i64::from(height) =>
            {
                inverse_edge_loops(*hole, width, height)
            }
            _ => self
                .contours(width, height)
                .into_iter()
                .filter_map(rect_loop)
                .collect(),
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
            Self::Ellipse(ellipse) => clip(ellipse.rect, width, height),
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

    pub fn polygon(points: &[(f64, f64)], width: u32, height: u32) -> Self {
        polygons_selection(&[points], width, height)
    }

    // Nonzero winding across every ring. Opposite contours cancel where they overlap.
    pub fn polygons(rings: &[&[(f64, f64)]], width: u32, height: u32) -> Self {
        polygons_selection(rings, width, height)
    }

    pub fn flood(
        image: &RgbaImage,
        x: u32,
        y: u32,
        tolerance: u8,
        offset: (i32, i32),
        canvas: (u32, u32),
    ) -> Self {
        flood_selection(image, x, y, tolerance, offset, canvas, |_, _| true)
    }

    pub fn flood_within(
        image: &RgbaImage,
        x: u32,
        y: u32,
        tolerance: u8,
        selection: &Selection,
    ) -> Self {
        let canvas = image.dimensions();
        flood_selection(image, x, y, tolerance, (0, 0), canvas, |px, py| {
            selection.covers(i64::from(px), i64::from(py), canvas.0, canvas.1)
        })
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
        let mapped = || {
            mask.pixels
                .iter()
                .enumerate()
                .filter_map(|(index, &coverage)| {
                    if coverage == 0 {
                        return None;
                    }
                    let (mx, my) = map(
                        mask.x + (index % mask.width as usize) as i32,
                        mask.y + (index / mask.width as usize) as i32,
                    );
                    (mx >= 0 && my >= 0 && (mx as u32) < width && (my as u32) < height)
                        .then_some((mx, my, coverage))
                })
        };
        let bounds = mapped().fold(None, |bounds, (x, y, _)| {
            Some(match bounds {
                None => (x, y, x, y),
                Some((left, top, right, bottom)) => {
                    (x.min(left), y.min(top), x.max(right), y.max(bottom))
                }
            })
        });
        let Some((left, top, right, bottom)) = bounds else {
            return Self::Empty;
        };
        let (buf_w, buf_h) = ((right - left + 1) as u32, (bottom - top + 1) as u32);
        let mut pixels = vec![0u8; buf_w as usize * buf_h as usize];
        for (x, y, coverage) in mapped() {
            let index = (y - top) as usize * buf_w as usize + (x - left) as usize;
            pixels[index] = pixels[index].max(coverage);
        }
        finish_mask(left, top, buf_w, buf_h, pixels, width, height)
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
        // A small feathered selection can have no pixel at half coverage.
        let threshold = if self.pixels.iter().any(|&value| value >= 128) {
            128
        } else {
            1
        };
        let selected = |x: i32, y: i32| self.coverage(x, y) >= threshold;
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
            .for_each(|(start, end)| edges.push((end, canvas_y + 1, start, canvas_y + 1)));
        }
        for x in 0..self.width as i32 {
            let canvas_x = self.x + x;
            coalesce(self.y, self.height, |y| {
                let canvas_y = self.y + y;
                selected(canvas_x, canvas_y) && !selected(canvas_x - 1, canvas_y)
            })
            .into_iter()
            .for_each(|(start, end)| edges.push((canvas_x, end, canvas_x, start)));
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

fn rect_loop(rect: PixelRect) -> Option<Vec<(f64, f64)>> {
    if rect.width == 0 || rect.height == 0 {
        return None;
    }
    let left = rect.x as f64;
    let top = rect.y as f64;
    let right = rect.right() as f64;
    let bottom = rect.bottom() as f64;
    Some(vec![
        (left, top),
        (right, top),
        (right, bottom),
        (left, bottom),
        (left, top),
    ])
}

type GridPoint = (i32, i32);

// Every edge keeps selected pixels on its right. Prefer a right turn at a
// diagonal contact, so touching islands stay separate loops. Seed order is stable
// because brush spacing makes the starting vertex observable.
fn chain_edges(edges: &[MaskEdge]) -> Vec<Vec<(f64, f64)>> {
    let mut links: HashMap<GridPoint, Vec<GridPoint>> = HashMap::new();
    for &(x0, y0, x1, y1) in edges {
        if (x0, y0) != (x1, y1) {
            links.entry((x0, y0)).or_default().push((x1, y1));
        }
    }
    let mut seeds: Vec<GridPoint> = links.keys().copied().collect();
    seeds.sort_unstable();
    let mut lines = Vec::new();
    for seed in seeds {
        while let Some(next) = take_link(&mut links, seed, None) {
            let mut line = vec![grid_point(seed), grid_point(next)];
            let mut previous = seed;
            let mut current = next;
            while current != seed {
                let Some(step) = take_link(&mut links, current, Some(previous)) else {
                    break;
                };
                line.push(grid_point(step));
                previous = current;
                current = step;
            }
            if line.len() >= 2 {
                lines.push(line);
            }
        }
    }
    lines
}

fn take_link(
    links: &mut HashMap<GridPoint, Vec<GridPoint>>,
    from: GridPoint,
    previous: Option<GridPoint>,
) -> Option<GridPoint> {
    let list = links.get_mut(&from)?;
    let direction = |from: GridPoint, to: GridPoint| {
        if to.0 > from.0 {
            0
        } else if to.1 > from.1 {
            1
        } else if to.0 < from.0 {
            2
        } else {
            3
        }
    };
    let index = list
        .iter()
        .enumerate()
        .min_by_key(|(_, next)| {
            let turn = previous.map_or(0, |previous| {
                match (direction(from, **next) + 4 - direction(previous, from)) % 4 {
                    1 => 0,
                    0 => 1,
                    3 => 2,
                    _ => 3,
                }
            });
            (turn, **next)
        })?
        .0;
    Some(list.swap_remove(index))
}

// An edge-touching inverse is a notch or disconnected strips, not two
// overlapping rectangles. A 3×3 partition is enough without a canvas-sized mask.
fn inverse_edge_loops(hole: PixelRect, width: u32, height: u32) -> Vec<Vec<(f64, f64)>> {
    let Some(hole) = clip(hole, width, height) else {
        return rect_loop(PixelRect::new(0, 0, width, height))
            .into_iter()
            .collect();
    };
    let mut xs = vec![0, hole.x as i32, hole.right() as i32, width as i32];
    let mut ys = vec![0, hole.y as i32, hole.bottom() as i32, height as i32];
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();
    let selected = |x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x < width as i32
            && y < height as i32
            && !contains(hole, i64::from(x), i64::from(y))
    };
    let mut edges = Vec::new();
    for x in xs.windows(2) {
        for y in ys.windows(2) {
            let (left, right, top, bottom) = (x[0], x[1], y[0], y[1]);
            if !selected(left, top) {
                continue;
            }
            if !selected(left, top - 1) {
                edges.push((left, top, right, top));
            }
            if !selected(right, top) {
                edges.push((right, top, right, bottom));
            }
            if !selected(left, bottom) {
                edges.push((right, bottom, left, bottom));
            }
            if !selected(left - 1, top) {
                edges.push((left, bottom, left, top));
            }
        }
    }
    chain_edges(&edges)
}

fn grid_point((x, y): GridPoint) -> (f64, f64) {
    (f64::from(x), f64::from(y))
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

fn drag_rect(start: (u32, u32), end: (u32, u32)) -> PixelRect {
    let left = i64::from(start.0.min(end.0));
    let top = i64::from(start.1.min(end.1));
    let right = i64::from(start.0.max(end.0)) + 1;
    let bottom = i64::from(start.1.max(end.1)) + 1;
    PixelRect::new(left, top, (right - left) as u32, (bottom - top) as u32)
}

fn square_drag_rect(start: (u32, u32), end: (u32, u32)) -> PixelRect {
    let sx = i64::from(start.0);
    let sy = i64::from(start.1);
    let dx = i64::from(end.0) - sx;
    let dy = i64::from(end.1) - sy;
    let side = dx.abs().max(dy.abs()) + 1;
    PixelRect::new(
        if dx >= 0 { sx } else { sx - side + 1 },
        if dy >= 0 { sy } else { sy - side + 1 },
        side as u32,
        side as u32,
    )
}

fn span_difference(current: (i64, i64), other: Option<(i64, i64)>) -> Vec<(i64, i64)> {
    let Some((other_start, other_end)) = other else {
        return vec![current];
    };
    let (start, end) = current;
    let mut parts = Vec::new();
    if start < other_start {
        parts.push((start, end.min(other_start)));
    }
    if other_end < end {
        parts.push((start.max(other_end), end));
    }
    parts.retain(|(start, end)| start < end);
    parts
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

fn polygons_selection(rings: &[&[(f64, f64)]], width: u32, height: u32) -> Selection {
    if width == 0 || height == 0 || rings.iter().all(|points| points.len() < 3) {
        return Selection::Empty;
    }
    let scan_edge =
        |value: f64, limit: u32| (value - 0.5).ceil().clamp(0., f64::from(limit)) as i32;
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for points in rings {
        if points.len() < 3 {
            continue;
        }
        for &(x, y) in *points {
            if !x.is_finite() || !y.is_finite() {
                return Selection::Empty;
            }
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    let (min_x, max_x) = (scan_edge(min_x, width), scan_edge(max_x, width));
    let (min_y, max_y) = (scan_edge(min_y, height), scan_edge(max_y, height));
    if min_x >= max_x || min_y >= max_y {
        return Selection::Empty;
    }
    let buf_w = (max_x - min_x) as u32;
    let buf_h = (max_y - min_y) as u32;
    let mut pixels = vec![0u8; buf_w as usize * buf_h as usize];
    let mut edges = Vec::new();
    for points in rings {
        if points.len() < 3 {
            continue;
        }
        for index in 0..points.len() {
            let (x0, y0) = points[index];
            let (x1, y1) = points[(index + 1) % points.len()];
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
    }
    let mut hits = Vec::new();
    for y in min_y..max_y {
        let scan = f64::from(y) + 0.5;
        hits.clear();
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
        for &(x, dir) in &hits {
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
    let start = ((left - 0.5).ceil() as i32).max(origin_x);
    let end = ((right - 0.5).ceil() as i32).min(origin_x + width as i32);
    if start < end {
        let row = (y - origin_y) as usize * width as usize;
        pixels[row + (start - origin_x) as usize..row + (end - origin_x) as usize].fill(255);
    }
}

fn flood_selection(
    image: &RgbaImage,
    x: u32,
    y: u32,
    tolerance: u8,
    offset: (i32, i32),
    canvas: (u32, u32),
    included: impl Fn(u32, u32) -> bool,
) -> Selection {
    if x >= image.width() || y >= image.height() {
        return Selection::Empty;
    }
    let target = *image.get_pixel(x, y);
    let (width, height) = image.dimensions();
    let Some(bounds) = clip(
        PixelRect::new(i64::from(offset.0), i64::from(offset.1), width, height),
        canvas.0,
        canvas.1,
    ) else {
        return Selection::Empty;
    };
    let local_x = (bounds.x - i64::from(offset.0)) as u32;
    let local_y = (bounds.y - i64::from(offset.1)) as u32;
    let mut pixels = vec![0; bounds.width as usize * bounds.height as usize];
    let mut seen = vec![false; width as usize * height as usize];
    let mut stack = vec![(x, y)];
    seen[(y * width + x) as usize] = true;
    while let Some((px, py)) = stack.pop() {
        if !included(px, py) || !within_tolerance(*image.get_pixel(px, py), target, tolerance) {
            continue;
        }
        if (local_x..local_x + bounds.width).contains(&px)
            && (local_y..local_y + bounds.height).contains(&py)
        {
            pixels[((py - local_y) * bounds.width + (px - local_x)) as usize] = 255;
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
    finish_mask(
        bounds.x as i32,
        bounds.y as i32,
        bounds.width,
        bounds.height,
        pixels,
        canvas.0,
        canvas.1,
    )
}

pub(crate) fn apply_coverage(alpha: u8, coverage: u8) -> u8 {
    ((u16::from(alpha) * u16::from(coverage) + 127) / 255) as u8
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
    let area = u64::from(radius * 2 + 1) * u64::from(radius * 2 + 1);
    let mut output = vec![0u8; source.len()];
    // Running column sums replace an eight-byte-per-pixel integral image.
    let columns = buf_w as usize;
    let rows = buf_h as usize;
    let pad = radius as usize;
    let mut vertical = vec![0u64; columns];
    for row in 0..=pad.min(rows - 1) {
        for (column, sum) in vertical.iter_mut().enumerate() {
            *sum += u64::from(source[row * columns + column]);
        }
    }
    for row in 0..rows {
        if row > 0 {
            if row + pad < rows {
                for (column, sum) in vertical.iter_mut().enumerate() {
                    *sum += u64::from(source[(row + pad) * columns + column]);
                }
            }
            if row > pad {
                for (column, sum) in vertical.iter_mut().enumerate() {
                    *sum -= u64::from(source[(row - pad - 1) * columns + column]);
                }
            }
        }
        let mut sum: u64 = vertical[..=pad.min(columns - 1)].iter().sum();
        for column in 0..columns {
            if column > 0 {
                if column + pad < columns {
                    sum += vertical[column + pad];
                }
                if column > pad {
                    sum -= vertical[column - pad - 1];
                }
            }
            output[row * columns + column] = match kind {
                Reshape::Grow => u8::from(sum > 0) * 255,
                Reshape::Shrink => u8::from(sum == area) * 255,
                Reshape::Feather => ((sum + area / 2) / area).min(255) as u8,
            };
        }
    }
    finish_mask(left, top, buf_w, buf_h, output, width, height)
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
        outline: Arc::default(),
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
    fn ellipse_selects_its_center_and_leaves_the_corners_clear() {
        let ellipse = Selection::ellipse_rect(PixelRect::new(0, 0, 5, 5), 5, 5);
        assert!(ellipse.covers(2, 2, 5, 5));
        assert!(!ellipse.covers(0, 0, 5, 5));
        assert!(!ellipse.covers(4, 4, 5, 5));
        let inverted = ellipse.clone().invert(5, 5);
        assert!(!inverted.covers(2, 2, 5, 5));
        assert!(inverted.covers(0, 0, 5, 5));
    }

    #[test]
    fn ellipse_drag_bounds_match_the_rectangle_and_circle_uses_a_square() {
        let bounds = PixelRect::new(1, 2, 4, 5);
        assert_eq!(
            Selection::ellipse((1, 2), (4, 6), 10, 10),
            Selection::ellipse_rect(bounds, 10, 10)
        );
        assert_eq!(
            Selection::circle((1, 2), (5, 3), 10, 10),
            Selection::ellipse_rect(PixelRect::new(1, 2, 5, 5), 10, 10)
        );
    }

    #[test]
    fn ellipse_clips_and_moves_like_a_rectangle() {
        let ellipse = Selection::ellipse_rect(PixelRect::new(2, 2, 4, 4), 10, 10);
        assert_eq!(
            ellipse.crop_bounds(10, 10),
            Some(PixelRect::new(2, 2, 4, 4))
        );
        assert_eq!(
            ellipse.clone().rebase(1, 1, 9, 9),
            Selection::ellipse_rect(PixelRect::new(1, 1, 4, 4), 9, 9)
        );
        assert_eq!(
            ellipse.clone().resize_canvas((10, 10), (12, 10), (2, 0)),
            Selection::ellipse_rect(PixelRect::new(4, 2, 4, 4), 12, 10)
        );
        assert_eq!(
            Selection::ellipse_rect(PixelRect::new(20, 20, 4, 4), 10, 10),
            Selection::Empty
        );
    }

    #[test]
    fn ellipse_combines_and_inverts_by_coverage() {
        let ellipse = Selection::ellipse_rect(PixelRect::new(1, 1, 5, 5), 10, 10);
        let added = ellipse.clone().combine(
            Selection::Rect(PixelRect::new(0, 0, 3, 3)),
            SelectionOp::Add,
            10,
            10,
        );
        assert!(added.covers(0, 0, 10, 10));
        let subtracted = ellipse
            .clone()
            .combine(Selection::Full, SelectionOp::Subtract, 10, 10);
        assert_eq!(subtracted, Selection::Empty);
        let inverted = ellipse.invert(10, 10);
        assert!(inverted.covers(0, 0, 10, 10));
        assert!(!inverted.covers(3, 3, 10, 10));
    }

    #[test]
    fn ellipse_outline_is_cached_and_nonempty() {
        let ellipse = Selection::ellipse_rect(PixelRect::new(1, 1, 6, 6), 10, 10);
        let edges = ellipse.outline_edges();
        assert!(!edges.is_empty());
        let first = edges.as_ptr();
        assert_eq!(ellipse.outline_edges().as_ptr(), first);
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
    fn an_inverse_selection_does_not_extend_into_new_canvas_space() {
        let inverse = Selection::Inverse(PixelRect::new(1, 1, 1, 1));
        let resized = inverse.resize_canvas((3, 3), (5, 5), (1, 1));
        assert!(resized.covers(1, 1, 5, 5));
        assert!(!resized.covers(2, 2, 5, 5));
        assert!(!resized.covers(0, 0, 5, 5));
        assert!(!resized.covers(4, 4, 5, 5));
    }

    #[test]
    fn polygon_winding_agrees_in_both_directions_and_clips_to_the_canvas() {
        let clockwise = [(1.5, 1.5), (5.5, 1.5), (1.5, 5.5)];
        let mut counter = clockwise;
        counter.reverse();
        let left = Selection::polygon(&clockwise, 8, 8);
        let right = Selection::polygon(&counter, 8, 8);
        assert!(left.covers(2, 2, 8, 8));
        assert!(!left.covers(4, 4, 8, 8));
        assert_eq!(left.covers(2, 2, 8, 8), right.covers(2, 2, 8, 8));
        assert_eq!(left.covers(4, 4, 8, 8), right.covers(4, 4, 8, 8));
        let clipped = Selection::polygon(&[(-1.5, 1.5), (2.5, 1.5), (2.5, -1.5)], 4, 4);
        assert!(clipped.covers(1, 0, 4, 4));
        assert!(!clipped.covers(3, 3, 4, 4));
    }

    #[test]
    fn fractional_lasso_vertices_select_pixel_centers_and_clip_distant_edges() {
        let outline = [(0.6, 0.6), (1.9, 0.6), (1.9, 1.9), (0.6, 1.9)];
        assert_eq!(
            Selection::polygon(&outline, 4, 4),
            Selection::Rect(PixelRect::new(1, 1, 1, 1))
        );
        let outside = [(-1e12, -1e12), (4., -1e12), (4., 4.), (-1e12, 4.)];
        assert_eq!(Selection::polygon(&outside, 4, 4), Selection::Full);
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

    #[test]
    fn stroke_polylines_follow_rectangles_holes_and_mask_contours() {
        assert!(Selection::Empty.stroke_polylines(8, 8).is_empty());
        assert!(Selection::Unrestricted.stroke_polylines(8, 8).is_empty());
        let rect = Selection::rectangle((2, 3), (5, 6), 12, 12);
        let rect_line = vec![(2.0, 3.0), (6.0, 3.0), (6.0, 7.0), (2.0, 7.0), (2.0, 3.0)];
        assert_eq!(rect.stroke_polylines(12, 12), vec![rect_line.clone()]);
        let inverse = rect.invert(12, 12).stroke_polylines(12, 12);
        assert_eq!(inverse.len(), 2);
        assert_eq!(inverse[0].first(), inverse[0].last());
        assert_eq!(inverse[1], rect_line);
        assert_eq!(
            Selection::Full.stroke_polylines(4, 3),
            vec![vec![
                (0.0, 0.0),
                (4.0, 0.0),
                (4.0, 3.0),
                (0.0, 3.0),
                (0.0, 0.0)
            ]]
        );

        let outer = [(1.0, 1.0), (15.0, 1.0), (15.0, 15.0), (1.0, 15.0)];
        let hole = [(5.0, 11.0), (11.0, 11.0), (11.0, 5.0), (5.0, 5.0)];
        let donut = Selection::polygons(&[&outer, &hole], 16, 16);
        assert!(matches!(donut, Selection::Mask(_)));
        assert!(donut.covers(3, 8, 16, 16));
        assert!(!donut.covers(8, 8, 16, 16));
        let contours = donut.stroke_polylines(16, 16);
        assert_eq!(contours.len(), 2);
        assert!(contours.iter().all(|line| {
            line.len() >= 4
                && line.first() == line.last()
                && line[line.len() - 2] != *line.last().unwrap()
        }));
    }

    #[test]
    fn diagonal_islands_have_separate_repeatable_stroke_loops() {
        let mask = Selection::Rect(PixelRect::new(0, 0, 1, 1)).combine(
            Selection::Rect(PixelRect::new(1, 1, 1, 1)),
            SelectionOp::Add,
            3,
            3,
        );
        let lines = mask.stroke_polylines(3, 3);
        assert_eq!(lines.len(), 2);
        assert!(
            lines
                .iter()
                .all(|line| line.len() == 5 && line.first() == line.last())
        );
        assert_eq!(lines, mask.stroke_polylines(3, 3));
        let reversed = mask
            .outline_edges()
            .iter()
            .rev()
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(lines, chain_edges(&reversed));
    }

    #[test]
    fn an_inverse_touching_the_canvas_strokes_only_the_selected_boundary() {
        let selection = Selection::Inverse(PixelRect::new(0, 0, 2, 2));
        let lines = selection.stroke_polylines(4, 4);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].first(), lines[0].last());
        assert!(!lines[0].contains(&(0.0, 0.0)));
        let length: f64 = lines[0]
            .windows(2)
            .map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1))
            .sum();
        assert_eq!(length, 16.0);
        let strips = Selection::Inverse(PixelRect::new(1, 0, 2, 4)).stroke_polylines(4, 4);
        assert_eq!(strips.len(), 2);
        assert!(strips.iter().all(|line| line.first() == line.last()));
        assert!(
            Selection::Inverse(PixelRect::new(0, 0, 4, 4))
                .stroke_polylines(4, 4)
                .is_empty()
        );
    }
    #[test]
    fn mask_mapping_keeps_partial_coverage_and_clips_outside_points() {
        let source = finish_mask(0, 0, 3, 2, vec![0, 80, 160, 240, 0, 40], 3, 2);
        let rotated = source.clone().map_mask(2, 3, |x, y| (1 - y, x));
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(
                    source.coverage(x, y, 3, 2),
                    rotated.coverage(1 - y, x, 2, 3)
                );
            }
        }
        let clipped = source.clone().map_mask(3, 2, |x, y| (x - 1, y));
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(
                    clipped.coverage(x, y, 3, 2),
                    source.coverage(x + 1, y, 3, 2)
                );
            }
        }
    }

    #[test]
    fn bounded_morphology_matches_direct_neighborhood_sums() {
        for (width, height) in [(5, 4), (1, 5), (5, 1)] {
            let source = finish_mask(
                0,
                0,
                width,
                height,
                (0..width * height)
                    .map(|i| ((i * 59 + 20) % 256) as u8)
                    .collect(),
                width,
                height,
            );
            for radius in [1, 2, 6] {
                for kind in [Reshape::Grow, Reshape::Shrink, Reshape::Feather] {
                    let mut expected = Vec::new();
                    let area = u64::from((radius * 2 + 1) * (radius * 2 + 1));
                    for y in 0..i64::from(height) {
                        for x in 0..i64::from(width) {
                            let mut sum = 0u64;
                            for dy in -i64::from(radius)..=i64::from(radius) {
                                for dx in -i64::from(radius)..=i64::from(radius) {
                                    let value = source.coverage(x + dx, y + dy, width, height);
                                    sum += match kind {
                                        Reshape::Feather => u64::from(value),
                                        _ => u64::from(value > 0),
                                    };
                                }
                            }
                            expected.push(match kind {
                                Reshape::Grow => u8::from(sum > 0) * 255,
                                Reshape::Shrink => u8::from(sum == area) * 255,
                                Reshape::Feather => ((sum + area / 2) / area) as u8,
                            });
                        }
                    }
                    let result = reshape(source.clone(), radius, width, height, kind);
                    for y in 0..height {
                        for x in 0..width {
                            assert_eq!(
                                result.coverage(i64::from(x), i64::from(y), width, height),
                                expected[(y * width + x) as usize]
                            );
                        }
                    }
                }
            }
        }
    }
}
