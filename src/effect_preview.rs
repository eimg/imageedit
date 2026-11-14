use std::sync::OnceLock;

use image::{Rgba, RgbaImage};

use crate::adjustments::{self, Adjustment};
use crate::document::{self, DocumentContent, LayerId};
use crate::filters::{self, FilterSpec};
use crate::selection::Selection;

const MAX_DISPLAY_EDGE: u32 = 1024;

// Fit the image to the actual logical preview box, then account for Retina/DPI.
// Never enlarge the source or let a large screen create unbounded background work.
pub fn display_edge(canvas: (u32, u32), bounds: (f32, f32), scale: f32) -> u32 {
    let fit = (bounds.0 / canvas.0.max(1) as f32).min(bounds.1 / canvas.1.max(1) as f32);
    let edge = canvas.0.max(canvas.1) as f32 * fit * scale;
    if edge.is_finite() {
        (edge.ceil() as u32).clamp(1, MAX_DISPLAY_EDGE)
    } else {
        MAX_DISPLAY_EDGE
    }
}

// An immutable dialog snapshot. Area reduction visits visible source pixels in
// background work; scratch space is bounded to source/preview rows, never a full canvas.
pub struct EffectPreview {
    content: DocumentContent,
    layer: LayerId,
    selection: Selection,
    size: (u32, u32),
    sampling_size: (u32, u32),
    original: OnceLock<RgbaImage>,
}

struct Region {
    pixels: RgbaImage,
    origin: (i64, i64),
    scale: f64,
}

struct ShadowPreview {
    image: filters::ShadowImage,
    origin: (i64, i64),
    scale: f64,
}

impl EffectPreview {
    pub fn new(content: DocumentContent, layer: LayerId, selection: Selection, edge: u32) -> Self {
        let (w, h) = content.size();
        let edge = edge.clamp(1, MAX_DISPLAY_EDGE);
        Self {
            content,
            layer,
            selection,
            size: adjustments::preview_size(w, h, edge),
            sampling_size: adjustments::preview_size(w, h, edge * 2),
            original: OnceLock::new(),
        }
    }

    pub fn resized(&self, edge: u32) -> Option<Self> {
        let edge = edge.clamp(1, MAX_DISPLAY_EDGE);
        let (w, h) = self.content.size();
        (adjustments::preview_size(w, h, edge) != self.size).then(|| {
            Self::new(
                self.content.clone(),
                self.layer,
                self.selection.clone(),
                edge,
            )
        })
    }

    pub fn original(&self) -> &RgbaImage {
        self.original
            .get_or_init(|| self.composite(|pixel, _, _| pixel, None))
    }

    pub fn adjustment(&self, adjustment: Adjustment) -> RgbaImage {
        if adjustment.is_identity() {
            return self.original().clone();
        }
        let prepared = adjustments::Prepared::from(adjustment);
        let alpha_locked = self
            .content
            .layer(self.layer)
            .is_some_and(|layer| layer.locks.alpha);
        self.composite(
            |pixel, x, y| {
                if alpha_locked && pixel[3] == 0 {
                    return pixel;
                }
                adjustments::blend_coverage(
                    pixel,
                    self.content.mode().constrain(prepared.map(pixel)),
                    self.coverage(x, y),
                )
            },
            None,
        )
    }

    pub fn filter(&self, spec: FilterSpec) -> RgbaImage {
        if spec.is_identity() {
            return self.original().clone();
        }
        if let FilterSpec::Shadow {
            radius,
            offset,
            opacity,
            color,
        } = spec
        {
            let Some(region) = self.region(radius, offset, true) else {
                return self.original().clone();
            };
            let shadow = filters::render_shadow(
                &region.pixels,
                (f64::from(radius) * region.scale).round() as u32,
                opacity,
                self.content.mode().constrain(color),
            )
            .map(|image| ShadowPreview {
                image,
                origin: region.origin,
                scale: region.scale,
            });
            return self.composite(|pixel, _, _| pixel, shadow.as_ref());
        }
        let radius = match spec {
            FilterSpec::Blur { radius } | FilterSpec::Sharpen { radius, .. } => radius,
            _ => 0,
        };
        let Some(region) = self.region(radius, (0, 0), false) else {
            return self.original().clone();
        };
        let radius = (f64::from(radius) * region.scale).round() as u32;
        // A subpixel radius cannot be represented in this reduced preview.
        // Leave it unchanged instead of showing a resampling artifact as an effect.
        if radius == 0 {
            return self.original().clone();
        }
        let alpha_locked = self
            .content
            .layer(self.layer)
            .is_some_and(|layer| layer.locks.alpha);
        let filtered = filters::preview(
            &region.pixels,
            match spec {
                FilterSpec::Blur { .. } => FilterSpec::Blur { radius },
                FilterSpec::Sharpen { amount, .. } => FilterSpec::Sharpen { radius, amount },
                _ => unreachable!(),
            },
            0,
            |_, _| 255,
            |pixel| self.content.mode().constrain(pixel),
            alpha_locked,
        );
        self.composite(
            |pixel, x, y| {
                if alpha_locked && pixel[3] == 0 {
                    return pixel;
                }
                let sx = ((x - region.origin.0) as f64 * region.scale).floor() as u32;
                let sy = ((y - region.origin.1) as f64 * region.scale).floor() as u32;
                let target = *filtered
                    .get_pixel(sx.min(filtered.width() - 1), sy.min(filtered.height() - 1));
                let target = if alpha_locked {
                    crate::paint::preserve_alpha(pixel, target)
                } else {
                    target
                };
                filters::blend(pixel, target, self.coverage(x, y))
            },
            None,
        )
    }

    // Retain the visible source plus the filter's complete sampling halo. This
    // avoids squeezing a huge layer into a tiny preview of a small canvas, and
    // includes off-canvas pixels that can blur or cast a shadow onto the canvas.
    fn region(&self, radius: u32, offset: (i32, i32), masked: bool) -> Option<Region> {
        let layer = self.content.layer(self.layer)?;
        let canvas = self.content.size();
        let scale = f64::from(self.sampling_size.0.max(self.sampling_size.1))
            / f64::from(canvas.0.max(canvas.1));
        let pad = i64::from(filters::kernel_support(radius));
        let origin = (
            i64::from(layer.offset.0) + i64::from(offset.0),
            i64::from(layer.offset.1) + i64::from(offset.1),
        );
        let left = (-origin.0 - pad).clamp(0, i64::from(layer.pixels.width()));
        let top = (-origin.1 - pad).clamp(0, i64::from(layer.pixels.height()));
        let right =
            (i64::from(canvas.0) - origin.0 + pad).clamp(0, i64::from(layer.pixels.width()));
        let bottom =
            (i64::from(canvas.1) - origin.1 + pad).clamp(0, i64::from(layer.pixels.height()));
        if right <= left || bottom <= top {
            return None;
        }
        let w = ((right - left) as f64 * scale).ceil().max(1.) as u32;
        let h = ((bottom - top) as f64 * scale).ceil().max(1.) as u32;
        let pixels = reduce_area(
            ((right - left) as u32, (bottom - top) as u32),
            (w, h),
            |y, row| {
                for (x, pixel) in row.iter_mut().enumerate() {
                    let (sx, sy) = (left as u32 + x as u32, top as u32 + y);
                    *pixel = *layer.pixels.get_pixel(sx, sy);
                    if masked {
                        *pixel = layer.masked(*pixel, sx, sy);
                    }
                }
            },
        );
        Some(Region {
            pixels,
            origin: (origin.0 + left, origin.1 + top),
            scale,
        })
    }

    fn coverage(&self, x: i64, y: i64) -> u8 {
        let size = self.content.size();
        self.selection.coverage(x, y, size.0, size.1)
    }

    fn composite(
        &self,
        map: impl Fn(Rgba<u8>, i64, i64) -> Rgba<u8>,
        shadow: Option<&ShadowPreview>,
    ) -> RgbaImage {
        let canvas = self.content.size();
        reduce_area(canvas, self.size, |cy, row| {
            row.fill(Rgba([0; 4]));
            for layer in self.content.layers() {
                // Shadows are independent of source visibility/opacity and selection.
                if layer.id == self.layer
                    && let Some(shadow) = shadow
                {
                    let sy = ((i64::from(cy) - shadow.origin.1) as f64 * shadow.scale).floor()
                        as i64
                        + i64::from(shadow.image.pad);
                    if (0..i64::from(shadow.image.pixels.height())).contains(&sy) {
                        for (cx, pixel) in row.iter_mut().enumerate() {
                            let sx = ((cx as i64 - shadow.origin.0) as f64 * shadow.scale).floor()
                                as i64
                                + i64::from(shadow.image.pad);
                            if (0..i64::from(shadow.image.pixels.width())).contains(&sx) {
                                *pixel = document::source_over(
                                    *pixel,
                                    *shadow.image.pixels.get_pixel(sx as u32, sy as u32),
                                    255,
                                );
                            }
                        }
                    }
                }
                if !layer.visible || layer.opacity == 0 {
                    continue;
                }
                let sy = i64::from(cy) - i64::from(layer.offset.1);
                if !(0..i64::from(layer.pixels.height())).contains(&sy) {
                    continue;
                }
                let left = i64::from(layer.offset.0).clamp(0, i64::from(canvas.0)) as usize;
                let right = (i64::from(layer.offset.0) + i64::from(layer.pixels.width()))
                    .clamp(0, i64::from(canvas.0)) as usize;
                for (cx, destination) in row.iter_mut().enumerate().take(right).skip(left) {
                    let sx = (cx as i64 - i64::from(layer.offset.0)) as u32;
                    let mut pixel = *layer.pixels.get_pixel(sx, sy as u32);
                    if layer.id == self.layer {
                        pixel = map(pixel, cx as i64, i64::from(cy));
                    }
                    pixel = layer.masked(pixel, sx, sy as u32);
                    *destination =
                        document::blend(*destination, pixel, layer.opacity, layer.blend_mode);
                }
            }
        })
    }
}

// Exact box-area reduction with premultiplied color. Every contributing source
// pixel is read before decimation, so periodic fine detail cannot alias away.
// Cache one horizontal row across adjacent output rows; memory is O(source width
// + output width), besides the output bitmap. Native-size paths retain exact bytes.
fn reduce_area(
    source: (u32, u32),
    target: (u32, u32),
    mut render_row: impl FnMut(u32, &mut [Rgba<u8>]),
) -> RgbaImage {
    let mut output = RgbaImage::new(target.0, target.1);
    let mut row = vec![Rgba([0; 4]); source.0 as usize];
    if source == target {
        for y in 0..source.1 {
            render_row(y, &mut row);
            for (x, pixel) in row.iter().enumerate() {
                output.put_pixel(x as u32, y, *pixel);
            }
        }
        return output;
    }
    let mut horizontal = vec![[0_f64; 4]; target.0 as usize];
    let mut vertical = horizontal.clone();
    let mut cached_y = None;
    let dx = f64::from(source.0) / f64::from(target.0);
    let dy = f64::from(source.1) / f64::from(target.1);
    for oy in 0..target.1 {
        vertical.fill([0.; 4]);
        let (top, bottom) = (f64::from(oy) * dy, f64::from(oy + 1) * dy);
        for sy in top.floor() as u32..(bottom.ceil() as u32).min(source.1) {
            if cached_y != Some(sy) {
                render_row(sy, &mut row);
                for ox in 0..target.0 {
                    let (left, right) = (f64::from(ox) * dx, f64::from(ox + 1) * dx);
                    let mut sum = [0.; 4];
                    for sx in left.floor() as u32..(right.ceil() as u32).min(source.0) {
                        let weight =
                            (right.min(f64::from(sx + 1)) - left.max(f64::from(sx))).max(0.);
                        let p = row[sx as usize];
                        let alpha = f64::from(p[3]) * weight;
                        for c in 0..3 {
                            sum[c] += f64::from(p[c]) * alpha;
                        }
                        sum[3] += alpha;
                    }
                    horizontal[ox as usize] = sum.map(|v| v / dx);
                }
                cached_y = Some(sy);
            }
            let weight = (bottom.min(f64::from(sy + 1)) - top.max(f64::from(sy))).max(0.);
            for (sum, source) in vertical.iter_mut().zip(&horizontal) {
                for c in 0..4 {
                    sum[c] += source[c] * weight;
                }
            }
        }
        for (ox, sum) in vertical.iter().enumerate() {
            let alpha = (sum[3] / dy).round().clamp(0., 255.) as u8;
            let pixel = if alpha == 0 {
                Rgba([0; 4])
            } else {
                Rgba([
                    (sum[0] / sum[3]).round() as u8,
                    (sum[1] / sum[3]).round() as u8,
                    (sum[2] / sum[3]).round() as u8,
                    alpha,
                ])
            };
            output.put_pixel(ox as u32, oy, pixel);
        }
    }
    output
}

pub fn checker(image: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        let gray = if (x / 6 + y / 6) % 2 == 0 { 66 } else { 88 };
        document::source_over(Rgba([gray, gray, gray, 255]), *image.get_pixel(x, y), 255)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{BlendMode, RasterLayer};
    use crate::preview::PixelRect;
    use crate::transform::ColorMode;
    use std::sync::Arc;

    fn scene() -> DocumentContent {
        let layer = |id, pixels, offset, opacity, visible| RasterLayer {
            effect: None,
            mask: None,
            id: LayerId(id),
            name: format!("Layer {id}"),
            pixels: Arc::new(pixels),
            offset,
            visible,
            opacity,
            blend_mode: BlendMode::Normal,
            locks: Default::default(),
            text: None,
        };
        DocumentContent::from_layers(
            12,
            9,
            vec![
                layer(
                    0,
                    RgbaImage::from_pixel(12, 9, Rgba([15, 30, 50, 140])),
                    (0, 0),
                    210,
                    true,
                ),
                layer(
                    1,
                    RgbaImage::from_fn(8, 7, |x, y| {
                        Rgba([
                            20 + x as u8 * 25,
                            30 + y as u8 * 25,
                            80,
                            if x == 2 { 0 } else { 180 },
                        ])
                    }),
                    (-1, 1),
                    137,
                    true,
                ),
                layer(
                    2,
                    RgbaImage::from_pixel(4, 5, Rgba([60, 200, 100, 120])),
                    (4, 2),
                    170,
                    true,
                ),
                layer(
                    3,
                    RgbaImage::from_pixel(12, 9, Rgba([255, 0, 255, 255])),
                    (0, 0),
                    255,
                    false,
                ),
            ],
        )
        .unwrap()
    }

    #[test]
    fn display_resolution_fits_retina_portrait_and_large_screens_without_upscaling_sources() {
        assert_eq!(display_edge((1280, 720), (296., 180.), 1.), 296);
        assert_eq!(display_edge((1280, 720), (296., 180.), 2.), 592);
        assert_eq!(display_edge((720, 1280), (296., 180.), 2.), 360);
        assert_eq!(
            display_edge((4000, 3000), (4000., 3000.), 4.),
            MAX_DISPLAY_EDGE
        );
        let preview = EffectPreview::new(scene(), LayerId(1), Selection::Full, 592);
        assert_eq!(preview.original().dimensions(), (12, 9));
        assert!(preview.resized(1024).is_none());
    }

    #[test]
    fn downsampling_preserves_detail_and_cannot_mix_hidden_rgb_into_edges() {
        let pixels = RgbaImage::from_fn(64, 64, |x, y| {
            let value = if (x + y) % 2 == 0 { 0 } else { 255 };
            Rgba([value, value, value, 255])
        });
        let content = DocumentContent::single_layer("Detail".into(), pixels.clone());
        let preview = EffectPreview::new(content, LayerId(0), Selection::Full, 32);
        assert_eq!(
            *preview.original(),
            RgbaImage::from_pixel(32, 32, Rgba([128, 128, 128, 255]))
        );
        assert!(
            preview
                .original()
                .pixels()
                .all(|p| (120..=135).contains(&p[0]))
        );

        let pixels = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 0])
            }
        });
        let content = DocumentContent::single_layer("Transparent edge".into(), pixels);
        let preview = EffectPreview::new(content, LayerId(0), Selection::Full, 1);
        assert_eq!(*preview.original().get_pixel(0, 0), Rgba([255, 0, 0, 128]));
        assert_eq!(
            *preview.adjustment(Adjustment::Invert).get_pixel(0, 0),
            Rgba([0, 255, 255, 128])
        );
    }

    fn half_size_reference(image: &RgbaImage) -> RgbaImage {
        RgbaImage::from_fn(image.width() / 2, image.height() / 2, |x, y| {
            let pixels = [
                image.get_pixel(x * 2, y * 2),
                image.get_pixel(x * 2 + 1, y * 2),
                image.get_pixel(x * 2, y * 2 + 1),
                image.get_pixel(x * 2 + 1, y * 2 + 1),
            ];
            let alpha: u32 = pixels.iter().map(|p| u32::from(p[3])).sum();
            if alpha == 0 {
                return Rgba([0; 4]);
            }
            let channel = |c: usize| {
                ((pixels
                    .iter()
                    .map(|p| u32::from(p[c]) * u32::from(p[3]))
                    .sum::<u32>()
                    + alpha / 2)
                    / alpha) as u8
            };
            Rgba([channel(0), channel(1), channel(2), ((alpha + 2) / 4) as u8])
        })
    }

    #[test]
    fn periodic_detail_is_averaged_before_large_and_fractional_reductions() {
        for phase in [0, 1] {
            for checker in [false, true] {
                let pixels = RgbaImage::from_fn(64, 64, |x, y| {
                    let v = if (x + if checker { y } else { 0 } + phase) % 2 == 0 {
                        0
                    } else {
                        255
                    };
                    Rgba([v, v, v, 255])
                });
                let content = DocumentContent::single_layer("Stripes".into(), pixels);
                for edge in [16, 8, 5] {
                    let preview =
                        EffectPreview::new(content.clone(), LayerId(0), Selection::Full, edge);
                    for image in [
                        preview.original().clone(),
                        preview.adjustment(Adjustment::Invert),
                        preview.filter(FilterSpec::Blur { radius: 4 }),
                    ] {
                        assert!(
                            image
                                .pixels()
                                .all(|p| p[0].abs_diff(128) <= 10 && p[3] == 255)
                        );
                    }
                    let region = preview.region(0, (0, 0), false).unwrap();
                    assert!(region.pixels.pixels().all(|p| p[0].abs_diff(128) <= 10));
                }
            }
        }
        // Nonintegral footprint weights preserve area and include partial source pixels.
        let reduced = reduce_area((3, 1), (2, 1), |_, row| {
            row.copy_from_slice(&[
                Rgba([0, 0, 0, 255]),
                Rgba([255, 255, 255, 255]),
                Rgba([0, 0, 0, 255]),
            ])
        });
        assert!(reduced.pixels().all(|p| *p == Rgba([85, 85, 85, 255])));
    }

    #[test]
    #[ignore = "release performance measurement"]
    fn effect_preview_benchmark() {
        let content = DocumentContent::single_layer(
            "4K".into(),
            RgbaImage::from_fn(3840, 2160, |x, y| Rgba([x as u8, y as u8, 90, 255])),
        );
        let preview = EffectPreview::new(content, LayerId(0), Selection::Full, 592);
        let start = std::time::Instant::now();
        std::hint::black_box(preview.original());
        let before = start.elapsed();
        let start = std::time::Instant::now();
        std::hint::black_box(preview.adjustment(Adjustment::Invert));
        eprintln!(
            "4K area preview: before {} ms, adjustment {} ms",
            before.as_millis(),
            start.elapsed().as_millis()
        );
    }

    #[test]
    fn reduced_adjustments_map_source_pixels_before_averaging_the_composition() {
        let mut content = scene();
        content
            .resize_canvas(12, 10, crate::transform::Anchor::NorthWest)
            .unwrap();
        let original = content.composite();
        let selection = Selection::ellipse_rect_quality(PixelRect::new(0, 1, 7, 6), 12, 10, true);
        let preview = EffectPreview::new(content.clone(), LayerId(1), selection.clone(), 6);
        let adjustment = Adjustment::BrightnessContrast {
            brightness: 20,
            contrast: 65,
        };
        let mut committed = content.clone();
        committed.apply_adjustment(LayerId(1), adjustment, &selection);
        assert_eq!(
            preview.adjustment(adjustment),
            half_size_reference(&committed.composite())
        );
        let resized = preview.resized(9).unwrap();
        assert_eq!(resized.original().dimensions(), (9, 8));
        assert_eq!(preview.original().dimensions(), (6, 5));
        assert_eq!(*content.composite(), *original);
    }

    #[test]
    fn locked_alpha_previews_match_commits_without_expanding_the_silhouette() {
        let mut content = scene();
        content.set_layer_locks(
            LayerId(1),
            crate::document::LayerLocks {
                alpha: true,
                position: false,
            },
        );
        let selection = Selection::ellipse_rect_quality(PixelRect::new(0, 0, 8, 8), 12, 9, true);
        let preview = EffectPreview::new(content.clone(), LayerId(1), selection.clone(), 256);
        for spec in [
            FilterSpec::Blur { radius: 2 },
            FilterSpec::Sharpen {
                radius: 1,
                amount: 75,
            },
        ] {
            let mut committed = content.clone();
            committed.apply_filter(LayerId(1), spec, &selection);
            assert_eq!(preview.filter(spec), *committed.composite());
            for (before, after) in content
                .layer(LayerId(1))
                .unwrap()
                .pixels
                .pixels()
                .zip(committed.layer(LayerId(1)).unwrap().pixels.pixels())
            {
                assert_eq!(before[3], after[3]);
            }
        }
    }

    #[test]
    fn adjustments_match_committed_composition_with_offsets_opacity_and_soft_selection() {
        let mut levels = adjustments::Levels::default();
        levels.red = levels.red.set_gamma(180);
        for mode in [ColorMode::Rgb, ColorMode::Grayscale] {
            let mut content = scene();
            content.set_color_mode(mode);
            for selection in [
                Selection::Unrestricted,
                Selection::Empty,
                Selection::ellipse_rect_quality(PixelRect::new(0, 1, 7, 6), 12, 9, true)
                    .feather(1, 12, 9),
            ] {
                let preview =
                    EffectPreview::new(content.clone(), LayerId(1), selection.clone(), 256);
                assert_eq!(*preview.original(), *content.composite());
                for adjustment in [
                    Adjustment::Invert,
                    Adjustment::Desaturate,
                    Adjustment::BrightnessContrast {
                        brightness: 30,
                        contrast: -15,
                    },
                    Adjustment::HueSaturation {
                        hue: 75,
                        saturation: -30,
                        lightness: 10,
                    },
                    Adjustment::Levels(levels),
                    Adjustment::Curves(Default::default()),
                ] {
                    let mut committed = content.clone();
                    committed.apply_adjustment(LayerId(1), adjustment, &selection);
                    assert_eq!(
                        preview.adjustment(adjustment),
                        *committed.composite(),
                        "{adjustment:?} {mode:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn filters_and_shadows_match_commits_in_the_canvas_composition() {
        for mode in [ColorMode::Rgb, ColorMode::Grayscale] {
            let mut content = scene();
            content.set_color_mode(mode);
            let selection =
                Selection::ellipse_rect_quality(PixelRect::new(0, 0, 8, 8), 12, 9, true);
            let preview = EffectPreview::new(content.clone(), LayerId(1), selection.clone(), 256);
            for spec in [
                FilterSpec::Blur { radius: 2 },
                FilterSpec::Sharpen {
                    radius: 1,
                    amount: 65,
                },
                FilterSpec::Blur { radius: 0 },
                FilterSpec::Shadow {
                    radius: 2,
                    offset: (3, -2),
                    opacity: 67,
                    color: Rgba([30, 150, 220, 100]),
                },
                FilterSpec::Shadow {
                    radius: 0,
                    offset: (-2, 1),
                    opacity: 100,
                    color: Rgba([10, 20, 30, 255]),
                },
            ] {
                let mut committed = content.clone();
                if matches!(spec, FilterSpec::Shadow { .. }) {
                    committed.place_drop_shadow(LayerId(1), spec).unwrap();
                } else {
                    committed.apply_filter(LayerId(1), spec, &selection);
                }
                assert_eq!(
                    preview.filter(spec),
                    *committed.composite(),
                    "{spec:?} {mode:?}"
                );
            }
        }
        // Shadows can enter the canvas from an entirely off-canvas source,
        // and are independent of that source layer's opacity and selection.
        let mut content = scene();
        content.set_offset(LayerId(1), (-100, 1));
        content.set_opacity(LayerId(1), 0);
        let spec = FilterSpec::Shadow {
            radius: 2,
            offset: (103, 1),
            opacity: 80,
            color: Rgba([20, 30, 40, 255]),
        };
        let preview = EffectPreview::new(content.clone(), LayerId(1), Selection::Empty, 256);
        content.place_drop_shadow(LayerId(1), spec).unwrap();
        assert_eq!(preview.filter(spec), *content.composite());
    }

    #[test]
    fn small_canvas_filters_keep_detail_and_off_canvas_sampling_from_a_large_layer() {
        let layer = RasterLayer {
            effect: None,
            mask: None,
            id: LayerId(0),
            name: "Large".into(),
            pixels: Arc::new(RgbaImage::from_fn(1024, 30, |x, y| {
                Rgba([if x == 499 { 255 } else { 0 }, (y * 7) as u8, 40, 255])
            })),
            offset: (-500, -10),
            visible: true,
            opacity: 180,
            blend_mode: BlendMode::Normal,
            locks: Default::default(),
            text: None,
        };
        let content = DocumentContent::from_layers(8, 6, vec![layer]).unwrap();
        let preview = EffectPreview::new(content.clone(), LayerId(0), Selection::Full, 256);
        for spec in [
            FilterSpec::Blur { radius: 2 },
            FilterSpec::Sharpen {
                radius: 2,
                amount: 50,
            },
            FilterSpec::Shadow {
                radius: 1,
                offset: (3, 2),
                opacity: 100,
                color: Rgba([20, 30, 40, 255]),
            },
        ] {
            let mut committed = content.clone();
            if matches!(spec, FilterSpec::Shadow { .. }) {
                committed.place_drop_shadow(LayerId(0), spec).unwrap();
            } else {
                committed.apply_filter(LayerId(0), spec, &Selection::Full);
            }
            assert_eq!(preview.filter(spec), *committed.composite(), "{spec:?}");
        }
        assert_ne!(
            preview.filter(FilterSpec::Blur { radius: 2 }),
            *preview.original()
        );
    }

    #[test]
    fn reduced_previews_sample_the_canvas_and_bound_filter_scratch_space() {
        let layer = scene().layers()[1].clone();
        let content = DocumentContent::from_layers(16_384, 2, vec![layer]).unwrap();
        let preview = EffectPreview::new(content.clone(), LayerId(1), Selection::Full, 256);
        assert_eq!(preview.original().dimensions(), (256, 1));
        let adjusted = preview.adjustment(Adjustment::Invert);
        assert!(adjusted.get_pixel(0, 0)[3] > 0);
        assert!(adjusted.pixels().skip(1).all(|p| p[3] == 0));
        assert_ne!(adjusted, *preview.original());
        let max_pad = filters::kernel_support(filters::MAX_RADIUS);
        for offset in [(0, 0), (i32::MIN, i32::MAX), (i32::MAX, i32::MIN)] {
            if let Some(region) = preview.region(filters::MAX_RADIUS, offset, false) {
                assert!(region.pixels.width() <= 512 + 2 * max_pad + 1);
                assert!(region.pixels.height() <= 512 + 2 * max_pad + 1);
            }
        }
        let blur = preview.filter(FilterSpec::Blur { radius: 1 });
        assert_eq!(blur, *preview.original()); // Subpixel kernel, no resampling artifacts.

        let mut large_layer = content.layers()[0].clone();
        large_layer.pixels = Arc::new(RgbaImage::from_pixel(1024, 1024, Rgba([30, 50, 80, 180])));
        large_layer.offset = (-200, -200);
        let content = DocumentContent::from_layers(128, 128, vec![large_layer]).unwrap();
        let preview = EffectPreview::new(content, LayerId(1), Selection::Full, 32);
        let region = preview.region(filters::MAX_RADIUS, (0, 0), false).unwrap();
        // Filter inputs use twice the display resolution.
        assert_eq!(region.pixels.dimensions(), (160, 160));
        assert_eq!(
            preview
                .filter(FilterSpec::Blur {
                    radius: filters::MAX_RADIUS
                })
                .dimensions(),
            (32, 32)
        );
    }

    #[test]
    fn comparison_snapshot_and_checker_stay_stable_without_mutating_document_pixels() {
        let mut content = scene();
        let preview = EffectPreview::new(content.clone(), LayerId(1), Selection::Full, 256);
        let before = content.composite();
        let _after = preview.adjustment(Adjustment::Invert);
        assert_eq!(*content.composite(), *before);
        content.apply_adjustment(LayerId(1), Adjustment::Invert, &Selection::Full);
        assert_eq!(*preview.original(), *before);
        assert_ne!(*preview.original(), *content.composite());
        let clear = RgbaImage::new(12, 1);
        let checked = checker(&clear);
        assert_ne!(checked.get_pixel(0, 0), checked.get_pixel(6, 0));
        assert!(checked.pixels().all(|pixel| pixel[3] == 255));
        assert!(clear.pixels().all(|pixel| pixel[3] == 0));
    }
}
