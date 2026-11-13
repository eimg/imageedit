use std::sync::OnceLock;

use image::{Rgba, RgbaImage};

use crate::adjustments::{self, Adjustment};
use crate::document::{self, BlendMode, DocumentContent, LayerId};
use crate::filters::{self, FilterSpec};
use crate::selection::Selection;

// An immutable dialog snapshot. Work and allocations are bounded by the preview
// edge, even when a layer is much larger than (or far outside) the canvas.
pub struct EffectPreview {
    content: DocumentContent,
    layer: LayerId,
    selection: Selection,
    size: (u32, u32),
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
        Self {
            content,
            layer,
            selection,
            size: adjustments::preview_size(w, h, edge.max(1)),
            original: OnceLock::new(),
        }
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
        self.composite(
            |pixel, x, y| {
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
            let Some(region) = self.region(radius, offset) else {
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
        let Some(region) = self.region(radius, (0, 0)) else {
            return self.original().clone();
        };
        let radius = (f64::from(radius) * region.scale).round() as u32;
        // A subpixel radius cannot be represented in this reduced preview.
        // Leave it unchanged instead of showing a resampling artifact as an effect.
        if radius == 0 {
            return self.original().clone();
        }
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
        );
        self.composite(
            |pixel, x, y| {
                let sx = ((x - region.origin.0) as f64 * region.scale).floor() as u32;
                let sy = ((y - region.origin.1) as f64 * region.scale).floor() as u32;
                let target = *filtered
                    .get_pixel(sx.min(filtered.width() - 1), sy.min(filtered.height() - 1));
                filters::blend(pixel, target, self.coverage(x, y))
            },
            None,
        )
    }

    // Retain the visible source plus the filter's complete sampling halo. This
    // avoids squeezing a huge layer into a tiny preview of a small canvas, and
    // includes off-canvas pixels that can blur or cast a shadow onto the canvas.
    fn region(&self, radius: u32, offset: (i32, i32)) -> Option<Region> {
        let layer = self.content.layer(self.layer)?;
        let canvas = self.content.size();
        let scale = f64::from(self.size.0.max(self.size.1)) / f64::from(canvas.0.max(canvas.1));
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
        let pixels = RgbaImage::from_fn(w, h, |x, y| {
            let sx = (left + (f64::from(x) / scale).floor() as i64).min(right - 1) as u32;
            let sy = (top + (f64::from(y) / scale).floor() as i64).min(bottom - 1) as u32;
            *layer.pixels.get_pixel(sx, sy)
        });
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
        let (w, h) = self.size;
        let canvas = self.content.size();
        let mut image = RgbaImage::new(w, h);
        for layer in self.content.layers() {
            // A committed shadow is a separate, fully opaque layer immediately
            // below its source. Neither source opacity nor the selection clips it.
            if layer.id == self.layer
                && let Some(shadow) = shadow
            {
                for y in 0..h {
                    for x in 0..w {
                        let cx = i64::from(sample(x, w, canvas.0)) - shadow.origin.0;
                        let cy = i64::from(sample(y, h, canvas.1)) - shadow.origin.1;
                        let sx =
                            (cx as f64 * shadow.scale).floor() as i64 + i64::from(shadow.image.pad);
                        let sy =
                            (cy as f64 * shadow.scale).floor() as i64 + i64::from(shadow.image.pad);
                        if sx >= 0
                            && sy >= 0
                            && sx < i64::from(shadow.image.pixels.width())
                            && sy < i64::from(shadow.image.pixels.height())
                        {
                            let destination = image.get_pixel_mut(x, y);
                            *destination = document::source_over(
                                *destination,
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
            for y in 0..h {
                let cy = i64::from(sample(y, h, canvas.1));
                let sy = cy - i64::from(layer.offset.1);
                if sy < 0 || sy >= i64::from(layer.pixels.height()) {
                    continue;
                }
                for x in 0..w {
                    let cx = i64::from(sample(x, w, canvas.0));
                    let sx = cx - i64::from(layer.offset.0);
                    if sx < 0 || sx >= i64::from(layer.pixels.width()) {
                        continue;
                    }
                    let mut pixel = *layer.pixels.get_pixel(sx as u32, sy as u32);
                    if layer.id == self.layer {
                        pixel = map(pixel, cx, cy);
                    }
                    let destination = image.get_pixel_mut(x, y);
                    *destination = match layer.blend_mode {
                        BlendMode::Normal => {
                            document::source_over(*destination, pixel, layer.opacity)
                        }
                    };
                }
            }
        }
        image
    }
}

fn sample(index: u32, source: u32, target: u32) -> u32 {
    (u64::from(index) * u64::from(target) / u64::from(source.max(1))) as u32
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
            id: LayerId(id),
            name: format!("Layer {id}"),
            pixels: Arc::new(pixels),
            offset,
            visible,
            opacity,
            blend_mode: BlendMode::Normal,
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
            id: LayerId(0),
            name: "Large".into(),
            pixels: Arc::new(RgbaImage::from_fn(1024, 30, |x, y| {
                Rgba([if x == 499 { 255 } else { 0 }, (y * 7) as u8, 40, 255])
            })),
            offset: (-500, -10),
            visible: true,
            opacity: 180,
            blend_mode: BlendMode::Normal,
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
        let mut committed = content.clone();
        committed.apply_adjustment(LayerId(1), Adjustment::Invert, &Selection::Full);
        let flat = committed.composite();
        let expected = RgbaImage::from_fn(256, 1, |x, _| *flat.get_pixel(x * 64, 0));
        assert_eq!(preview.adjustment(Adjustment::Invert), expected);
        let max_pad = filters::kernel_support(filters::MAX_RADIUS);
        for offset in [(0, 0), (i32::MIN, i32::MAX), (i32::MAX, i32::MIN)] {
            if let Some(region) = preview.region(filters::MAX_RADIUS, offset) {
                assert!(region.pixels.width() <= 256 + 2 * max_pad + 1);
                assert!(region.pixels.height() <= 256 + 2 * max_pad + 1);
            }
        }
        let blur = preview.filter(FilterSpec::Blur { radius: 1 });
        assert_eq!(blur, *preview.original()); // Subpixel kernel, no resampling artifacts.

        let mut large_layer = content.layers()[0].clone();
        large_layer.pixels = Arc::new(RgbaImage::from_pixel(1024, 1024, Rgba([30, 50, 80, 180])));
        large_layer.offset = (-200, -200);
        let content = DocumentContent::from_layers(128, 128, vec![large_layer]).unwrap();
        let preview = EffectPreview::new(content, LayerId(1), Selection::Full, 32);
        let region = preview.region(filters::MAX_RADIUS, (0, 0)).unwrap();
        assert_eq!(region.pixels.dimensions(), (80, 80));
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
