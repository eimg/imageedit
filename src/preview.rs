use std::{sync::Arc, time::Instant};

use image::{Rgba, RgbaImage};

use crate::document::DocumentContent;

pub const TILE_FINE: u32 = 256;
#[cfg_attr(not(test), allow(dead_code))]
pub const TILE_COARSE: u32 = 512;
// GPUI samples atlas textures with a linear filter, so a tile keeps one source
// pixel past each edge. Without it, a seam blends against the neighboring atlas tile.
pub const FILTER_APRON: u32 = 1;
const MAX_FIT_SHIFT: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn new(x: i64, y: i64, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> i64 {
        self.x.saturating_add(i64::from(self.width))
    }

    pub fn bottom(self) -> i64 {
        self.y.saturating_add(i64::from(self.height))
    }

    fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    fn expand(self, apron: u32) -> Self {
        let pad = i64::from(apron);
        Self {
            x: self.x.saturating_sub(pad),
            y: self.y.saturating_sub(pad),
            width: self.width.saturating_add(apron.saturating_mul(2)),
            height: self.height.saturating_add(apron.saturating_mul(2)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileUpdate {
    pub rebuilt: u32,
    pub retained: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dirty {
    None,
    // The document changed, but its pixels did not.
    Unpainted,
    Region(PixelRect),
    Canvas,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSpan {
    pub index: usize,
    pub content: PixelRect,
    pub stored: PixelRect,
}

// Coarsest power-of-two reduction that still covers the viewport.
// 8192 canvas pixels in a 1600 pixel window uses shift 2 (2048 source pixels).
pub fn fit_level_shift(canvas_edge: u32, viewport_edge: u32) -> u32 {
    if viewport_edge == 0 || canvas_edge <= viewport_edge {
        return 0;
    }
    let mut shift = 0;
    while shift < MAX_FIT_SHIFT && (canvas_edge >> (shift + 1)) >= viewport_edge {
        shift += 1;
    }
    shift
}

#[derive(Clone)]
pub struct TileCache {
    tile: u32,
    apron: u32,
    shift: u32,
    canvas_width: u32,
    canvas_height: u32,
    cols: u32,
    rows: u32,
    tiles: Vec<Option<Arc<RgbaImage>>>,
}

impl TileCache {
    pub fn new(width: u32, height: u32, tile: u32, apron: u32) -> Self {
        Self::leveled(width, height, tile, apron, 0)
    }

    // `shift` is the Fit reduction: each level pixel covers `1 << shift` canvas pixels.
    pub fn leveled(width: u32, height: u32, tile: u32, apron: u32, shift: u32) -> Self {
        let shift = shift.min(MAX_FIT_SHIFT);
        let scale = 1u32 << shift;
        let tile = tile.max(1);
        let cols = width.div_ceil(scale).max(1).div_ceil(tile).max(1);
        let rows = height.div_ceil(scale).max(1).div_ceil(tile).max(1);
        Self {
            tile,
            apron,
            shift,
            canvas_width: width,
            canvas_height: height,
            cols,
            rows,
            tiles: vec![None; (cols as usize).saturating_mul(rows as usize)],
        }
    }

    pub fn shift(&self) -> u32 {
        self.shift
    }

    pub fn spans(&self) -> impl Iterator<Item = TileSpan> + '_ {
        (0..self.rows).flat_map(move |row| (0..self.cols).map(move |column| self.span(column, row)))
    }

    pub fn slot(&self, index: usize) -> Option<&Arc<RgbaImage>> {
        self.tiles.get(index).and_then(|tile| tile.as_ref())
    }

    #[cfg(test)]
    pub fn tile(&self, column: u32, row: u32) -> Option<&Arc<RgbaImage>> {
        self.index(column, row)
            .and_then(|index| self.tiles[index].as_ref())
    }

    // Rebuilds tiles whose filter coverage meets `dirty`. Untouched tiles keep their image.
    pub fn rebuild(&mut self, content: &DocumentContent, dirty: PixelRect) -> TileUpdate {
        if dirty.is_empty() {
            return TileUpdate {
                rebuilt: 0,
                retained: self.stored(),
            };
        }
        let mut rebuilt = 0;
        for row in 0..self.rows {
            for column in 0..self.cols {
                if !self.coverage(column, row).overlaps(dirty) {
                    continue;
                }
                let index = (row * self.cols + column) as usize;
                let rect = self.coverage(column, row);
                let image = content.composite_rect(rect.x, rect.y, rect.width, rect.height);
                self.tiles[index] = Some(Arc::new(box_downsample(&image, self.shift)));
                rebuilt += 1;
            }
        }
        TileUpdate {
            rebuilt,
            retained: self.stored() - rebuilt,
        }
    }

    #[cfg(test)]
    pub fn visible(&self, view: PixelRect) -> u32 {
        if view.is_empty() {
            return 0;
        }
        let mut count = 0;
        for row in 0..self.rows {
            for column in 0..self.cols {
                if self.content_rect(column, row).overlaps(view) {
                    count += 1;
                }
            }
        }
        count
    }

    fn stored(&self) -> u32 {
        self.tiles.iter().filter(|tile| tile.is_some()).count() as u32
    }

    #[cfg(test)]
    fn index(&self, column: u32, row: u32) -> Option<usize> {
        (column < self.cols && row < self.rows).then_some((row * self.cols + column) as usize)
    }

    fn span(&self, column: u32, row: u32) -> TileSpan {
        TileSpan {
            index: (row * self.cols + column) as usize,
            content: self.content_rect(column, row),
            stored: self.stored_rect(column, row),
        }
    }

    fn scale(&self) -> u32 {
        1u32 << self.shift
    }

    fn content_rect(&self, column: u32, row: u32) -> PixelRect {
        let scale = u64::from(self.scale());
        let origin_x = u64::from(column) * u64::from(self.tile) * scale;
        let origin_y = u64::from(row) * u64::from(self.tile) * scale;
        let width = (u64::from(self.tile) * scale)
            .min(u64::from(self.canvas_width).saturating_sub(origin_x));
        let height = (u64::from(self.tile) * scale)
            .min(u64::from(self.canvas_height).saturating_sub(origin_y));
        PixelRect::new(
            origin_x as i64,
            origin_y as i64,
            width as u32,
            height as u32,
        )
    }

    fn coverage(&self, column: u32, row: u32) -> PixelRect {
        clip_to_canvas(
            self.content_rect(column, row)
                .expand(self.apron.saturating_mul(self.scale())),
            self.canvas_width,
            self.canvas_height,
        )
    }

    fn stored_rect(&self, column: u32, row: u32) -> PixelRect {
        let source = self.coverage(column, row);
        let scale = self.scale();
        // A partial block at the canvas edge still occupies one texel in the image.
        // Keep its drawn pitch consistent with the samples shared by adjacent tiles.
        PixelRect::new(
            source.x,
            source.y,
            source.width.div_ceil(scale) * scale,
            source.height.div_ceil(scale) * scale,
        )
    }
}

fn clip_to_canvas(rect: PixelRect, width: u32, height: u32) -> PixelRect {
    let right = rect.right().clamp(0, i64::from(width));
    let bottom = rect.bottom().clamp(0, i64::from(height));
    let x = rect.x.clamp(0, i64::from(width));
    let y = rect.y.clamp(0, i64::from(height));
    PixelRect::new(x, y, (right - x) as u32, (bottom - y) as u32)
}

// Averages each (1<<shift) square. Used for the Fit level chosen by `fit_level_shift`.
pub fn box_downsample(source: &RgbaImage, shift: u32) -> RgbaImage {
    if shift == 0 {
        return source.clone();
    }
    let factor = 1u32 << shift;
    let width = source.width().div_ceil(factor);
    let height = source.height().div_ceil(factor);
    RgbaImage::from_fn(width, height, |x, y| {
        let left = x * factor;
        let top = y * factor;
        let right = (left + factor).min(source.width());
        let bottom = (top + factor).min(source.height());
        let count = u64::from(right - left) * u64::from(bottom - top);
        let mut rgb = [0u64; 3];
        let mut alpha = 0u64;
        for sy in top..bottom {
            for sx in left..right {
                let [r, g, b, a] = source.get_pixel(sx, sy).0;
                let a = u64::from(a);
                alpha += a;
                for (sum, channel) in rgb.iter_mut().zip([r, g, b]) {
                    *sum += u64::from(channel) * a;
                }
            }
        }
        let [r, g, b] = rgb.map(|sum| (sum / alpha.max(1)) as u8);
        Rgba([r, g, b, (alpha / count) as u8])
    })
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub struct TimedUpdate {
    pub tile: u32,
    pub rebuilt: u32,
    pub p95_micros: u128,
    pub samples: u32,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn time_updates(
    content: &DocumentContent,
    tile: u32,
    dirty: PixelRect,
    warmup: u32,
    samples: u32,
) -> TimedUpdate {
    let (width, height) = content.size();
    let mut cache = TileCache::new(width, height, tile, FILTER_APRON);
    for _ in 0..warmup {
        cache.rebuild(content, dirty);
    }
    let mut elapsed = Vec::with_capacity(samples as usize);
    let mut rebuilt = 0;
    for _ in 0..samples {
        let started = Instant::now();
        rebuilt = cache.rebuild(content, dirty).rebuilt;
        elapsed.push(started.elapsed().as_micros());
    }
    elapsed.sort_unstable();
    let index = (samples as usize * 95).div_ceil(100) - 1;
    TimedUpdate {
        tile,
        rebuilt,
        p95_micros: elapsed[index],
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{BlendMode, LayerId, RasterLayer};

    fn solid(width: u32, height: u32, pixel: [u8; 4]) -> DocumentContent {
        DocumentContent::single_layer(
            "Image".into(),
            RgbaImage::from_pixel(width, height, Rgba(pixel)),
        )
    }

    #[test]
    fn an_interior_edit_rebuilds_one_tile_and_keeps_the_others() {
        let content = solid(640, 400, [10, 20, 30, 255]);
        let mut cache = TileCache::new(640, 400, TILE_FINE, FILTER_APRON);
        let first = cache.rebuild(&content, PixelRect::new(0, 0, 640, 400));
        assert_eq!(first.rebuilt, 6);
        let kept = cache.tile(0, 0).unwrap().clone();

        let interior = PixelRect::new(300, 40, 64, 64);
        let update = cache.rebuild(&content, interior);
        assert_eq!(update.rebuilt, 1);
        assert_eq!(update.retained, 5);
        assert!(Arc::ptr_eq(cache.tile(0, 0).unwrap(), &kept));
        assert!(!Arc::ptr_eq(cache.tile(1, 0).unwrap(), &kept));
        assert_eq!(
            cache.rebuild(&content, PixelRect::new(0, 0, 0, 10)).rebuilt,
            0
        );
    }

    #[test]
    fn edits_on_a_shared_edge_or_corner_rebuild_every_touched_tile() {
        let content = solid(512, 512, [1, 2, 3, 255]);
        let mut cache = TileCache::new(512, 512, TILE_FINE, FILTER_APRON);
        assert_eq!(
            cache
                .rebuild(&content, PixelRect::new(224, 20, 64, 64))
                .rebuilt,
            2
        );
        assert_eq!(
            cache
                .rebuild(&content, PixelRect::new(224, 224, 64, 64))
                .rebuilt,
            4
        );
        assert_eq!(cache.visible(PixelRect::new(0, 0, 100, 100)), 1);
        assert_eq!(cache.visible(PixelRect::new(200, 0, 100, 10)), 2);
        assert_eq!(cache.visible(PixelRect::new(600, 0, 20, 20)), 0);
    }

    #[test]
    fn a_tile_only_rebuilds_for_pixels_inside_its_sampling_apron() {
        let content = solid(512, 256, [1, 2, 3, 255]);
        let mut cache = TileCache::new(512, 256, TILE_FINE, FILTER_APRON);
        assert_eq!(
            cache
                .rebuild(&content, PixelRect::new(254, 10, 1, 1))
                .rebuilt,
            1
        );
        assert_eq!(
            cache
                .rebuild(&content, PixelRect::new(255, 10, 1, 1))
                .rebuilt,
            2
        );
    }

    #[test]
    fn an_odd_canvas_keeps_a_consistent_sampling_pitch_across_fit_tiles() {
        let content = solid(19, 3, [1, 2, 3, 255]);
        let mut cache = TileCache::leveled(19, 3, 4, FILTER_APRON, 1);
        cache.rebuild(&content, PixelRect::new(0, 0, 19, 3));
        let spans: Vec<_> = cache.spans().collect();
        assert_eq!(spans[2].content.right(), 19);
        assert_eq!(spans[2].stored.right(), 20);
        for span in spans {
            let tile = cache.slot(span.index).unwrap();
            assert_eq!(span.stored.width, tile.width() * 2);
            assert_eq!(span.stored.height, tile.height() * 2);
        }
    }

    #[test]
    fn fit_downsampling_weights_color_by_alpha() {
        let source = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 0])
            }
        });
        assert_eq!(
            box_downsample(&source, 1).get_pixel(0, 0).0,
            [255, 0, 0, 127]
        );
    }

    #[test]
    fn the_filter_apron_makes_a_linear_seam_match_the_composite() {
        let mut pixels = RgbaImage::from_pixel(16, 2, Rgba([0, 0, 255, 255]));
        for y in 0..2 {
            for x in 8..16 {
                pixels.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let content = DocumentContent::single_layer("Image".into(), pixels);
        let mut cache = TileCache::new(16, 2, 8, FILTER_APRON);
        cache.rebuild(&content, PixelRect::new(0, 0, 16, 2));
        let left = cache.tile(0, 0).unwrap();
        let right = cache.tile(1, 0).unwrap();
        // Stored tiles are the content plus the one-pixel apron, clipped to the canvas.
        assert_eq!(left.dimensions(), (9, 2));
        assert_eq!(right.dimensions(), (9, 2));
        assert_eq!(left.get_pixel(8, 0).0, [255, 0, 0, 255]);
        assert_eq!(right.get_pixel(0, 0).0, [0, 0, 255, 255]);
        let blended = [0, 1, 2, 3].map(|channel| {
            ((u16::from(left.get_pixel(7, 0).0[channel])
                + u16::from(left.get_pixel(8, 0).0[channel]))
                / 2) as u8
        });
        assert_eq!(blended, [127, 0, 127, 255]);

        let mut bare = TileCache::new(16, 2, 8, 0);
        bare.rebuild(&content, PixelRect::new(0, 0, 16, 2));
        assert_eq!(bare.tile(0, 0).unwrap().dimensions(), (8, 2));
        assert_eq!(bare.tile(0, 0).unwrap().get_pixel(7, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn fit_level_and_box_downsample_cover_the_viewport() {
        assert_eq!(fit_level_shift(8192, 1600), 2);
        assert_eq!(fit_level_shift(8192, 4096), 1);
        assert_eq!(fit_level_shift(100, 400), 0);
        let source = RgbaImage::from_fn(2, 2, |x, y| Rgba([x as u8 * 10, y as u8 * 20, 0, 250]));
        let down = box_downsample(&source, 1);
        assert_eq!(down.dimensions(), (1, 1));
        assert_eq!(down.get_pixel(0, 0).0, [5, 10, 0, 250]);
    }

    #[test]
    fn a_partial_layer_still_updates_only_the_dirty_tile() {
        let base = RasterLayer {
            id: LayerId(0),
            name: "Image".into(),
            pixels: Arc::new(RgbaImage::from_pixel(300, 80, Rgba([0, 0, 0, 255]))),
            offset: (0, 0),
            visible: true,
            opacity: 255,
            blend_mode: BlendMode::Normal,
        };
        let glaze = RasterLayer {
            id: LayerId(1),
            name: "Glaze".into(),
            pixels: Arc::new(RgbaImage::from_pixel(20, 20, Rgba([255, 0, 0, 128]))),
            offset: (270, 10),
            visible: true,
            opacity: 200,
            blend_mode: BlendMode::Normal,
        };
        let content = DocumentContent::from_layers(300, 80, vec![base, glaze]).unwrap();
        let mut cache = TileCache::new(300, 80, TILE_FINE, FILTER_APRON);
        assert_eq!(
            cache
                .rebuild(&content, PixelRect::new(10, 10, 64, 64))
                .rebuilt,
            1
        );
        let tile = cache.tile(0, 0).unwrap();
        assert_eq!(tile.get_pixel(10, 10).0, [0, 0, 0, 255]);
    }

    #[test]
    #[ignore = "release timing for the 8192 canvas; run with --release -- --ignored --nocapture"]
    fn preview_benchmark() {
        if cfg!(debug_assertions) {
            eprintln!(
                "skipping preview_benchmark: run cargo test --release --locked preview_benchmark -- --ignored --nocapture"
            );
            return;
        }
        let (width, height) = (8192, 8192);
        let pixels = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([(x & 255) as u8, (y & 255) as u8, 40, 255])
        });
        let content = DocumentContent::single_layer("Image".into(), pixels);
        let interior = PixelRect::new(300, 300, 64, 64);
        let full = PixelRect::new(0, 0, width, height);
        for tile in [TILE_FINE, TILE_COARSE] {
            let boundary = PixelRect::new(i64::from(tile) - 32, i64::from(tile) - 32, 64, 64);
            let interior = time_updates(&content, tile, interior, 2, 20);
            let boundary = time_updates(&content, tile, boundary, 1, 10);
            assert_eq!((interior.tile, boundary.tile), (tile, tile));
            let started = Instant::now();
            let refreshed =
                TileCache::new(width, height, tile, FILTER_APRON).rebuild(&content, full);
            println!(
                "tile {}: interior p95 {} µs ({} tiles, {} samples), boundary p95 {} µs ({} tiles), full {} ms ({} tiles)",
                interior.tile,
                interior.p95_micros,
                interior.rebuilt,
                interior.samples,
                boundary.p95_micros,
                boundary.rebuilt,
                started.elapsed().as_millis(),
                refreshed.rebuilt
            );
        }
        let started = Instant::now();
        let _ = image::imageops::resize(
            content.layers()[0].pixels.as_ref(),
            4096,
            4096,
            image::imageops::FilterType::Triangle,
        );
        println!(
            "current full preview resize to 4096: {} ms",
            started.elapsed().as_millis()
        );
    }
}
