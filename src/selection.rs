mod antialias;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use image::{Rgba, RgbaImage};

use crate::preview::PixelRect;

pub const MAX_GAP_RADIUS: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegionMatch {
    pub tolerance: u8,
    pub gap_radius: u8,
}

impl RegionMatch {
    /// Close small breaks in the nonmatching boundaries, in source-pixel space.
    /// Original nonmatching pixels remain blocked independently of this mask.
    pub fn gap_barrier(self, image: &RgbaImage, seed: (u32, u32)) -> Option<Selection> {
        let radius = u32::from(self.gap_radius.min(MAX_GAP_RADIUS));
        if radius == 0 || seed.0 >= image.width() || seed.1 >= image.height() {
            return None;
        }
        let target = *image.get_pixel(seed.0, seed.1);
        let mut pixels: Vec<u8> = image
            .pixels()
            .map(|pixel| u8::from(!within_tolerance(*pixel, target, self.tolerance)) * 255)
            .collect();
        let (width, height) = image.dimensions();
        // The source is a finite domain: erosion ignores nonexistent neighbors
        // so nearby image edges do not reopen a bridge. Homogeneous sources
        // still have no barriers, including one-pixel-wide images.
        extrema_image(
            &mut pixels,
            (width as usize, height as usize),
            radius as usize,
            true,
            0,
        );
        extrema_image(
            &mut pixels,
            (width as usize, height as usize),
            radius as usize,
            false,
            255,
        );
        Some(finish_mask(0, 0, width, height, pixels, width, height))
    }
}

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
    // A rectangle with rounded corners, inscribed in this rectangle.
    RoundedRect(RoundedRectSelection),
    // The canvas except this rectangle.
    Inverse(PixelRect),
    Mask(Mask),
}

#[derive(Clone, Debug)]
pub struct RoundedRectSelection {
    rect: PixelRect,
    radius: u32,
    outline: Arc<OnceLock<Vec<MaskEdge>>>,
    antialias: bool,
}

impl PartialEq for RoundedRectSelection {
    fn eq(&self, other: &Self) -> bool {
        self.rect == other.rect && self.radius == other.radius && self.antialias == other.antialias
    }
}

impl Eq for RoundedRectSelection {}

#[derive(Clone, Debug)]
pub struct EllipseSelection {
    rect: PixelRect,
    outline: Arc<OnceLock<Vec<MaskEdge>>>,
    antialias: bool,
}

impl PartialEq for EllipseSelection {
    fn eq(&self, other: &Self) -> bool {
        self.rect == other.rect && self.antialias == other.antialias
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
    fn edge_contains(&self, x: i64, y: i64) -> bool {
        if self.antialias {
            self.coverage(x, y) > 0
        } else {
            self.contains(x, y)
        }
    }

    fn coverage(&self, x: i64, y: i64) -> u8 {
        if !contains(self.rect, x, y) {
            return 0;
        }
        if !self.antialias {
            return u8::from(self.contains(x, y)) * 255;
        }
        let rx = self.rect.width as f64 / 2.;
        let ry = self.rect.height as f64 / 2.;
        let dx = ((x as f64 + 0.5 - self.rect.x as f64 - rx) / rx).abs();
        let dy = ((y as f64 + 0.5 - self.rect.y as f64 - ry) / ry).abs();
        let mx = 0.5 / rx;
        let my = 0.5 / ry;
        // Bound the whole pixel in normalized ellipse space. Only pixels whose
        // boxes intersect the curve need the sixteen subpixel tests.
        if (dx + mx).powi(2) + (dy + my).powi(2) <= 1. {
            return 255;
        }
        if (dx - mx).max(0.).powi(2) + (dy - my).max(0.).powi(2) > 1. {
            return 0;
        }
        sample_coverage(x, y, |x, y| self.contains_point(x, y))
    }

    pub fn rect(&self) -> PixelRect {
        self.rect
    }

    fn contains(&self, x: i64, y: i64) -> bool {
        self.contains_point(x as f64 + 0.5, y as f64 + 0.5)
    }

    fn contains_point(&self, x: f64, y: f64) -> bool {
        let rect = self.rect;
        if rect.width == 0 || rect.height == 0 {
            return false;
        }
        let cx = rect.x as f64 + rect.width as f64 / 2.;
        let cy = rect.y as f64 + rect.height as f64 / 2.;
        let rx = rect.width as f64 / 2.;
        let ry = rect.height as f64 / 2.;
        let dx = (x - cx) / rx;
        let dy = (y - cy) / ry;
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
        if self.antialias {
            start -= 1;
            end += 1;
        }
        start = start.max(rect.x);
        end = end.min(rect.right());
        while start < end && !self.edge_contains(start, y) {
            start += 1;
        }
        while start < end && !self.edge_contains(end - 1, y) {
            end -= 1;
        }
        if start >= end {
            return None;
        }
        while start > rect.x && self.edge_contains(start - 1, y) {
            start -= 1;
        }
        while end < rect.right() && self.edge_contains(end, y) {
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
        if self.antialias {
            start -= 1;
            end += 1;
        }
        start = start.max(rect.y);
        end = end.min(rect.bottom());
        while start < end && !self.edge_contains(x, start) {
            start += 1;
        }
        while start < end && !self.edge_contains(x, end - 1) {
            end -= 1;
        }
        if start >= end {
            return None;
        }
        while start > rect.y && self.edge_contains(x, start - 1) {
            start -= 1;
        }
        while end < rect.bottom() && self.edge_contains(x, end) {
            end += 1;
        }
        Some((start, end))
    }

    fn outline(&self) -> Vec<MaskEdge> {
        shape_outline(self.rect, |y| self.row_span(y), |x| self.column_span(x))
    }
}

impl RoundedRectSelection {
    fn edge_contains(&self, x: i64, y: i64) -> bool {
        if self.antialias {
            self.coverage(x, y) > 0
        } else {
            self.contains(x, y)
        }
    }

    fn coverage(&self, x: i64, y: i64) -> u8 {
        if !contains(self.rect, x, y) {
            return 0;
        }
        if !self.antialias {
            return u8::from(self.contains(x, y)) * 255;
        }
        let radius = self.clamped_radius() as f64;
        let left = self.rect.x as f64;
        let top = self.rect.y as f64;
        let right = self.rect.right() as f64;
        let bottom = self.rect.bottom() as f64;
        if (x as f64 >= left + radius && x as f64 + 1. <= right - radius)
            || (y as f64 >= top + radius && y as f64 + 1. <= bottom - radius)
        {
            return 255;
        }
        let px = x as f64 + 0.5;
        let py = y as f64 + 0.5;
        let dx = (px - px.clamp(left + radius, right - radius)).abs();
        let dy = (py - py.clamp(top + radius, bottom - radius)).abs();
        if (dx + 0.5).powi(2) + (dy + 0.5).powi(2) <= radius * radius {
            return 255;
        }
        if (dx - 0.5).max(0.).powi(2) + (dy - 0.5).max(0.).powi(2) > radius * radius {
            return 0;
        }
        sample_coverage(x, y, |x, y| self.contains_point(x, y))
    }

    pub fn rect(&self) -> PixelRect {
        self.rect
    }

    pub fn radius(&self) -> u32 {
        self.clamped_radius()
    }

    fn clamped_radius(&self) -> u32 {
        self.radius.min(self.rect.width.min(self.rect.height) / 2)
    }

    fn contains(&self, x: i64, y: i64) -> bool {
        let rect = self.rect;
        if rect.width == 0 || rect.height == 0 {
            return false;
        }
        self.contains_point(x as f64 + 0.5, y as f64 + 0.5)
    }

    fn contains_point(&self, px: f64, py: f64) -> bool {
        let rect = self.rect;
        let left = rect.x as f64;
        let top = rect.y as f64;
        let right = rect.right() as f64;
        let bottom = rect.bottom() as f64;
        if px < left || px >= right || py < top || py >= bottom {
            return false;
        }
        let radius = f64::from(self.clamped_radius());
        if radius <= 0.0 {
            return true;
        }
        let cx = px.clamp(left + radius, right - radius);
        let cy = py.clamp(top + radius, bottom - radius);
        let dx = px - cx;
        let dy = py - cy;
        dx * dx + dy * dy <= radius * radius + 1e-9
    }

    fn row_span(&self, y: i64) -> Option<(i64, i64)> {
        let rect = self.rect;
        if rect.width == 0 || rect.height == 0 {
            return None;
        }
        let py = y as f64 + 0.5;
        let top = rect.y as f64;
        let bottom = rect.bottom() as f64;
        if py < top || py >= bottom {
            return None;
        }
        let radius = f64::from(self.clamped_radius());
        let (left, right) = if radius <= 0.0 {
            (rect.x as f64, rect.right() as f64)
        } else if py < top + radius {
            let half = (radius * radius - (top + radius - py).powi(2))
                .max(0.0)
                .sqrt();
            (
                rect.x as f64 + radius - half,
                rect.right() as f64 - radius + half,
            )
        } else if py >= bottom - radius {
            let half = (radius * radius - (py - (bottom - radius)).powi(2))
                .max(0.0)
                .sqrt();
            (
                rect.x as f64 + radius - half,
                rect.right() as f64 - radius + half,
            )
        } else {
            (rect.x as f64, rect.right() as f64)
        };
        let mut start = (left - 0.5).ceil() as i64;
        let mut end = (right - 0.5).floor() as i64 + 1;
        if self.antialias {
            start -= 1;
            end += 1;
        }
        start = start.max(rect.x);
        end = end.min(rect.right());
        while start < end && !self.edge_contains(start, y) {
            start += 1;
        }
        while start < end && !self.edge_contains(end - 1, y) {
            end -= 1;
        }
        if start >= end {
            return None;
        }
        while start > rect.x && self.edge_contains(start - 1, y) {
            start -= 1;
        }
        while end < rect.right() && self.edge_contains(end, y) {
            end += 1;
        }
        Some((start, end))
    }

    fn column_span(&self, x: i64) -> Option<(i64, i64)> {
        let rect = self.rect;
        if rect.width == 0 || rect.height == 0 {
            return None;
        }
        let px = x as f64 + 0.5;
        let left = rect.x as f64;
        let right = rect.right() as f64;
        if px < left || px >= right {
            return None;
        }
        let radius = f64::from(self.clamped_radius());
        let (top, bottom) = if radius <= 0.0 {
            (rect.y as f64, rect.bottom() as f64)
        } else if px < left + radius {
            let half = (radius * radius - (left + radius - px).powi(2))
                .max(0.0)
                .sqrt();
            (
                rect.y as f64 + radius - half,
                rect.bottom() as f64 - radius + half,
            )
        } else if px >= right - radius {
            let half = (radius * radius - (px - (right - radius)).powi(2))
                .max(0.0)
                .sqrt();
            (
                rect.y as f64 + radius - half,
                rect.bottom() as f64 - radius + half,
            )
        } else {
            (rect.y as f64, rect.bottom() as f64)
        };
        let mut start = (top - 0.5).ceil() as i64;
        let mut end = (bottom - 0.5).floor() as i64 + 1;
        if self.antialias {
            start -= 1;
            end += 1;
        }
        start = start.max(rect.y);
        end = end.min(rect.bottom());
        while start < end && !self.edge_contains(x, start) {
            start += 1;
        }
        while start < end && !self.edge_contains(x, end - 1) {
            end -= 1;
        }
        if start >= end {
            return None;
        }
        while start > rect.y && self.edge_contains(x, start - 1) {
            start -= 1;
        }
        while end < rect.bottom() && self.edge_contains(x, end) {
            end += 1;
        }
        Some((start, end))
    }

    fn outline(&self) -> Vec<MaskEdge> {
        shape_outline(self.rect, |y| self.row_span(y), |x| self.column_span(x))
    }
}

fn sample_coverage(x: i64, y: i64, inside: impl Fn(f64, f64) -> bool) -> u8 {
    let mut count = 0u16;
    for sy in 0..4 {
        for sx in 0..4 {
            count += u16::from(inside(
                x as f64 + (sx as f64 + 0.5) / 4.,
                y as f64 + (sy as f64 + 0.5) / 4.,
            ));
        }
    }
    ((count * 255 + 8) / 16) as u8
}

impl Selection {
    pub fn shape_drag(
        shape: SelectionShape,
        start: (i64, i64),
        end: (i64, i64),
        canvas: (u32, u32),
        radius: u32,
        antialias: bool,
        ratio: Option<crate::constraints::AspectRatio>,
    ) -> Self {
        if !antialias && ratio.is_none() {
            return match shape {
                SelectionShape::Rectangle if radius == 0 => {
                    Self::rectangle(start, end, canvas.0, canvas.1)
                }
                SelectionShape::Rectangle => {
                    Self::rounded_rect(start, end, radius, canvas.0, canvas.1)
                }
                SelectionShape::Ellipse => Self::ellipse(start, end, canvas.0, canvas.1),
                SelectionShape::Circle => Self::circle(start, end, canvas.0, canvas.1),
            };
        }
        let rect = ratio.map_or_else(
            || {
                if shape == SelectionShape::Circle {
                    square_drag_rect(start, end)
                } else {
                    drag_rect(start, end)
                }
            },
            |ratio| ratio.drag(start, end, None),
        );
        match shape {
            SelectionShape::Rectangle => {
                Self::rounded_rect_quality(rect, radius, canvas.0, canvas.1, antialias)
            }
            SelectionShape::Ellipse | SelectionShape::Circle => {
                Self::ellipse_rect_quality(rect, canvas.0, canvas.1, antialias)
            }
        }
    }

    pub fn rectangle(start: (i64, i64), end: (i64, i64), width: u32, height: u32) -> Self {
        Self::from_rect(drag_rect(start, end), width, height)
    }

    pub fn ellipse(start: (i64, i64), end: (i64, i64), width: u32, height: u32) -> Self {
        Self::ellipse_rect(drag_rect(start, end), width, height)
    }

    pub fn circle(start: (i64, i64), end: (i64, i64), width: u32, height: u32) -> Self {
        Self::ellipse_rect(square_drag_rect(start, end), width, height)
    }

    pub fn rounded_rect(
        start: (i64, i64),
        end: (i64, i64),
        radius: u32,
        width: u32,
        height: u32,
    ) -> Self {
        Self::rounded_rect_rect(drag_rect(start, end), radius, width, height)
    }

    pub fn ellipse_rect(rect: PixelRect, width: u32, height: u32) -> Self {
        Self::ellipse_rect_quality(rect, width, height, false)
    }

    pub fn ellipse_rect_quality(rect: PixelRect, width: u32, height: u32, antialias: bool) -> Self {
        Self::Ellipse(EllipseSelection {
            rect,
            outline: Arc::default(),
            antialias,
        })
        .clip_shape(rect, width, height)
    }

    pub fn rounded_rect_rect(rect: PixelRect, radius: u32, width: u32, height: u32) -> Self {
        Self::rounded_rect_quality(rect, radius, width, height, false)
    }

    pub fn rounded_rect_quality(
        rect: PixelRect,
        radius: u32,
        width: u32,
        height: u32,
        antialias: bool,
    ) -> Self {
        let radius = radius.min(rect.width.min(rect.height) / 2);
        if radius == 0 {
            return Self::from_rect(rect, width, height);
        }
        Self::RoundedRect(RoundedRectSelection {
            rect,
            radius,
            outline: Arc::default(),
            antialias,
        })
        .clip_shape(rect, width, height)
    }

    // Clipping a curved shape must retain its original curvature. Rebuilding
    // the shape in the clipped box would squeeze it into a different ellipse
    // or add rounded corners along the new canvas edge.
    fn clip_shape(self, rect: PixelRect, width: u32, height: u32) -> Self {
        let Some(clipped) = clip(rect, width, height) else {
            return Self::Empty;
        };
        if clipped == rect {
            return self;
        }
        let mut pixels = Vec::with_capacity(clipped.width as usize * clipped.height as usize);
        for y in clipped.y..clipped.bottom() {
            for x in clipped.x..clipped.right() {
                pixels.push(self.coverage(x, y, width, height));
            }
        }
        finish_mask(
            clipped.x as i32,
            clipped.y as i32,
            clipped.width,
            clipped.height,
            pixels,
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
            Self::Ellipse(ellipse) => raster_combine(
                Self::Full,
                Self::Ellipse(ellipse),
                SelectionOp::Subtract,
                width,
                height,
            ),
            Self::RoundedRect(shape) => raster_combine(
                Self::Full,
                Self::RoundedRect(shape),
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
            Self::Ellipse(ellipse) => inside && ellipse.edge_contains(x, y),
            Self::RoundedRect(shape) => inside && shape.edge_contains(x, y),
            Self::Inverse(hole) => inside && !contains(*hole, x, y),
            Self::Mask(mask) => inside && mask.coverage(x as i32, y as i32) > 0,
        }
    }

    pub fn coverage(&self, x: i64, y: i64, width: u32, height: u32) -> u8 {
        let inside = x >= 0 && y >= 0 && x < i64::from(width) && y < i64::from(height);
        match self {
            Self::Ellipse(shape) => {
                if inside {
                    shape.coverage(x, y)
                } else {
                    0
                }
            }
            Self::RoundedRect(shape) => {
                if inside {
                    shape.coverage(x, y)
                } else {
                    0
                }
            }
            Self::Mask(mask) => {
                if inside {
                    mask.coverage(x as i32, y as i32)
                } else {
                    0
                }
            }
            _ => u8::from(self.covers(x, y, width, height)) * 255,
        }
    }

    pub fn is_antialiased(&self) -> bool {
        match self {
            Self::Ellipse(shape) => shape.antialias,
            Self::RoundedRect(shape) => shape.antialias,
            _ => false,
        }
    }

    pub fn with_antialias(self, antialias: bool, width: u32, height: u32) -> Self {
        match self {
            Self::Ellipse(shape) => {
                Self::ellipse_rect_quality(shape.rect, width, height, antialias)
            }
            Self::RoundedRect(shape) => {
                Self::rounded_rect_quality(shape.rect, shape.radius(), width, height, antialias)
            }
            other => other,
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
            Self::Ellipse(shape) => {
                Self::ellipse_rect_quality(rect, width, height, shape.antialias)
            }
            Self::RoundedRect(shape) => {
                Self::rounded_rect_quality(rect, shape.radius(), width, height, shape.antialias)
            }
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
            Self::Ellipse(ellipse) => Self::ellipse_rect_quality(
                translate(ellipse.rect, -origin_x, -origin_y),
                width,
                height,
                ellipse.antialias,
            ),
            Self::RoundedRect(shape) => Self::rounded_rect_quality(
                translate(shape.rect, -origin_x, -origin_y),
                shape.radius(),
                width,
                height,
                shape.antialias,
            ),
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
            Self::Ellipse(ellipse) => Self::ellipse_rect_quality(
                translate(ellipse.rect, dx, dy),
                new.0,
                new.1,
                ellipse.antialias,
            ),
            Self::RoundedRect(shape) => Self::rounded_rect_quality(
                translate(shape.rect, dx, dy),
                shape.radius(),
                new.0,
                new.1,
                shape.antialias,
            ),
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
            Self::Ellipse(_) | Self::RoundedRect(_) => Vec::new(),
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
            Self::RoundedRect(shape) => shape.outline.get_or_init(|| shape.outline()),
            _ => &[],
        }
    }

    // Closed canvas polylines for stroking. Rectangles use their edges. A mask chains
    // its outline, so a hole is a second loop and spacing continues around each corner.
    pub fn stroke_polylines(&self, width: u32, height: u32) -> Vec<Vec<(f64, f64)>> {
        match self {
            Self::Mask(_) | Self::Ellipse(_) | Self::RoundedRect(_) => {
                chain_edges(self.outline_edges())
            }
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
            Self::RoundedRect(shape) => clip(shape.rect, width, height),
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
        polygons_selection(&[points], width, height, false)
    }

    pub fn polygon_quality(
        points: &[(f64, f64)],
        width: u32,
        height: u32,
        antialias: bool,
    ) -> Self {
        if !antialias {
            return Self::polygon(points, width, height);
        }
        polygons_selection(&[points], width, height, true)
    }

    // Nonzero winding across every ring. Opposite contours cancel where they overlap.
    pub fn polygons(rings: &[&[(f64, f64)]], width: u32, height: u32) -> Self {
        polygons_selection(rings, width, height, false)
    }

    pub fn flood(
        image: &RgbaImage,
        x: u32,
        y: u32,
        tolerance: u8,
        offset: (i32, i32),
        canvas: (u32, u32),
    ) -> Self {
        flood_selection(image, (x, y), tolerance, offset, canvas, |_, _| true, false)
    }

    pub fn flood_region(
        image: &RgbaImage,
        seed: (u32, u32),
        matching: RegionMatch,
        offset: (i32, i32),
        canvas: (u32, u32),
        limit: Option<&Selection>,
    ) -> Self {
        Self::flood_region_quality(image, seed, matching, offset, canvas, limit, false)
    }

    // Connectivity and gap closure are always hard; antialiasing only rebuilds
    // the completed region's contour before it is clipped to canvas coverage.
    pub fn flood_region_quality(
        image: &RgbaImage,
        seed: (u32, u32),
        matching: RegionMatch,
        offset: (i32, i32),
        canvas: (u32, u32),
        limit: Option<&Selection>,
        antialias: bool,
    ) -> Self {
        if !antialias && matching.gap_radius == 0 && limit.is_none() {
            return Self::flood(image, seed.0, seed.1, matching.tolerance, offset, canvas);
        }
        let barrier = matching.gap_barrier(image, seed);
        flood_selection(
            image,
            seed,
            matching.tolerance,
            offset,
            canvas,
            |px, py| {
                barrier.as_ref().is_none_or(|barrier| {
                    !barrier.covers(i64::from(px), i64::from(py), image.width(), image.height())
                }) && limit.is_none_or(|limit| {
                    limit.covers(
                        i64::from(px) + i64::from(offset.0),
                        i64::from(py) + i64::from(offset.1),
                        canvas.0,
                        canvas.1,
                    )
                })
            },
            antialias,
        )
    }

    pub fn flood_within(
        image: &RgbaImage,
        x: u32,
        y: u32,
        tolerance: u8,
        selection: &Selection,
    ) -> Self {
        let canvas = image.dimensions();
        flood_selection(
            image,
            (x, y),
            tolerance,
            (0, 0),
            canvas,
            |px, py| selection.covers(i64::from(px), i64::from(py), canvas.0, canvas.1),
            false,
        )
    }

    pub fn grow(self, radius: u32, width: u32, height: u32) -> Self {
        reshape(self, radius, width, height, Reshape::Grow)
    }

    pub fn shrink(self, radius: u32, width: u32, height: u32) -> Self {
        reshape(self, radius, width, height, Reshape::Shrink)
    }

    // The square averaging kernel has zero coverage outside the canvas. Its
    // denominator stays fixed there, so a bounded full selection also softens
    // at image edges. Unrestricted means no mask and stays unrestricted.
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
        if !matches!(self, Self::Mask(_) | Self::RoundedRect(_)) {
            return self;
        }
        if old.0 == 0 || old.1 == 0 || new.0 == 0 || new.1 == 0 {
            return Self::Empty;
        }
        let Some(bounds) = self.bounds(old.0, old.1) else {
            return Self::Empty;
        };
        let (x, width) = scaled_span(bounds.x, bounds.width, old.0, new.0);
        let (y, height) = scaled_span(bounds.y, bounds.height, old.1, new.1);
        if width == 0 || height == 0 {
            return Self::Empty;
        }
        let mut pixels = vec![0u8; width as usize * height as usize];
        for row in 0..height {
            for column in 0..width {
                let source_x =
                    bounds.x + (i64::from(column) * i64::from(bounds.width)) / i64::from(width);
                let source_y =
                    bounds.y + (i64::from(row) * i64::from(bounds.height)) / i64::from(height);
                pixels[(row * width + column) as usize] =
                    self.coverage(source_x, source_y, old.0, old.1);
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

// Keep arithmetic and stored dimensions representable even for an extreme
// zoomed-out pointer, without constraining the gesture to canvas edges.
fn drag_point(point: (i64, i64)) -> (i64, i64) {
    let limit = |value: i64| value.clamp(i64::from(i32::MIN) / 2, i64::from(i32::MAX) / 2);
    (limit(point.0), limit(point.1))
}

fn drag_rect(start: (i64, i64), end: (i64, i64)) -> PixelRect {
    let start = drag_point(start);
    let end = drag_point(end);
    PixelRect::new(
        start.0.min(end.0),
        start.1.min(end.1),
        (start.0.abs_diff(end.0) + 1) as u32,
        (start.1.abs_diff(end.1) + 1) as u32,
    )
}

fn square_drag_rect(start: (i64, i64), end: (i64, i64)) -> PixelRect {
    let (sx, sy) = drag_point(start);
    let end = drag_point(end);
    let dx = end.0 - sx;
    let dy = end.1 - sy;
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

fn shape_outline(
    rect: PixelRect,
    row_span: impl Fn(i64) -> Option<(i64, i64)>,
    column_span: impl Fn(i64) -> Option<(i64, i64)>,
) -> Vec<MaskEdge> {
    let mut edges = Vec::new();
    if rect.width == 0 || rect.height == 0 {
        return edges;
    }
    for y in rect.y..rect.bottom() {
        let Some(current) = row_span(y) else {
            continue;
        };
        for (start, end) in span_difference(current, row_span(y - 1)) {
            edges.push((start as i32, y as i32, end as i32, y as i32));
        }
        for (start, end) in span_difference(current, row_span(y + 1)) {
            edges.push((end as i32, (y + 1) as i32, start as i32, (y + 1) as i32));
        }
    }
    for x in rect.x..rect.right() {
        let Some(current) = column_span(x) else {
            continue;
        };
        for (start, end) in span_difference(current, column_span(x - 1)) {
            edges.push((x as i32, end as i32, x as i32, start as i32));
        }
        for (start, end) in span_difference(current, column_span(x + 1)) {
            edges.push(((x + 1) as i32, start as i32, (x + 1) as i32, end as i32));
        }
    }
    edges
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

fn polygons_selection(
    rings: &[&[(f64, f64)]],
    width: u32,
    height: u32,
    antialias: bool,
) -> Selection {
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
    let (min_x, max_x, min_y, max_y) = if antialias {
        (
            min_x.floor().clamp(0., f64::from(width)) as i32,
            max_x.ceil().clamp(0., f64::from(width)) as i32,
            min_y.floor().clamp(0., f64::from(height)) as i32,
            max_y.ceil().clamp(0., f64::from(height)) as i32,
        )
    } else {
        (
            scan_edge(min_x, width),
            scan_edge(max_x, width),
            scan_edge(min_y, height),
            scan_edge(max_y, height),
        )
    };
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
        let samples = if antialias { 4 } else { 1 };
        for sample in 0..samples {
            let scan = f64::from(y) + (f64::from(sample) + 0.5) / f64::from(samples);
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
                    if antialias {
                        let row = (y - min_y) as usize * buf_w as usize;
                        add_sample_span(&mut pixels[row..row + buf_w as usize], min_x, left, x);
                    } else {
                        fill_span(&mut pixels, min_x, min_y, buf_w, y, left, x);
                    }
                }
                wind = next;
            }
        }
    }
    if antialias {
        for count in &mut pixels {
            *count = ((u16::from(*count) * 255 + 8) / 16) as u8;
        }
    }
    finish_mask(min_x, min_y, buf_w, buf_h, pixels, width, height)
}

// Count four horizontal subpixel samples for each of the four vertical scans.
// Interior spans visit a pixel once per scan, without a 4x-resolution mask.
fn add_sample_span(row: &mut [u8], origin_x: i32, left: f64, right: f64) {
    let edge = |value: f64| {
        ((value - f64::from(origin_x)) * 4. - 0.5)
            .ceil()
            .clamp(0., (row.len() * 4) as f64) as usize
    };
    let (start, end) = (edge(left), edge(right));
    if start >= end {
        return;
    }
    let first = start / 4;
    for (index, count) in row[first..end.div_ceil(4)].iter_mut().enumerate() {
        let pixel = first + index;
        *count += (end.min((pixel + 1) * 4) - start.max(pixel * 4)) as u8;
    }
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
    seed: (u32, u32),
    tolerance: u8,
    offset: (i32, i32),
    canvas: (u32, u32),
    included: impl Fn(u32, u32) -> bool,
    antialias: bool,
) -> Selection {
    let (x, y) = seed;
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
    // 0 = unseen, 1 = queued/rejected, 2 = admitted. Antialiasing reuses
    // these states instead of allocating another source-sized coverage mask.
    let mut seen = vec![0u8; width as usize * height as usize];
    let mut stack = vec![(x, y)];
    seen[(y * width + x) as usize] = 1;
    while let Some((px, py)) = stack.pop() {
        if !included(px, py) || !within_tolerance(*image.get_pixel(px, py), target, tolerance) {
            continue;
        }
        seen[(py * width + px) as usize] = antialias::ACCEPTED;
        if !antialias
            && (local_x..local_x + bounds.width).contains(&px)
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
            if seen[index] != 0 {
                continue;
            }
            seen[index] = 1;
            stack.push((next_x as u32, next_y as u32));
        }
    }
    if antialias {
        antialias::apply(
            &mut pixels,
            &seen,
            (width, height),
            PixelRect::new(
                i64::from(local_x),
                i64::from(local_y),
                bounds.width,
                bounds.height,
            ),
            included,
        );
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
    // Fully transparent pixels have no visible RGB. Keep alpha comparisons,
    // but do not split a transparent region because its stored RGB differs.
    if pixel.0[3] == 0 && target.0[3] == 0 {
        return true;
    }
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
            source[(row * buf_w + column) as usize] = coverage;
        }
    }
    if matches!(kind, Reshape::Grow | Reshape::Shrink) {
        let grow = matches!(kind, Reshape::Grow);
        extrema_image(
            &mut source,
            (buf_w as usize, buf_h as usize),
            radius as usize,
            grow,
            0,
        );
        return finish_mask(left, top, buf_w, buf_h, source, width, height);
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
            output[row * columns + column] = ((sum + area / 2) / area).min(255) as u8;
        }
    }
    finish_mask(left, top, buf_w, buf_h, output, width, height)
}

fn extrema_image(source: &mut [u8], size: (usize, usize), radius: usize, grow: bool, outside: u8) {
    let (columns, rows) = size;
    let mut horizontal = vec![0; source.len()];
    for row in 0..rows {
        extrema_line(
            columns,
            radius,
            grow,
            outside,
            |column| source[row * columns + column],
            |column, value| horizontal[row * columns + column] = value,
        );
    }
    for column in 0..columns {
        extrema_line(
            rows,
            radius,
            grow,
            outside,
            |row| horizontal[row * columns + column],
            |row, value| source[row * columns + column] = value,
        );
    }
}

// A monotonic deque gives a running max/min in linear time, retaining soft
// coverage. Ordinary selection morphology uses zero outside coverage; boundary
// closing uses 255 for erosion so nonexistent source pixels do not reopen gaps.
fn extrema_line(
    length: usize,
    radius: usize,
    grow: bool,
    outside: u8,
    sample: impl Fn(usize) -> u8,
    mut store: impl FnMut(usize, u8),
) {
    let mut queue = std::collections::VecDeque::new();
    let mut added = 0;
    for center in 0..length {
        let end = center.saturating_add(radius).min(length - 1);
        while added <= end {
            let value = sample(added);
            while queue.back().is_some_and(|&at| {
                if grow {
                    sample(at) <= value
                } else {
                    sample(at) >= value
                }
            }) {
                queue.pop_back();
            }
            queue.push_back(added);
            added += 1;
        }
        let start = center.saturating_sub(radius);
        while queue.front().is_some_and(|&at| at < start) {
            queue.pop_front();
        }
        let inside = queue.front().map_or(outside, |&at| sample(at));
        let value = if center < radius || radius >= length - center {
            if grow {
                inside.max(outside)
            } else {
                inside.min(outside)
            }
        } else {
            inside
        };
        store(center, value);
    }
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
    fn wand_antialias_smooths_corners_without_blurring_straight_edges_or_crossing_components() {
        let select = |image: &RgbaImage, seed, aa| {
            Selection::flood_region_quality(
                image,
                seed,
                RegionMatch::default(),
                (0, 0),
                image.dimensions(),
                None,
                aa,
            )
        };
        let corner = RgbaImage::from_fn(3, 3, |x, y| {
            Rgba(if (x, y) == (1, 1) {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            })
        });
        let hard = select(&corner, (1, 1), false);
        let soft = select(&corner, (1, 1), true);
        assert_eq!(hard.coverage(1, 1, 3, 3), 255);
        assert_eq!(soft.coverage(1, 1, 3, 3), 191);
        assert_eq!(soft.coverage(1, 0, 3, 3), 0); // No feather-like halo.
        for size in [(7, 5), (7, 1), (1, 7)] {
            let image = RgbaImage::from_fn(size.0, size.1, |x, y| {
                Rgba(if (if size.0 == 1 { y } else { x }) < 3 {
                    [255; 4]
                } else {
                    [0, 0, 0, 255]
                })
            });
            assert_eq!(select(&image, (0, 0), true), select(&image, (0, 0), false));
        }
        let diagonal = RgbaImage::from_fn(5, 5, |x, y| {
            Rgba(if x == y { [255; 4] } else { [0, 0, 0, 255] })
        });
        let soft = select(&diagonal, (0, 0), true);
        for index in 1..5 {
            assert_eq!(soft.coverage(index, index, 5, 5), 0);
        }
        assert_eq!(select(&diagonal, (5, 0), true), Selection::Empty);
    }

    #[test]
    fn wand_antialias_is_clipped_after_source_space_reconstruction_and_keeps_transparent_matching()
    {
        let image = RgbaImage::from_fn(9, 9, |x, y| {
            Rgba(if x + y <= 8 { [255; 4] } else { [0, 0, 0, 255] })
        });
        let full = Selection::flood_region_quality(
            &image,
            (0, 0),
            RegionMatch::default(),
            (0, 0),
            (9, 9),
            None,
            true,
        );
        assert!((0..9).any(|y| (0..9).any(|x| (1..255).contains(&full.coverage(x, y, 9, 9)))));
        for offset in [(-3, -2), (2, 1), (-8, -8)] {
            let clipped = Selection::flood_region_quality(
                &image,
                (0, 0),
                RegionMatch::default(),
                offset,
                (6, 5),
                None,
                true,
            );
            for y in 0..5 {
                for x in 0..6 {
                    assert_eq!(
                        clipped.coverage(x, y, 6, 5),
                        full.coverage(x - i64::from(offset.0), y - i64::from(offset.1), 9, 9)
                    );
                }
            }
            assert_eq!(clipped.coverage(-1, 0, 6, 5), 0);
            assert_eq!(clipped.coverage(6, 0, 6, 5), 0);
        }
        let clear = RgbaImage::from_fn(5, 3, |x, y| Rgba([x as u8 * 40, y as u8 * 70, 120, 0]));
        assert_eq!(
            Selection::flood_region_quality(
                &clear,
                (0, 0),
                RegionMatch::default(),
                (0, 0),
                (5, 3),
                None,
                true
            ),
            Selection::Full
        );
        let alpha_edge = RgbaImage::from_fn(4, 1, |x, _| {
            Rgba([x as u8 * 20, 0, 0, if x < 2 { 0 } else { 1 }])
        });
        for aa in [false, true] {
            let region = Selection::flood_region_quality(
                &alpha_edge,
                (0, 0),
                RegionMatch::default(),
                (0, 0),
                (4, 1),
                None,
                aa,
            );
            assert_eq!(region.coverage(1, 0, 4, 1), 255);
            assert_eq!(region.coverage(2, 0, 4, 1), 0);
        }
    }

    #[test]
    fn wand_antialias_respects_sealed_gaps_and_strict_selection_limits() {
        let image = RgbaImage::from_fn(9, 9, |x, y| {
            let wall = ((x == 1 || x == 7) && (1..=7).contains(&y))
                || ((y == 1 || y == 7) && (1..=7).contains(&x));
            Rgba(if wall && (x, y) != (4, 1) {
                [0, 0, 0, 255]
            } else {
                [255; 4]
            })
        });
        let matching = RegionMatch {
            tolerance: 0,
            gap_radius: 1,
        };
        let soft =
            Selection::flood_region_quality(&image, (4, 4), matching, (0, 0), (9, 9), None, true);
        assert_eq!(soft.coverage(4, 4, 9, 9), 255);
        for point in [(0, 0), (4, 0), (4, 1), (1, 4)] {
            assert_eq!(soft.coverage(point.0, point.1, 9, 9), 0);
        }
        let uniform = RgbaImage::from_pixel(5, 5, Rgba([255; 4]));
        let limit = Selection::Inverse(PixelRect::new(2, 2, 1, 1));
        let soft = Selection::flood_region_quality(
            &uniform,
            (0, 0),
            RegionMatch::default(),
            (0, 0),
            (5, 5),
            Some(&limit),
            true,
        );
        assert_eq!(soft.coverage(2, 2, 5, 5), 0);
        assert_eq!(soft.coverage(0, 0, 5, 5), 255);
    }

    #[test]
    fn gap_radius_seals_small_breaks_but_preserves_larger_openings_and_original_ink() {
        let line_art = |gap: std::ops::RangeInclusive<u32>| {
            RgbaImage::from_fn(9, 9, |x, y| {
                let wall = ((x == 1 || x == 7) && (1..=7).contains(&y))
                    || ((y == 1 || y == 7) && (1..=7).contains(&x));
                Rgba(if wall && !(y == 1 && gap.contains(&x)) {
                    [0, 0, 0, 255]
                } else {
                    [255; 4]
                })
            })
        };
        let select = |image: &RgbaImage, radius| {
            Selection::flood_region(
                image,
                (4, 4),
                RegionMatch {
                    tolerance: 0,
                    gap_radius: radius,
                },
                (0, 0),
                (9, 9),
                None,
            )
        };
        let narrow = line_art(4..=4);
        assert!(select(&narrow, 0).covers(0, 0, 9, 9));
        let sealed = select(&narrow, 1);
        assert!(sealed.covers(4, 4, 9, 9));
        assert!(!sealed.covers(0, 0, 9, 9));
        assert!(!sealed.covers(4, 1, 9, 9));
        assert!(!sealed.covers(1, 4, 9, 9));
        let wide = line_art(3..=5);
        assert!(select(&wide, 1).covers(4, 0, 9, 9));
        assert!(select(&wide, 2).covers(4, 4, 9, 9));
        assert!(!select(&wide, 2).covers(4, 0, 9, 9));
        assert_eq!(
            select(&narrow, 0),
            Selection::flood(&narrow, 4, 4, 0, (0, 0), (9, 9))
        );
        // Disabled gap closure does not even allocate a boundary mask.
        assert!(
            RegionMatch::default()
                .gap_barrier(&narrow, (4, 4))
                .is_none()
        );
    }

    #[test]
    fn gap_flood_honors_canvas_space_limits_and_rejects_invalid_seeds() {
        let image = RgbaImage::from_pixel(5, 5, Rgba([255; 4]));
        let matching = RegionMatch {
            tolerance: 0,
            gap_radius: u8::MAX,
        };
        let limit = Selection::Inverse(PixelRect::new(4, 0, 1, 7));
        let region =
            Selection::flood_region(&image, (0, 2), matching, (2, 1), (9, 7), Some(&limit));
        assert!(region.covers(2, 3, 9, 7));
        assert!(!region.covers(4, 3, 9, 7));
        assert!(!region.covers(5, 3, 9, 7));
        assert_eq!(
            region,
            Selection::flood_region(
                &image,
                (0, 2),
                RegionMatch {
                    gap_radius: MAX_GAP_RADIUS,
                    ..matching
                },
                (2, 1),
                (9, 7),
                Some(&limit)
            )
        );
        assert_eq!(
            Selection::flood_region(&image, (2, 2), matching, (2, 1), (9, 7), Some(&limit)),
            Selection::Empty
        );
        assert_eq!(
            Selection::flood_region(&image, (5, 5), matching, (0, 0), (5, 5), None),
            Selection::Empty
        );
        assert_eq!(
            Selection::flood_region(
                &RgbaImage::new(0, 0),
                (0, 0),
                matching,
                (0, 0),
                (5, 5),
                None
            ),
            Selection::Empty
        );
    }

    #[test]
    fn gap_aware_flood_matches_an_independent_binary_closing_and_connectivity_oracle() {
        let mut random = 17u32;
        for (width, height) in [(1, 1), (1, 6), (6, 1), (5, 7), (8, 6)] {
            for radius in 0..=2u8 {
                for _ in 0..8 {
                    let image = RgbaImage::from_fn(width, height, |_, _| {
                        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                        match random % 4 {
                            0 => Rgba([3, 20, 70, 0]),
                            1 => Rgba([210, 9, 35, 0]),
                            2 => Rgba([250, 248, 246, 255]),
                            _ => Rgba([255; 4]),
                        }
                    });
                    let seed = (width / 2, height / 2);
                    let target = image.get_pixel(seed.0, seed.1).0;
                    for tolerance in [0, 8, 255] {
                        let count = (width * height) as usize;
                        let matching: Vec<_> = image
                            .pixels()
                            .map(|pixel| {
                                (pixel[3] == 0 && target[3] == 0)
                                    || (0..4).all(|channel| {
                                        pixel[channel].abs_diff(target[channel]) <= tolerance
                                    })
                            })
                            .collect();
                        let mut dilated = vec![false; count];
                        let mut closed = vec![false; count];
                        if radius > 0 {
                            let r = i32::from(radius);
                            for y in 0..height as i32 {
                                for x in 0..width as i32 {
                                    dilated[(y as u32 * width + x as u32) as usize] =
                                        (-r..=r).any(|dy| {
                                            (-r..=r).any(|dx| {
                                                let (nx, ny) = (x + dx, y + dy);
                                                nx >= 0
                                                    && ny >= 0
                                                    && nx < width as i32
                                                    && ny < height as i32
                                                    && !matching
                                                        [(ny as u32 * width + nx as u32) as usize]
                                            })
                                        });
                                }
                            }
                            for y in 0..height as i32 {
                                for x in 0..width as i32 {
                                    closed[(y as u32 * width + x as u32) as usize] =
                                        (-r..=r).all(|dy| {
                                            (-r..=r).all(|dx| {
                                                let (nx, ny) = (x + dx, y + dy);
                                                nx < 0
                                                    || ny < 0
                                                    || nx >= width as i32
                                                    || ny >= height as i32
                                                    || dilated
                                                        [(ny as u32 * width + nx as u32) as usize]
                                            })
                                        });
                                }
                            }
                        }
                        let mut reached = vec![false; count];
                        let mut queue = std::collections::VecDeque::from([seed]);
                        while let Some((x, y)) = queue.pop_front() {
                            let index = (y * width + x) as usize;
                            if reached[index] || !matching[index] || closed[index] {
                                continue;
                            }
                            reached[index] = true;
                            for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                                if nx >= 0 && ny >= 0 && nx < width as i32 && ny < height as i32 {
                                    queue.push_back((nx as u32, ny as u32));
                                }
                            }
                        }
                        // Include clipping and off-canvas traversal in the comparison.
                        let offset = (-1, 1);
                        let canvas = (width + 1, height + 2);
                        let selected = Selection::flood_region(
                            &image,
                            seed,
                            RegionMatch {
                                tolerance,
                                gap_radius: radius,
                            },
                            offset,
                            canvas,
                            None,
                        );
                        for y in 0..canvas.1 {
                            for x in 0..canvas.0 {
                                let (sx, sy) = (x as i32 - offset.0, y as i32 - offset.1);
                                let expected = sx >= 0
                                    && sy >= 0
                                    && sx < width as i32
                                    && sy < height as i32
                                    && reached[(sy as u32 * width + sx as u32) as usize];
                                assert_eq!(
                                    selected.covers(i64::from(x), i64::from(y), canvas.0, canvas.1),
                                    expected,
                                    "{width}x{height}, radius {radius}, tolerance {tolerance}, ({x}, {y})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn antialiased_curves_match_subpixel_coverage_and_keep_geometry_when_clipped() {
        for rect in [
            PixelRect::new(0, 0, 8, 8),
            PixelRect::new(1, 2, 9, 3),
            PixelRect::new(-2, -2, 8, 8),
            PixelRect::new(1, 1, 1, 7),
            PixelRect::new(0, 0, 100, 2),
            PixelRect::new(0, 0, 2, 100),
        ] {
            for rounded in [false, true] {
                let source = if rounded {
                    Selection::RoundedRect(RoundedRectSelection {
                        rect,
                        radius: 3,
                        antialias: true,
                        outline: Arc::default(),
                    })
                } else {
                    Selection::Ellipse(EllipseSelection {
                        rect,
                        antialias: true,
                        outline: Arc::default(),
                    })
                };
                let clipped = source.clone().clip_shape(rect, 12, 12);
                for y in 0..12 {
                    for x in 0..12 {
                        let expected = if !contains(rect, x, y) {
                            0
                        } else {
                            sample_coverage(x, y, |x, y| match &source {
                                Selection::Ellipse(shape) => shape.contains_point(x, y),
                                Selection::RoundedRect(shape) => shape.contains_point(x, y),
                                _ => unreachable!(),
                            })
                        };
                        assert_eq!(
                            source.coverage(x, y, 12, 12),
                            expected,
                            "{rect:?} ({x},{y}) rounded {rounded}"
                        );
                        assert_eq!(clipped.coverage(x, y, 12, 12), expected);
                        assert_eq!(clipped.covers(x, y, 12, 12), expected > 0);
                        let (row, column) = match &source {
                            Selection::Ellipse(shape) => (shape.row_span(y), shape.column_span(x)),
                            Selection::RoundedRect(shape) => {
                                (shape.row_span(y), shape.column_span(x))
                            }
                            _ => unreachable!(),
                        };
                        assert_eq!(row.is_some_and(|(a, b)| x >= a && x < b), expected > 0);
                        assert_eq!(column.is_some_and(|(a, b)| y >= a && y < b), expected > 0);
                    }
                }
            }
        }
        let hard = Selection::ellipse_rect(PixelRect::new(0, 0, 8, 8), 8, 8);
        let soft = hard.clone().with_antialias(true, 8, 8);
        assert_eq!(hard.coverage(1, 0, 8, 8), 0);
        assert!((1..255).contains(&soft.coverage(1, 0, 8, 8)));
        assert_eq!(soft.coverage(4, 4, 8, 8), 255);
        assert_eq!(soft.clone().with_antialias(false, 8, 8), hard);
        assert!(soft.clone().rebase(1, 1, 6, 6).coverage(0, 0, 6, 6) > 0);
        let moved = soft.moved_box(PixelRect::new(2, 2, 8, 8), 12, 12);
        assert!(moved.is_antialiased());
        assert!((1..255).contains(&moved.coverage(3, 2, 12, 12)));
    }

    #[test]
    fn fully_transparent_regions_ignore_hidden_rgb_but_keep_alpha_boundaries() {
        let image = RgbaImage::from_fn(4, 1, |x, _| match x {
            0 => Rgba([255, 0, 0, 0]),
            1 => Rgba([0, 255, 0, 0]),
            2 => Rgba([0, 0, 255, 1]),
            _ => Rgba([255, 0, 0, 0]),
        });
        let selected = Selection::flood(&image, 0, 0, 0, (0, 0), (4, 1));
        assert_eq!(selected.coverage(0, 0, 4, 1), 255);
        assert_eq!(selected.coverage(1, 0, 4, 1), 255);
        assert_eq!(selected.coverage(2, 0, 4, 1), 0);
        assert_eq!(selected.coverage(3, 0, 4, 1), 0);
        assert!(!within_tolerance(Rgba([1, 2, 3, 1]), Rgba([3, 2, 1, 1]), 0));
        assert!(within_tolerance(Rgba([1, 2, 3, 1]), Rgba([3, 2, 1, 1]), 2));
    }

    #[test]
    fn soft_morphology_matches_min_max_coverage_including_canvas_edges() {
        let (width, height) = (7, 6);
        let pixels: Vec<u8> = (0..width * height)
            .map(|index| ((index * 73 + 31) % 256) as u8)
            .collect();
        let selection = finish_mask(0, 0, width, height, pixels, width, height);
        for radius in [0, 1, 2, 8] {
            for grow in [false, true] {
                let actual = if grow {
                    selection.clone().grow(radius, width, height)
                } else {
                    selection.clone().shrink(radius, width, height)
                };
                for y in 0..i64::from(height) {
                    for x in 0..i64::from(width) {
                        let mut expected = if grow { 0 } else { 255 };
                        for dy in -i64::from(radius)..=i64::from(radius) {
                            for dx in -i64::from(radius)..=i64::from(radius) {
                                let value = selection.coverage(x + dx, y + dy, width, height);
                                expected = if grow {
                                    expected.max(value)
                                } else {
                                    expected.min(value)
                                };
                            }
                        }
                        assert_eq!(
                            actual.coverage(x, y, width, height),
                            expected,
                            "({x},{y}), radius {radius}, grow {grow}"
                        );
                    }
                }
            }
        }
        let feathered = Selection::Rect(PixelRect::new(2, 2, 3, 3)).feather(1, 8, 8);
        for edited in [feathered.clone().grow(1, 8, 8), feathered.shrink(1, 8, 8)] {
            assert!(
                (0..8).any(|y| (0..8).any(|x| (1..255).contains(&edited.coverage(x, y, 8, 8))))
            );
        }
    }

    #[test]
    fn rectangle_clips_to_the_canvas_and_collapses_when_it_covers_it() {
        assert_eq!(
            Selection::rectangle((1, 2), (4, 6), 10, 10),
            Selection::Rect(PixelRect::new(1, 2, 4, 5))
        );
        assert_eq!(Selection::rectangle((0, 0), (9, 9), 10, 8), Selection::Full);
    }

    #[test]
    fn shape_drags_can_start_outside_and_keep_their_geometry_when_clipped() {
        assert_eq!(
            Selection::rectangle((-4, -3), (3, 4), 8, 8),
            Selection::Rect(PixelRect::new(0, 0, 4, 5)),
        );
        let oval = Selection::ellipse((-4, -4), (3, 3), 8, 8);
        let backwards = Selection::ellipse((3, 3), (-4, -4), 8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let inside = (x as f64 + 0.5).powi(2) + (y as f64 + 0.5).powi(2) <= 16.0;
                assert_eq!(oval.covers(x, y, 8, 8), inside);
                assert_eq!(backwards.covers(x, y, 8, 8), inside);
            }
        }
        assert!(!oval.covers(-1, 0, 8, 8));
        assert_eq!(
            Selection::ellipse((-8, -8), (-2, -2), 8, 8),
            Selection::Empty
        );
        let circle = Selection::circle((-2, 1), (4, 2), 8, 8);
        assert!(circle.covers(1, 4, 8, 8));
        assert!(!circle.covers(4, 1, 8, 8));
        assert_eq!(
            Selection::rectangle((i64::MIN, i64::MIN), (i64::MAX, i64::MAX), 8, 8),
            Selection::Full,
        );
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
    fn rounded_rect_follows_its_corner_radius() {
        let rect = PixelRect::new(1, 1, 6, 6);
        assert_eq!(
            Selection::rounded_rect_rect(rect, 0, 20, 20),
            Selection::Rect(rect)
        );
        let rounded = Selection::rounded_rect_rect(rect, 2, 20, 20);
        assert!(rounded.covers(1, 3, 20, 20));
        assert!(!rounded.covers(1, 1, 20, 20));
        assert!(rounded.covers(3, 3, 20, 20));
        assert_eq!(rounded.crop_bounds(20, 20), Some(rect));
    }

    #[test]
    fn rounded_rect_keeps_its_radius_when_moved() {
        let shape = Selection::rounded_rect_rect(PixelRect::new(1, 1, 6, 6), 2, 10, 10);
        assert_eq!(
            shape.clone().moved_box(PixelRect::new(2, 2, 6, 6), 10, 10),
            Selection::rounded_rect_rect(PixelRect::new(2, 2, 6, 6), 2, 10, 10)
        );
        assert_eq!(
            shape.rebase(1, 1, 9, 9),
            Selection::rounded_rect_rect(PixelRect::new(0, 0, 6, 6), 2, 9, 9)
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
    fn antialiased_polygon_spans_match_subpixel_winding_for_clips_holes_and_crossings() {
        fn inside(rings: &[Vec<(f64, f64)>], x: f64, y: f64) -> bool {
            let mut winding = 0;
            for ring in rings {
                for (a, b) in ring
                    .iter()
                    .zip(ring.iter().cycle().skip(1))
                    .take(ring.len())
                {
                    let side = (b.0 - a.0) * (y - a.1) - (x - a.0) * (b.1 - a.1);
                    if a.1 <= y && b.1 > y && side > 0. {
                        winding += 1;
                    }
                    if a.1 > y && b.1 <= y && side < 0. {
                        winding -= 1;
                    }
                }
            }
            winding != 0
        }
        let cases = [
            vec![vec![(0.21, 0.32), (6.43, 1.27), (1.74, 7.86)]],
            vec![vec![
                (-2.21, 2.32),
                (5.43, -1.27),
                (7.74, 5.86),
                (1.17, 3.24),
            ]],
            vec![vec![(0.21, 0.32), (7.43, 7.27), (0.74, 7.86), (7.17, 0.24)]],
            vec![
                vec![(0.21, 0.23), (7.79, 0.23), (7.79, 7.77), (0.21, 7.77)],
                vec![(2.21, 2.23), (2.21, 5.77), (5.79, 5.77), (5.79, 2.23)],
            ],
        ];
        for rings in cases {
            let refs: Vec<_> = rings.iter().map(Vec::as_slice).collect();
            let selection = polygons_selection(&refs, 8, 8, true);
            let reversed: Vec<Vec<_>> = rings
                .iter()
                .map(|ring| ring.iter().copied().rev().collect())
                .collect();
            let reversed_refs: Vec<_> = reversed.iter().map(Vec::as_slice).collect();
            assert_eq!(selection, polygons_selection(&reversed_refs, 8, 8, true));
            for y in 0..8 {
                for x in 0..8 {
                    assert_eq!(
                        selection.coverage(x, y, 8, 8),
                        sample_coverage(x, y, |x, y| inside(&rings, x, y)),
                        "pixel {x},{y} in {rings:?}"
                    );
                }
            }
            assert_eq!(selection.coverage(-1, 2, 8, 8), 0);
            assert_eq!(selection.coverage(8, 2, 8, 8), 0);
        }
    }

    #[test]
    fn polygon_antialiasing_keeps_thin_shapes_and_hard_mode_without_blurring_aligned_edges() {
        let thin = [(0.1, 0.1), (0.3, 0.1), (0.3, 3.2), (0.1, 3.2)];
        assert_eq!(
            Selection::polygon_quality(&thin, 4, 4, false),
            Selection::Empty
        );
        assert_eq!(
            Selection::polygon_quality(&thin, 4, 4, false),
            Selection::polygon(&thin, 4, 4)
        );
        let soft = Selection::polygon_quality(&thin, 4, 4, true);
        assert_eq!(soft.coverage(0, 1, 4, 4), 64);
        assert_eq!(soft.coverage(1, 1, 4, 4), 0);
        let aligned = [(1., 1.), (4., 1.), (4., 4.), (1., 4.)];
        assert_eq!(
            Selection::polygon_quality(&aligned, 6, 6, true),
            Selection::Rect(PixelRect::new(1, 1, 3, 3))
        );
        for points in [
            vec![],
            vec![(1., 1.)],
            vec![(1., 1.), (2., 2.), (3., 3.)],
            vec![(0., 0.), (f64::NAN, 1.), (2., 2.)],
        ] {
            assert_eq!(
                Selection::polygon_quality(&points, 4, 4, true),
                Selection::Empty
            );
        }
        let distant = [(-1e12, -1e12), (4., -1e12), (4., 4.), (-1e12, 4.)];
        assert_eq!(
            Selection::polygon_quality(&distant, 4, 4, true),
            Selection::Full
        );
        assert_eq!(
            Selection::polygon_quality(&aligned, 0, 6, true),
            Selection::Empty
        );
    }

    #[test]
    fn feathering_matches_a_fixed_kernel_with_zero_coverage_beyond_the_canvas() {
        let (width, height) = (7, 6);
        let selections = [
            Selection::Full,
            Selection::Rect(PixelRect::new(0, 0, 2, 3)),
            Selection::ellipse_rect_quality(PixelRect::new(-2, 1, 7, 7), width, height, true),
            Selection::polygon_quality(&[(-1.2, 0.2), (5.4, 1.2), (1.7, 5.8)], width, height, true),
        ];
        for source in selections {
            assert_eq!(source.clone().feather(0, width, height), source);
            for radius in [1, 2, 3, 128] {
                let soft = source.clone().feather(radius, width, height);
                let area = (u64::from(radius) * 2 + 1).pow(2);
                for y in 0..height {
                    for x in 0..width {
                        let mut sum = 0u64;
                        for sy in 0..height {
                            for sx in 0..width {
                                if sx.abs_diff(x) <= radius && sy.abs_diff(y) <= radius {
                                    sum += u64::from(source.coverage(
                                        i64::from(sx),
                                        i64::from(sy),
                                        width,
                                        height,
                                    ));
                                }
                            }
                        }
                        assert_eq!(
                            soft.coverage(i64::from(x), i64::from(y), width, height),
                            ((sum + area / 2) / area) as u8
                        );
                    }
                }
                assert_eq!(soft.coverage(-1, 0, width, height), 0);
            }
        }
        let full = Selection::Full.feather(1, 3, 3);
        assert_eq!(full.coverage(0, 0, 3, 3), 113);
        assert_eq!(full.coverage(1, 1, 3, 3), 255);
        assert_eq!(
            Selection::Unrestricted.feather(3, 3, 3),
            Selection::Unrestricted
        );
        assert_eq!(Selection::Empty.feather(3, 3, 3), Selection::Empty);
    }

    #[test]
    fn soft_polygon_and_wand_coverage_is_shared_by_fill_and_clear() {
        use crate::document::{DocumentContent, FillMode, FillRequest};
        let selection =
            Selection::polygon_quality(&[(0.21, 0.32), (6.43, 1.27), (1.74, 7.86)], 8, 8, true)
                .feather(1, 8, 8);
        let wand_image = RgbaImage::from_fn(8, 8, |x, y| {
            Rgba(if x + y < 8 { [255; 4] } else { [0, 0, 0, 255] })
        });
        let wand = Selection::flood_region_quality(
            &wand_image,
            (0, 0),
            RegionMatch::default(),
            (0, 0),
            (8, 8),
            None,
            true,
        );
        for selection in [selection, wand] {
            let mut filled = DocumentContent::single_layer("fill".into(), RgbaImage::new(8, 8));
            let fill_layer = filled.top_layer_id();
            filled.fill_bucket(
                fill_layer,
                1,
                1,
                Rgba([255; 4]),
                &selection,
                FillRequest {
                    mode: FillMode::Selection,
                    sample_all: false,
                    tolerance: 0,
                    gap_radius: 0,
                },
            );
            let mut cleared = DocumentContent::single_layer(
                "clear".into(),
                RgbaImage::from_pixel(8, 8, Rgba([255; 4])),
            );
            let clear_layer = cleared.top_layer_id();
            cleared.clear(clear_layer, &selection);
            for y in 0..8 {
                for x in 0..8 {
                    let coverage = selection.coverage(i64::from(x), i64::from(y), 8, 8);
                    assert_eq!(
                        filled.layer(fill_layer).unwrap().pixels.get_pixel(x, y)[3],
                        coverage
                    );
                    assert_eq!(
                        cleared.layer(clear_layer).unwrap().pixels.get_pixel(x, y)[3],
                        255 - coverage
                    );
                }
            }
        }
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
    fn bounded_morphology_matches_direct_neighborhoods_on_thin_canvases() {
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
                            let mut minimum = 255;
                            let mut maximum = 0;
                            for dy in -i64::from(radius)..=i64::from(radius) {
                                for dx in -i64::from(radius)..=i64::from(radius) {
                                    let value = source.coverage(x + dx, y + dy, width, height);
                                    sum += u64::from(value);
                                    minimum = minimum.min(value);
                                    maximum = maximum.max(value);
                                }
                            }
                            expected.push(match kind {
                                Reshape::Grow => maximum,
                                Reshape::Shrink => minimum,
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

    #[test]
    fn cropping_and_canvas_shrinking_clip_curves_without_reshaping_them() {
        for selection in [
            Selection::ellipse_rect(PixelRect::new(0, 0, 8, 8), 8, 8),
            Selection::rounded_rect_rect(PixelRect::new(0, 0, 8, 8), 3, 8, 8),
        ] {
            let cropped = selection.clone().rebase(4, 0, 4, 8);
            let resized = selection.clone().resize_canvas((8, 8), (4, 8), (-4, 0));
            for y in 0..8 {
                for x in 0..4 {
                    let expected = selection.coverage(x + 4, y, 8, 8);
                    assert_eq!(cropped.coverage(x, y, 4, 8), expected);
                    assert_eq!(resized.coverage(x, y, 4, 8), expected);
                }
            }
            // Clipped masks still have a closed contour for Path/Stroke Selection.
            assert!(
                cropped
                    .stroke_polylines(4, 8)
                    .iter()
                    .all(|line| line.first() == line.last())
            );
        }
    }

    #[test]
    fn rounded_selection_scaling_scales_corner_coverage_on_both_axes() {
        let original = Selection::rounded_rect_rect(PixelRect::new(1, 1, 6, 6), 3, 8, 8);
        for new in [(16, 16), (16, 24)] {
            let scaled = original.clone().scale_mask((8, 8), new);
            for y in 0..new.1 {
                for x in 0..new.0 {
                    assert_eq!(
                        scaled.coverage(x as i64, y as i64, new.0, new.1),
                        original.coverage((x * 8 / new.0) as i64, (y * 8 / new.1) as i64, 8, 8),
                    );
                }
            }
        }
    }
}
