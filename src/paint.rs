use std::collections::{BTreeSet, HashMap};

use image::{Rgba, RgbaImage};

use crate::filters;
use crate::preview::PixelRect;

pub const STROKE_TILE: u32 = 64;
pub type TileKey = (u32, u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brush {
    pub diameter: u32,
    pub spacing: u8,
    pub hardness: u8,
    pub opacity: u8,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            diameter: 16,
            spacing: 10,
            hardness: 100,
            opacity: 100,
        }
    }
}

impl Brush {
    pub fn clamped(self) -> Self {
        Self {
            diameter: self.diameter.clamp(1, 500),
            spacing: self.spacing.clamp(1, 100),
            hardness: self.hardness.min(100),
            opacity: self.opacity.min(100),
        }
    }

    pub fn spacing_px(self) -> f64 {
        let brush = self.clamped();
        (f64::from(brush.diameter) * f64::from(brush.spacing) / 100.0).max(1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeKind {
    Pencil,
    Paint,
    Erase,
    Blur,
    Sharpen,
    Clone { dx: i64, dy: i64 },
}

#[derive(Clone, Copy)]
struct PlotPoint {
    x: f64,
    y: f64,
}

struct Plotter {
    spacing: f64,
    from: Option<PlotPoint>,
    along: f64,
    last_stamp: Option<PlotPoint>,
}

impl Plotter {
    fn new(spacing: f64) -> Self {
        Self {
            spacing: spacing.max(1.0),
            from: None,
            along: 0.0,
            last_stamp: None,
        }
    }

    fn restart(&mut self) {
        self.from = None;
        self.along = 0.0;
        self.last_stamp = None;
    }

    fn push(&mut self, point: PlotPoint, clip: Option<(f64, f64)>) -> Vec<PlotPoint> {
        let Some(from) = self.from.replace(point) else {
            self.last_stamp = Some(point);
            return vec![point];
        };
        let dx = point.x - from.x;
        let dy = point.y - from.y;
        let len = dx.hypot(dy);
        if len < 1e-6 || !len.is_finite() {
            return Vec::new();
        }

        let first = self.spacing - self.along;
        self.along = (self.along + len).rem_euclid(self.spacing);
        if self.along < 1e-8 || self.spacing - self.along < 1e-8 {
            self.along = 0.0;
        }

        let (start, end) = clip.unwrap_or((0.0, 1.0));
        let skip = ((start * len - first) / self.spacing).ceil().max(0.0);
        let mut distance = first + skip * self.spacing;
        let last = end * len + 1e-9;
        let mut stamps = Vec::new();
        while distance <= last && start <= end {
            let t = (distance / len).min(1.0);
            stamps.push(PlotPoint {
                x: from.x + dx * t,
                y: from.y + dy * t,
            });
            distance += self.spacing;
        }
        if let Some(stamp) = stamps.last() {
            self.last_stamp = Some(*stamp);
        }
        stamps
    }
}

fn clip_segment(from: PlotPoint, to: PlotPoint, radius: f64, size: (u32, u32)) -> (f64, f64) {
    let mut start: f64 = 0.0;
    let mut end: f64 = 1.0;
    for (origin, dest, edge) in [(from.x, to.x, size.0), (from.y, to.y, size.1)] {
        let low = -radius;
        let high = f64::from(edge) + radius;
        let delta = dest - origin;
        if delta.abs() < f64::EPSILON {
            if !(low..=high).contains(&origin) {
                return (1.0, 0.0);
            }
            continue;
        }
        let a = (low - origin) / delta;
        let b = (high - origin) / delta;
        start = start.max(a.min(b));
        end = end.min(a.max(b));
    }
    (start, end)
}

#[cfg(test)]
fn plot_path(points: &[(f64, f64)], spacing: f64) -> Vec<(f64, f64)> {
    let mut plotter = Plotter::new(spacing);
    let mut stamps = Vec::new();
    for &(x, y) in points {
        stamps.extend(
            plotter
                .push(PlotPoint { x, y }, None)
                .into_iter()
                .map(|point| (point.x, point.y)),
        );
    }
    stamps
}

pub struct Stroke {
    kind: StrokeKind,
    color: Rgba<u8>,
    brush: Brush,
    plotter: Plotter,
    tiles: HashMap<TileKey, RgbaImage>,
    pending: BTreeSet<TileKey>,
}

impl Stroke {
    pub fn new(kind: StrokeKind, color: Rgba<u8>, brush: Brush) -> Self {
        let brush = brush.clamped();
        Self {
            kind,
            color,
            plotter: Plotter::new(brush.spacing_px()),
            brush,
            tiles: HashMap::new(),
            pending: BTreeSet::new(),
        }
    }

    pub fn changed(&self) -> bool {
        !self.tiles.is_empty()
    }

    pub fn take_pending_tiles(&mut self) -> BTreeSet<TileKey> {
        std::mem::take(&mut self.pending)
    }

    pub fn tile_bounds(&self, key: TileKey) -> Option<PixelRect> {
        let pixels = self.tiles.get(&key)?;
        Some(PixelRect::new(
            i64::from(key.0 * STROKE_TILE),
            i64::from(key.1 * STROKE_TILE),
            pixels.width(),
            pixels.height(),
        ))
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba<u8>> {
        let tile = self.tiles.get(&(x / STROKE_TILE, y / STROKE_TILE))?;
        Some(*tile.get_pixel(x % STROKE_TILE, y % STROKE_TILE))
    }

    pub fn sample(
        &mut self,
        x: f64,
        y: f64,
        source: &RgbaImage,
        coverage: impl Fn(i32, i32) -> u8,
    ) {
        self.sample_segment(x, y, source, coverage, false);
    }

    fn sample_segment(
        &mut self,
        x: f64,
        y: f64,
        source: &RgbaImage,
        coverage: impl Fn(i32, i32) -> u8,
        omit_endpoint: bool,
    ) {
        if !x.is_finite() || !y.is_finite() || self.brush.opacity == 0 {
            return;
        }
        let point = PlotPoint { x, y };
        let radius = f64::from(self.brush.diameter) / 2.0 + 1.0;
        let clip = self
            .plotter
            .from
            .map(|from| clip_segment(from, point, radius, source.dimensions()));
        for stamp in self.plotter.push(point, clip) {
            if !omit_endpoint || !same_point((stamp.x, stamp.y), (x, y)) {
                self.dab(stamp, source, &coverage);
            }
        }
    }

    pub fn finish(&mut self, source: &RgbaImage, coverage: impl Fn(i32, i32) -> u8) {
        if self.plotter.along > 1e-8
            && let Some(point) = self.plotter.from
        {
            if self.brush.diameter == 1
                && self.plotter.last_stamp.is_some_and(|last| {
                    last.x.floor() == point.x.floor() && last.y.floor() == point.y.floor()
                })
            {
                return;
            }
            self.dab(point, source, &coverage);
        }
    }

    // Samples one polyline in layer pixels and starts fresh, so the next contour is not
    // joined to this one. Coverage is full: a contour stroke is the boundary, so the
    // selection must not hide half of the brush. A repeated point is skipped. A closed
    // line meets its start and is not finished, or the join would be painted again.
    pub fn trace(&mut self, points: &[(f64, f64)], source: &RgbaImage) {
        self.plotter.restart();
        let mut end = points.len();
        while end > 1 && same_point(points[end - 1], points[end - 2]) {
            end -= 1;
        }
        let points = &points[..end];
        let closed = points.len() >= 2 && same_point(points[0], points[points.len() - 1]);
        let mut previous = None;
        for (index, &next) in points.iter().enumerate() {
            if previous.is_some_and(|prior| same_point(prior, next)) {
                continue;
            }
            self.sample_segment(
                next.0,
                next.1,
                source,
                |_, _| 255,
                closed && index + 1 == points.len(),
            );
            previous = Some(next);
        }
        if !closed {
            self.finish(source, |_, _| 255);
        }
    }

    fn dab(&mut self, point: PlotPoint, source: &RgbaImage, coverage: &impl Fn(i32, i32) -> u8) {
        let (mut x, mut y) = (point.x, point.y);
        let radius = f64::from(self.brush.diameter) / 2.0;
        let pencil = self.kind == StrokeKind::Pencil;
        if pencil || self.brush.diameter == 1 {
            let snap = |value: f64| {
                if self.brush.diameter.is_multiple_of(2) {
                    value.round()
                } else {
                    value.floor() + 0.5
                }
            };
            x = snap(x);
            y = snap(y);
        }
        let hardness = if pencil { 100 } else { self.brush.hardness };
        let min_x = ((x - radius).floor() as i32).max(0);
        let min_y = ((y - radius).floor() as i32).max(0);
        let max_x = ((x + radius).ceil() as i32).min(source.width() as i32);
        let max_y = ((y + radius).ceil() as i32).min(source.height() as i32);
        for py in min_y..max_y {
            for px in min_x..max_x {
                let falloff =
                    dab_falloff(px as f64 + 0.5 - x, py as f64 + 0.5 - y, radius, hardness);
                if falloff <= 0.0 {
                    continue;
                }
                let amount = falloff * f64::from(self.brush.opacity) / 100.0
                    * f64::from(coverage(px, py))
                    / 255.0;
                if amount <= 0.0 {
                    continue;
                }
                let (px, py) = (px as u32, py as u32);
                let before = self
                    .pixel(px, py)
                    .unwrap_or_else(|| *source.get_pixel(px, py));
                let after = match self.kind {
                    StrokeKind::Pencil | StrokeKind::Paint => {
                        source_over(before, self.color, amount)
                    }
                    StrokeKind::Erase => erase(before, amount),
                    StrokeKind::Blur => {
                        let coverage = (amount * 255.).round().clamp(0., 255.) as u8;
                        filters::blend(before, blur_pixel(source, px, py), coverage)
                    }
                    StrokeKind::Sharpen => {
                        let coverage = (amount * 255.).round().clamp(0., 255.) as u8;
                        filters::blend(before, sharpen_pixel(source, px, py), coverage)
                    }
                    StrokeKind::Clone { dx, dy } => {
                        let sx = i64::from(px) + dx;
                        let sy = i64::from(py) + dy;
                        if sx < 0
                            || sy < 0
                            || sx >= i64::from(source.width())
                            || sy >= i64::from(source.height())
                        {
                            continue;
                        }
                        source_over(before, *source.get_pixel(sx as u32, sy as u32), amount)
                    }
                };
                if before != after {
                    let key = (px / STROKE_TILE, py / STROKE_TILE);
                    let tile = self.tiles.entry(key).or_insert_with(|| {
                        let (x, y) = (key.0 * STROKE_TILE, key.1 * STROKE_TILE);
                        image::imageops::crop_imm(
                            source,
                            x,
                            y,
                            STROKE_TILE.min(source.width() - x),
                            STROKE_TILE.min(source.height() - y),
                        )
                        .to_image()
                    });
                    tile.put_pixel(px % STROKE_TILE, py % STROKE_TILE, after);
                    self.pending.insert(key);
                }
            }
        }
    }
}

pub fn apply_stroke(source: &RgbaImage, stroke: &Stroke) -> Option<(RgbaImage, Vec<PixelRect>)> {
    let mut image = None;
    let mut regions = Vec::new();
    for (&(column, row), tile) in &stroke.tiles {
        let (origin_x, origin_y) = (column * STROKE_TILE, row * STROKE_TILE);
        let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
        let (mut max_x, mut max_y) = (0, 0);
        for (x, y, pixel) in tile.enumerate_pixels() {
            let (x, y) = (origin_x + x, origin_y + y);
            if source.get_pixel(x, y) != pixel {
                image
                    .get_or_insert_with(|| source.clone())
                    .put_pixel(x, y, *pixel);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        if min_x <= max_x && min_y <= max_y {
            regions.push(PixelRect::new(
                i64::from(min_x),
                i64::from(min_y),
                max_x - min_x + 1,
                max_y - min_y + 1,
            ));
        }
    }
    Some((image?, regions))
}

fn blur_pixel(source: &RgbaImage, x: u32, y: u32) -> Rgba<u8> {
    let (width, height) = source.dimensions();
    if width == 0 || height == 0 {
        return Rgba([0, 0, 0, 0]);
    }
    let mut sum = [0.0f64; 4];
    let mut count = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let sx = (x as i32 + dx).clamp(0, width as i32 - 1) as u32;
            let sy = (y as i32 + dy).clamp(0, height as i32 - 1) as u32;
            let pixel = source.get_pixel(sx, sy).0;
            let alpha = f64::from(pixel[3]) / 255.0;
            sum[0] += f64::from(pixel[0]) * alpha;
            sum[1] += f64::from(pixel[1]) * alpha;
            sum[2] += f64::from(pixel[2]) * alpha;
            sum[3] += alpha;
            count += 1.0;
        }
    }
    if sum[3] <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let alpha = (sum[3] / count * 255.0).round().clamp(0.0, 255.0) as u8;
    let channel = |value: f64| (value / sum[3]).round().clamp(0.0, 255.0) as u8;
    Rgba([channel(sum[0]), channel(sum[1]), channel(sum[2]), alpha])
}

fn sharpen_pixel(source: &RgbaImage, x: u32, y: u32) -> Rgba<u8> {
    let before = *source.get_pixel(x, y);
    if before.0[3] == 0 {
        return before;
    }
    let soft = blur_pixel(source, x, y);
    let channel = |index: usize| {
        let original = f64::from(before.0[index]);
        let blurred = f64::from(soft.0[index]);
        (original + (original - blurred)).round().clamp(0.0, 255.0) as u8
    };
    Rgba([channel(0), channel(1), channel(2), before.0[3]])
}

fn dab_falloff(dx: f64, dy: f64, radius: f64, hardness: u8) -> f64 {
    if radius <= 0.0 {
        return 0.0;
    }
    let distance = dx.hypot(dy);
    if distance >= radius {
        return 0.0;
    }
    let solid = radius * f64::from(hardness) / 100.0;
    if distance <= solid || radius - solid <= 1e-6 {
        1.0
    } else {
        1.0 - (distance - solid) / (radius - solid)
    }
}

fn source_over(dst: Rgba<u8>, src: Rgba<u8>, amount: f64) -> Rgba<u8> {
    let src_a = f64::from(src.0[3]) / 255.0 * amount.clamp(0.0, 1.0);
    let dst_a = f64::from(dst.0[3]) / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let mix = |src_channel: u8, dst_channel: u8| {
        let value = (f64::from(src_channel) / 255.0 * src_a
            + f64::from(dst_channel) / 255.0 * dst_a * (1.0 - src_a))
            / out_a;
        (value * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Rgba([
        mix(src.0[0], dst.0[0]),
        mix(src.0[1], dst.0[1]),
        mix(src.0[2], dst.0[2]),
        (out_a * 255.0).round().clamp(0.0, 255.0) as u8,
    ])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientShape {
    Linear,
    Radial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientBlend {
    ForegroundToBackground,
    ForegroundToTransparent,
}

#[derive(Clone, Copy, Debug)]
pub struct Gradient {
    pub shape: GradientShape,
    pub blend: GradientBlend,
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub foreground: Rgba<u8>,
    pub background: Rgba<u8>,
}

pub fn gradient_color(gradient: &Gradient, x: f64, y: f64) -> Rgba<u8> {
    let t = gradient_t(gradient, x, y);
    let (start, end) = gradient_stops(gradient);
    if t <= 0.0 {
        return start;
    }
    if t >= 1.0 {
        return clear_if_empty(end);
    }
    clear_if_empty(Rgba([
        lerp_channel(start.0[0], end.0[0], t),
        lerp_channel(start.0[1], end.0[1], t),
        lerp_channel(start.0[2], end.0[2], t),
        lerp_channel(start.0[3], end.0[3], t),
    ]))
}

fn gradient_stops(gradient: &Gradient) -> (Rgba<u8>, Rgba<u8>) {
    match gradient.blend {
        GradientBlend::ForegroundToBackground => (gradient.foreground, gradient.background),
        GradientBlend::ForegroundToTransparent => (
            gradient.foreground,
            Rgba([
                gradient.foreground.0[0],
                gradient.foreground.0[1],
                gradient.foreground.0[2],
                0,
            ]),
        ),
    }
}

fn gradient_t(gradient: &Gradient, x: f64, y: f64) -> f64 {
    let dx = gradient.end.0 - gradient.start.0;
    let dy = gradient.end.1 - gradient.start.1;
    match gradient.shape {
        GradientShape::Linear => {
            let length = dx * dx + dy * dy;
            if length <= 1e-12 {
                0.0
            } else {
                (((x - gradient.start.0) * dx + (y - gradient.start.1) * dy) / length)
                    .clamp(0.0, 1.0)
            }
        }
        GradientShape::Radial => {
            let radius = dx.hypot(dy);
            if radius <= 1e-6 {
                0.0
            } else {
                ((x - gradient.start.0).hypot(y - gradient.start.1) / radius).clamp(0.0, 1.0)
            }
        }
    }
}

fn lerp_channel(start: u8, end: u8, t: f64) -> u8 {
    (f64::from(start) + (f64::from(end) - f64::from(start)) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn clear_if_empty(color: Rgba<u8>) -> Rgba<u8> {
    if color.0[3] == 0 {
        Rgba([0, 0, 0, 0])
    } else {
        color
    }
}

fn same_point(left: (f64, f64), right: (f64, f64)) -> bool {
    (left.0 - right.0).hypot(left.1 - right.1) < 1e-6
}

fn erase(pixel: Rgba<u8>, amount: f64) -> Rgba<u8> {
    let keep = 1.0 - amount.clamp(0.0, 1.0);
    let alpha = (f64::from(pixel.0[3]) * keep).round().clamp(0.0, 255.0) as u8;
    if alpha == 0 {
        Rgba([0, 0, 0, 0])
    } else {
        Rgba([pixel.0[0], pixel.0[1], pixel.0[2], alpha])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(points: &[(f64, f64)], expected: &[(f64, f64)]) {
        assert_eq!(points.len(), expected.len());
        for (point, target) in points.iter().zip(expected) {
            assert!((point.0 - target.0).abs() < 1e-6 && (point.1 - target.1).abs() < 1e-6);
        }
    }

    #[test]
    fn clone_stamp_copies_pixels_from_its_offset() {
        let mut source = RgbaImage::new(4, 1);
        source.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        source.put_pixel(3, 0, Rgba([0, 0, 255, 255]));
        let brush = Brush {
            diameter: 1,
            spacing: 100,
            hardness: 100,
            opacity: 100,
        };
        let mut stroke = Stroke::new(
            StrokeKind::Clone { dx: 3, dy: 0 },
            Rgba([0, 0, 0, 255]),
            brush,
        );
        stroke.sample(0.5, 0.5, &source, |_, _| 255);
        let cloned = apply_stroke(&source, &stroke).unwrap().0;
        assert_eq!(cloned.get_pixel(0, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn blur_and_sharpen_change_a_soft_impulse() {
        let mut source = RgbaImage::from_pixel(5, 5, Rgba([100, 100, 100, 255]));
        source.put_pixel(2, 2, Rgba([200, 200, 200, 255]));
        let brush = Brush {
            diameter: 5,
            spacing: 100,
            hardness: 100,
            opacity: 100,
        };
        let mut blur = Stroke::new(StrokeKind::Blur, Rgba([0, 0, 0, 255]), brush);
        blur.sample(2.5, 2.5, &source, |_, _| 255);
        let blurred = apply_stroke(&source, &blur).unwrap().0;
        assert!(blurred.get_pixel(2, 2).0[0] < 200);
        assert!(blurred.get_pixel(2, 1).0[0] > 100);

        let mut sharpen = Stroke::new(StrokeKind::Sharpen, Rgba([0, 0, 0, 255]), brush);
        sharpen.sample(2.5, 2.5, &source, |_, _| 255);
        let sharpened = apply_stroke(&source, &sharpen).unwrap().0;
        assert!(sharpened.get_pixel(2, 2).0[0] > 200);
        assert!(sharpened.get_pixel(2, 1).0[0] < 100);
    }

    #[test]
    fn fast_and_slow_pointer_samples_place_the_same_stamps() {
        let direct = plot_path(&[(0.0, 0.0), (30.0, 0.0)], 10.0);
        let mut slow = vec![(0.0, 0.0)];
        for step in 1..=30 {
            slow.push((step as f64, 0.0));
        }
        let stepped = plot_path(&slow, 10.0);
        let expected = [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0)];
        near(&direct, &expected);
        near(&stepped, &expected);

        let end = (30.0_f64, 40.0_f64);
        let length = end.0.hypot(end.1);
        let mut diagonal = vec![(0.0, 0.0)];
        for step in 1..=10 {
            let t = step as f64 / 10.0;
            diagonal.push((end.0 * t, end.1 * t));
        }
        let fine = plot_path(&diagonal, 10.0);
        let coarse = plot_path(&[(0.0, 0.0), end], 10.0);
        near(&fine, &coarse);
        assert_eq!(fine.len(), (length / 10.0).floor() as usize + 1);
    }

    fn paint_at(
        image: &RgbaImage,
        kind: StrokeKind,
        brush: Brush,
        points: &[(f64, f64)],
    ) -> Stroke {
        let mut stroke = Stroke::new(kind, Rgba([255, 0, 0, 255]), brush);
        for &(x, y) in points {
            stroke.sample(x, y, image, |_, _| 255);
        }
        stroke
    }

    #[test]
    fn paint_and_erase_honor_alpha_hardness_and_the_selection_edge() {
        let clear = RgbaImage::new(8, 4);
        let mut soft = Brush {
            diameter: 1,
            spacing: 100,
            hardness: 100,
            opacity: 50,
        };
        let once = paint_at(&clear, StrokeKind::Paint, soft, &[(0.5, 0.5)]);
        let pixel = once.pixel(0, 0).unwrap().0;
        assert_eq!(pixel[0], 255);
        assert!((120..=135).contains(&pixel[3]), "{pixel:?}");
        soft.diameter = 3;
        soft.spacing = 1;
        let twice = paint_at(&clear, StrokeKind::Paint, soft, &[(0.5, 0.5), (1.5, 0.5)]);
        let compounded = twice.pixel(0, 0).unwrap().0[3];
        assert!(compounded > pixel[3] && compounded < 255, "{compounded}");
        soft.diameter = 1;

        soft.opacity = 100;
        soft.diameter = 5;
        soft.hardness = 0;
        let feather = paint_at(&clear, StrokeKind::Paint, soft, &[(2.5, 1.5)]);
        let center = feather.pixel(2, 1).unwrap().0[3];
        let edge = feather.pixel(4, 1).unwrap().0[3];
        assert!(center > edge && edge > 0, "{center} {edge}");

        let mut opaque = RgbaImage::from_pixel(4, 1, Rgba([0, 0, 40, 200]));
        let mut stroke = Stroke::new(
            StrokeKind::Erase,
            Rgba([0, 0, 0, 255]),
            Brush {
                diameter: 3,
                spacing: 100,
                hardness: 100,
                opacity: 100,
            },
        );
        stroke.sample(1.5, 0.5, &opaque, |x, _| if x >= 2 { 0 } else { 255 });
        let (erased, bounds) = apply_stroke(&opaque, &stroke).unwrap();
        assert_eq!(erased.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(erased.get_pixel(1, 0).0, [0, 0, 0, 0]);
        assert_eq!(erased.get_pixel(2, 0).0, [0, 0, 40, 200]);
        assert_eq!(bounds, [PixelRect::new(0, 0, 2, 1)]);
        opaque.put_pixel(0, 0, Rgba([0, 0, 40, 200]));
        let mut half = Stroke::new(
            StrokeKind::Erase,
            Rgba([0, 0, 0, 255]),
            Brush {
                diameter: 1,
                spacing: 100,
                hardness: 100,
                opacity: 50,
            },
        );
        half.sample(0.5, 0.5, &opaque, |_, _| 128);
        let (partial, _) = apply_stroke(&opaque, &half).unwrap();
        let alpha = partial.get_pixel(0, 0).0[3];
        assert!(alpha > 0 && alpha < 200, "{alpha}");
    }

    #[test]
    fn a_stroke_buffer_stays_inside_the_dirty_region() {
        let image = RgbaImage::new(200, 200);
        let brush = Brush {
            diameter: 3,
            spacing: 100,
            hardness: 100,
            opacity: 100,
        };
        let mut stroke = Stroke::new(StrokeKind::Paint, Rgba([0, 0, 0, 255]), brush);
        stroke.sample(0.5, 10.5, &image, |_, _| 255);
        stroke.sample(50.5, 10.5, &image, |_, _| 255);
        assert_eq!(stroke.tiles.len(), 1);
        assert!(
            stroke
                .tiles
                .values()
                .map(|tile| tile.as_raw().len())
                .sum::<usize>()
                < image.as_raw().len()
        );
        let (_, bounds) = apply_stroke(&image, &stroke).unwrap();
        assert!(
            bounds
                .iter()
                .all(|rect| rect.width < 80 && rect.height < 10)
        );
    }

    #[test]
    fn shift_click_fills_the_pixels_between_the_endpoints() {
        let image = RgbaImage::new(16, 2);
        let stroke = paint_at(
            &image,
            StrokeKind::Paint,
            Brush {
                diameter: 1,
                spacing: 100,
                hardness: 100,
                opacity: 100,
            },
            &[(0.5, 0.5), (10.5, 0.5)],
        );
        let (painted, _) = apply_stroke(&image, &stroke).unwrap();
        for x in 0..11 {
            assert_eq!(painted.get_pixel(x, 0).0[3], 255, "{x}");
        }
        assert_eq!(painted.get_pixel(11, 0).0[3], 0);
    }

    #[test]
    fn a_long_diagonal_only_stores_touched_tiles() {
        let image = RgbaImage::new(1024, 1024);
        let stroke = paint_at(
            &image,
            StrokeKind::Pencil,
            Brush {
                diameter: 1,
                ..Brush::default()
            },
            &[(0.2, 0.2), (1023.8, 1023.8)],
        );
        assert_eq!(stroke.tiles.len(), 16);
        assert_eq!(stroke.pixel(0, 0).unwrap()[3], 255);
        assert!(stroke.pixel(512, 0).is_none());
    }

    #[test]
    fn a_long_stroke_commit_skips_untouched_preview_tiles() {
        use crate::{
            document::DocumentContent,
            preview::{Dirty, FILTER_APRON, TILE_FINE, TileCache},
        };
        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(1024, 1024));
        let layer = content.top_layer_id();
        let stroke = paint_at(
            &content.layer(layer).unwrap().pixels,
            StrokeKind::Pencil,
            Brush {
                diameter: 1,
                ..Brush::default()
            },
            &[(0.5, 0.5), (1023.5, 1023.5)],
        );
        let mut cache = TileCache::new(1024, 1024, TILE_FINE, FILTER_APRON);
        cache.rebuild(&content, PixelRect::new(0, 0, 1024, 1024));
        let untouched = cache.tile(3, 0).unwrap().clone();
        let changed = cache.tile(3, 3).unwrap().clone();
        let Dirty::Regions(regions) = content.apply_stroke(layer, &stroke) else {
            panic!("expected separate dirty regions");
        };
        assert!(cache.rebuild_regions(&content, &regions).rebuilt < 16);
        assert!(std::sync::Arc::ptr_eq(
            &untouched,
            cache.tile(3, 0).unwrap()
        ));
        assert!(!std::sync::Arc::ptr_eq(&changed, cache.tile(3, 3).unwrap()));
    }

    #[test]
    fn a_closed_trace_does_not_darken_its_start_twice() {
        let image = RgbaImage::new(8, 8);
        let mut stroke = Stroke::new(
            StrokeKind::Paint,
            Rgba([255, 0, 0, 255]),
            Brush {
                diameter: 1,
                spacing: 100,
                hardness: 100,
                opacity: 50,
            },
        );
        stroke.trace(
            &[(1.5, 1.5), (5.5, 1.5), (5.5, 5.5), (1.5, 5.5), (1.5, 1.5)],
            &image,
        );
        assert_eq!(stroke.pixel(1, 1).unwrap()[3], 128);
        assert_eq!(stroke.pixel(2, 1).unwrap()[3], 128);
        assert_eq!(stroke.pixel(1, 2).unwrap()[3], 128);
        let mut repeated = Stroke::new(
            StrokeKind::Paint,
            Rgba([255, 0, 0, 255]),
            Brush {
                diameter: 1,
                spacing: 100,
                hardness: 100,
                opacity: 50,
            },
        );
        repeated.trace(
            &[
                (1.5, 1.5),
                (1.5, 1.5),
                (5.5, 1.5),
                (5.5, 5.5),
                (1.5, 5.5),
                (1.5, 1.5),
                (1.5, 1.5),
            ],
            &image,
        );
        assert_eq!(stroke.tiles, repeated.tiles);
    }

    #[test]
    fn release_stamps_an_endpoint_between_spaced_dabs() {
        let image = RgbaImage::new(32, 4);
        let mut stroke = paint_at(
            &image,
            StrokeKind::Paint,
            Brush {
                diameter: 4,
                spacing: 100,
                ..Brush::default()
            },
            &[(1.5, 1.5), (4.5, 1.5)],
        );
        assert_eq!(stroke.pixel(4, 1).unwrap()[3], 0);
        stroke.finish(&image, |_, _| 255);
        assert_eq!(stroke.pixel(4, 1).unwrap()[3], 255);
    }

    #[test]
    fn a_small_stroke_rebuilds_one_preview_tile() {
        use crate::document::DocumentContent;
        use crate::preview::{Dirty, TILE_FINE, TileCache};

        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(512, 512));
        let id = content.top_layer_id();
        let image = content.layer(id).unwrap().pixels.clone();
        let mut stroke = Stroke::new(
            StrokeKind::Paint,
            Rgba([0, 0, 0, 255]),
            Brush {
                diameter: 4,
                spacing: 100,
                hardness: 100,
                opacity: 100,
            },
        );
        stroke.sample(10.5, 10.5, &image, |_, _| 255);
        let Dirty::Region(rect) = content.apply_stroke(id, &stroke) else {
            panic!("stroke did not change a region");
        };
        let rebuilt = TileCache::new(512, 512, TILE_FINE, 1)
            .rebuild(&content, rect)
            .rebuilt;
        assert_eq!(rebuilt, 1);
        assert!(rect.width <= 8 && rect.height <= 8);
    }

    #[test]
    #[ignore = "release timing for a 4K stroke; run with --release -- --ignored --nocapture"]
    fn stroke_benchmark() {
        use std::time::Instant;

        use crate::document::DocumentContent;
        use crate::preview::{Dirty, TILE_FINE, TileCache};

        if cfg!(debug_assertions) {
            eprintln!(
                "skipping stroke_benchmark: run cargo test --release --locked stroke_benchmark -- --ignored --nocapture"
            );
            return;
        }
        let image = RgbaImage::new(4096, 4096);
        let mut stroke = Stroke::new(
            StrokeKind::Paint,
            Rgba([255, 0, 0, 255]),
            Brush {
                diameter: 8,
                spacing: 25,
                hardness: 80,
                opacity: 100,
            },
        );
        let started = Instant::now();
        stroke.sample(100.5, 100.5, &image, |_, _| 255);
        stroke.sample(160.5, 100.5, &image, |_, _| 255);
        let buffered = started.elapsed();
        assert!(stroke.tiles.len() <= 2);
        let mut content = DocumentContent::single_layer("Image".into(), image);
        let id = content.top_layer_id();
        let started = Instant::now();
        for key in stroke.take_pending_tiles() {
            let rect = stroke.tile_bounds(key).unwrap();
            std::hint::black_box(content.composite_stroke_tile(id, &stroke, rect));
        }
        let composed = started.elapsed();
        let dirty = match content.apply_stroke(id, &stroke) {
            Dirty::Region(rect) => vec![rect],
            Dirty::Regions(rects) => rects,
            _ => panic!("stroke did not change a region"),
        };
        let started = Instant::now();
        let rebuilt = TileCache::new(4096, 4096, TILE_FINE, 1)
            .rebuild_regions(&content, &dirty)
            .rebuilt;
        println!(
            "4K stroke buffer {} µs, live tile composition {} µs, dirty tile rebuild {} µs ({} tiles)",
            buffered.as_micros(),
            composed.as_micros(),
            started.elapsed().as_micros(),
            rebuilt,
        );
        assert!(rebuilt <= 4);
    }

    fn ramp(shape: GradientShape, blend: GradientBlend) -> Gradient {
        Gradient {
            shape,
            blend,
            start: (0.0, 0.0),
            end: (4.0, 0.0),
            foreground: Rgba([255, 0, 0, 255]),
            background: Rgba([0, 0, 255, 255]),
        }
    }

    #[test]
    fn gradient_endpoints_clamp_and_a_zero_length_line_stays_foreground() {
        let linear = ramp(GradientShape::Linear, GradientBlend::ForegroundToBackground);
        assert_eq!(gradient_color(&linear, 0.0, 2.0), linear.foreground);
        assert_eq!(gradient_color(&linear, 4.0, -3.0), linear.background);
        assert_eq!(gradient_color(&linear, -2.0, 0.0), linear.foreground);
        assert_eq!(gradient_color(&linear, 8.0, 0.0), linear.background);
        assert_eq!(gradient_color(&linear, 2.0, 9.0).0, [128, 0, 128, 255]);

        let mut collapsed = linear;
        collapsed.end = collapsed.start;
        assert_eq!(gradient_color(&collapsed, 3.0, 3.0), linear.foreground);

        let radial = ramp(GradientShape::Radial, GradientBlend::ForegroundToBackground);
        assert_eq!(gradient_color(&radial, 0.0, 0.0), radial.foreground);
        assert_eq!(gradient_color(&radial, 4.0, 0.0), radial.background);
        assert_eq!(gradient_color(&radial, 9.0, 0.0), radial.background);
        radial_zero_length_is_foreground(radial);
    }

    fn radial_zero_length_is_foreground(mut radial: Gradient) {
        radial.end = radial.start;
        assert_eq!(gradient_color(&radial, 2.0, 2.0), radial.foreground);
    }

    #[test]
    fn foreground_to_transparent_fades_alpha_and_clears_the_end() {
        let gradient = ramp(
            GradientShape::Linear,
            GradientBlend::ForegroundToTransparent,
        );
        assert_eq!(gradient_color(&gradient, 0.0, 0.0).0, [255, 0, 0, 255]);
        assert_eq!(gradient_color(&gradient, 2.0, 0.0).0, [255, 0, 0, 128]);
        assert_eq!(gradient_color(&gradient, 4.0, 0.0).0, [0, 0, 0, 0]);
    }

    fn render_trace(points: &[(f64, f64)], brush: Brush) -> RgbaImage {
        let source = RgbaImage::new(16, 16);
        let mut stroke = Stroke::new(StrokeKind::Paint, Rgba([255, 0, 0, 255]), brush);
        stroke.trace(points, &source);
        apply_stroke(&source, &stroke)
            .map(|(image, _)| image)
            .unwrap_or(source)
    }

    #[test]
    fn a_repeated_endpoint_matches_the_plain_line_and_a_closed_join_is_stamped_once() {
        let brush = Brush {
            diameter: 3,
            spacing: 100,
            hardness: 100,
            opacity: 50,
        };
        let plain = render_trace(&[(2.0, 3.0), (12.0, 3.0)], brush);
        let repeated = render_trace(&[(2.0, 3.0), (2.0, 3.0), (12.0, 3.0), (12.0, 3.0)], brush);
        assert_eq!(plain.as_raw(), repeated.as_raw());
        let once = render_trace(&[(2.0, 3.0)], brush);
        let mut again = Stroke::new(StrokeKind::Paint, Rgba([255, 0, 0, 255]), brush);
        again.trace(&[(2.0, 3.0)], &once);
        let doubled = apply_stroke(&once, &again).unwrap().0;
        assert!(doubled.get_pixel(2, 3).0[3] > plain.get_pixel(2, 3).0[3]);

        let square = [(4.0, 4.0), (9.0, 4.0), (9.0, 9.0), (4.0, 9.0), (4.0, 4.0)];
        let source = RgbaImage::new(16, 16);
        let mut finished = Stroke::new(StrokeKind::Paint, Rgba([255, 0, 0, 255]), brush);
        for &(x, y) in &square {
            finished.sample(x, y, &source, |_, _| 255);
        }
        finished.finish(&source, |_, _| 255);
        let finished = apply_stroke(&source, &finished).unwrap().0;
        let traced = render_trace(&square, brush);
        let join = render_trace(&[(4.0, 4.0)], brush);
        assert!(finished.get_pixel(4, 4).0[3] > traced.get_pixel(4, 4).0[3]);
        assert_eq!(traced.get_pixel(4, 4).0, join.get_pixel(4, 4).0);
    }
}
