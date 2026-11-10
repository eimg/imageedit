use image::{Rgba, RgbaImage};

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
    Paint,
    Erase,
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
}

impl Plotter {
    fn new(spacing: f64) -> Self {
        Self {
            spacing: spacing.max(1.0),
            from: None,
            along: 0.0,
        }
    }

    fn push(&mut self, point: PlotPoint) -> Vec<PlotPoint> {
        let Some(from) = self.from else {
            self.from = Some(point);
            self.along = 0.0;
            return vec![point];
        };
        let dx = point.x - from.x;
        let dy = point.y - from.y;
        let len = dx.hypot(dy);
        if len < 1e-6 {
            self.from = Some(point);
            return Vec::new();
        }
        let mut stamps = Vec::new();
        let mut walked = 0.0;
        let mut since_stamp = self.along;
        while since_stamp + (len - walked) >= self.spacing - 1e-9 {
            let step = self.spacing - since_stamp;
            walked += step;
            if walked > len + 1e-9 {
                break;
            }
            let t = walked / len;
            stamps.push(PlotPoint {
                x: from.x + dx * t,
                y: from.y + dy * t,
            });
            since_stamp = 0.0;
        }
        self.along = since_stamp + (len - walked);
        self.from = Some(point);
        stamps
    }
}

#[cfg(test)]
fn plot_path(points: &[(f64, f64)], spacing: f64) -> Vec<(f64, f64)> {
    let mut plotter = Plotter::new(spacing);
    let mut stamps = Vec::new();
    for &(x, y) in points {
        stamps.extend(
            plotter
                .push(PlotPoint { x, y })
                .into_iter()
                .map(|point| (point.x, point.y)),
        );
    }
    stamps
}

struct Buffer {
    origin: (i32, i32),
    pixels: RgbaImage,
}

pub struct Stroke {
    kind: StrokeKind,
    color: Rgba<u8>,
    brush: Brush,
    plotter: Plotter,
    buffer: Option<Buffer>,
    changed: bool,
}

impl Stroke {
    pub fn new(kind: StrokeKind, color: Rgba<u8>, brush: Brush) -> Self {
        let brush = brush.clamped();
        Self {
            kind,
            color,
            plotter: Plotter::new(brush.spacing_px()),
            brush,
            buffer: None,
            changed: false,
        }
    }

    pub fn changed(&self) -> bool {
        self.changed
    }

    pub fn origin(&self) -> (i32, i32) {
        self.buffer
            .as_ref()
            .map(|buffer| buffer.origin)
            .unwrap_or((0, 0))
    }

    pub fn pixels(&self) -> Option<&RgbaImage> {
        self.buffer.as_ref().map(|buffer| &buffer.pixels)
    }

    pub fn sample(
        &mut self,
        x: f64,
        y: f64,
        source: &RgbaImage,
        coverage: impl Fn(i32, i32) -> u8,
    ) {
        for point in self.plotter.push(PlotPoint { x, y }) {
            self.dab(point.x, point.y, source, &coverage);
        }
    }

    fn dab(&mut self, x: f64, y: f64, source: &RgbaImage, coverage: &impl Fn(i32, i32) -> u8) {
        let radius = f64::from(self.brush.diameter) / 2.0;
        let min_x = (x - radius).floor() as i32;
        let min_y = (y - radius).floor() as i32;
        let max_x = (x + radius).ceil() as i32;
        let max_y = (y + radius).ceil() as i32;
        self.cover(min_x, min_y, max_x, max_y, source);
        let kind = self.kind;
        let color = self.color;
        let hardness = self.brush.hardness;
        let opacity = self.brush.opacity;
        let wrote = {
            let Some(buffer) = self.buffer.as_mut() else {
                return;
            };
            let (origin_x, origin_y) = buffer.origin;
            let mut wrote = false;
            for py in min_y..max_y {
                for px in min_x..max_x {
                    if px < origin_x || py < origin_y {
                        continue;
                    }
                    let local_x = (px - origin_x) as u32;
                    let local_y = (py - origin_y) as u32;
                    if local_x >= buffer.pixels.width() || local_y >= buffer.pixels.height() {
                        continue;
                    }
                    let falloff =
                        dab_falloff(px as f64 + 0.5 - x, py as f64 + 0.5 - y, radius, hardness);
                    if falloff <= 0.0 {
                        continue;
                    }
                    let amount =
                        falloff * f64::from(opacity) / 100.0 * f64::from(coverage(px, py)) / 255.0;
                    if amount <= 0.0 {
                        continue;
                    }
                    let before = *buffer.pixels.get_pixel(local_x, local_y);
                    let after = match kind {
                        StrokeKind::Paint => source_over(before, color, amount),
                        StrokeKind::Erase => erase(before, amount),
                    };
                    if after != before {
                        buffer.pixels.put_pixel(local_x, local_y, after);
                        wrote = true;
                    }
                }
            }
            wrote
        };
        self.changed |= wrote;
    }

    fn cover(&mut self, min_x: i32, min_y: i32, max_x: i32, max_y: i32, source: &RgbaImage) {
        let limit_x = source.width() as i32;
        let limit_y = source.height() as i32;
        let min_x = min_x.clamp(0, limit_x);
        let min_y = min_y.clamp(0, limit_y);
        let max_x = max_x.clamp(0, limit_x);
        let max_y = max_y.clamp(0, limit_y);
        if min_x >= max_x || min_y >= max_y {
            return;
        }
        let Some(buffer) = self.buffer.take() else {
            self.buffer = Some(Buffer {
                origin: (min_x, min_y),
                pixels: copied(source, min_x, min_y, max_x, max_y),
            });
            return;
        };
        let (origin_x, origin_y) = buffer.origin;
        let old_right = origin_x + buffer.pixels.width() as i32;
        let old_bottom = origin_y + buffer.pixels.height() as i32;
        let next_x = min_x.min(origin_x);
        let next_y = min_y.min(origin_y);
        let next_right = max_x.max(old_right);
        let next_bottom = max_y.max(old_bottom);
        if next_x == origin_x
            && next_y == origin_y
            && next_right == old_right
            && next_bottom == old_bottom
        {
            self.buffer = Some(buffer);
            return;
        }
        let mut next = copied(source, next_x, next_y, next_right, next_bottom);
        blit_image(
            &buffer.pixels,
            &mut next,
            (origin_x - next_x) as u32,
            (origin_y - next_y) as u32,
        );
        self.buffer = Some(Buffer {
            origin: (next_x, next_y),
            pixels: next,
        });
    }
}

pub fn apply_stroke(
    source: &RgbaImage,
    stroke: &Stroke,
) -> Option<(RgbaImage, (i32, i32, u32, u32))> {
    let buffer = stroke.buffer.as_ref().filter(|_| stroke.changed)?;
    let mut image = source.clone();
    let (origin_x, origin_y) = buffer.origin;
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    let mut changed = false;
    for y in 0..buffer.pixels.height() {
        for x in 0..buffer.pixels.width() {
            let px = origin_x + x as i32;
            let py = origin_y + y as i32;
            if px < 0 || py < 0 || px >= source.width() as i32 || py >= source.height() as i32 {
                continue;
            }
            let next = *buffer.pixels.get_pixel(x, y);
            if next == *image.get_pixel(px as u32, py as u32) {
                continue;
            }
            image.put_pixel(px as u32, py as u32, next);
            changed = true;
            min_x = min_x.min(px);
            min_y = min_y.min(py);
            max_x = max_x.max(px);
            max_y = max_y.max(py);
        }
    }
    changed.then_some((
        image,
        (
            min_x,
            min_y,
            (max_x - min_x + 1) as u32,
            (max_y - min_y + 1) as u32,
        ),
    ))
}

fn copied(source: &RgbaImage, min_x: i32, min_y: i32, max_x: i32, max_y: i32) -> RgbaImage {
    let mut image = RgbaImage::new((max_x - min_x) as u32, (max_y - min_y) as u32);
    for y in 0..image.height() {
        for x in 0..image.width() {
            image.put_pixel(
                x,
                y,
                *source.get_pixel((min_x + x as i32) as u32, (min_y + y as i32) as u32),
            );
        }
    }
    image
}

fn blit_image(source: &RgbaImage, dest: &mut RgbaImage, origin_x: u32, origin_y: u32) {
    for y in 0..source.height() {
        for x in 0..source.width() {
            dest.put_pixel(origin_x + x, origin_y + y, *source.get_pixel(x, y));
        }
    }
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
        let pixel = once.pixels().unwrap().get_pixel(0, 0).0;
        assert_eq!(pixel[0], 255);
        assert!((120..=135).contains(&pixel[3]), "{pixel:?}");
        soft.diameter = 3;
        soft.spacing = 1;
        let twice = paint_at(&clear, StrokeKind::Paint, soft, &[(0.5, 0.5), (1.5, 0.5)]);
        let compounded = twice.pixels().unwrap().get_pixel(0, 0).0[3];
        assert!(compounded > pixel[3] && compounded < 255, "{compounded}");
        soft.diameter = 1;

        soft.opacity = 100;
        soft.diameter = 5;
        soft.hardness = 0;
        let feather = paint_at(&clear, StrokeKind::Paint, soft, &[(2.5, 1.5)]);
        let center = feather.pixels().unwrap().get_pixel(2, 1).0[3];
        let edge = feather.pixels().unwrap().get_pixel(4, 1).0[3];
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
        assert_eq!(bounds, (0, 0, 2, 1));
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
        let pixels = stroke.pixels().unwrap();
        assert!(pixels.width() < 80, "{}", pixels.width());
        assert!(pixels.height() < 10, "{}", pixels.height());
        assert!(pixels.width() as usize * pixels.height() as usize * 4 < image.as_raw().len());
        let (_, bounds) = apply_stroke(&image, &stroke).unwrap();
        assert!(bounds.2 < 80 && bounds.3 < 10);
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
        let image = RgbaImage::new(8192, 8192);
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
        let pixels = stroke.pixels().unwrap();
        assert!(pixels.width() < 128 && pixels.height() < 32);
        let mut content = DocumentContent::single_layer("Image".into(), image);
        let id = content.top_layer_id();
        let Dirty::Region(rect) = content.apply_stroke(id, &stroke) else {
            panic!("stroke did not change a region");
        };
        let started = Instant::now();
        let rebuilt = TileCache::new(8192, 8192, TILE_FINE, 1)
            .rebuild(&content, rect)
            .rebuilt;
        println!(
            "4K stroke buffer {} µs, dirty tile rebuild {} µs ({} tiles, {}×{})",
            buffered.as_micros(),
            started.elapsed().as_micros(),
            rebuilt,
            rect.width,
            rect.height
        );
        assert!(rebuilt <= 4);
    }
}
