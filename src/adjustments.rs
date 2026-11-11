use image::{Rgba, RgbaImage};

pub const PREVIEW_EDGE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adjustment {
    BrightnessContrast {
        brightness: i16,
        contrast: i16,
    },
    HueSaturation {
        hue: i16,
        saturation: i16,
        lightness: i16,
    },
    Desaturate,
    Invert,
}

impl Adjustment {
    pub fn is_identity(self) -> bool {
        match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => brightness == 0 && contrast == 0,
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
            } => hue == 0 && saturation == 0 && lightness == 0,
            Self::Desaturate | Self::Invert => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreviewGeneration {
    latest: u64,
    shown: u64,
}

impl PreviewGeneration {
    pub fn request(&mut self) -> u64 {
        self.latest = self.latest.saturating_add(1);
        self.latest
    }

    pub fn latest(self) -> u64 {
        self.latest
    }

    #[cfg(test)]
    pub fn shown(self) -> u64 {
        self.shown
    }

    // A finished preview is shown only when no newer slider change is waiting.
    pub fn finish(&mut self, generation: u64) -> bool {
        if generation == self.latest && generation != 0 {
            self.shown = generation;
            true
        } else {
            false
        }
    }
}

pub struct Adjusted {
    pub pixels: RgbaImage,
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

pub fn adjust_pixel(pixel: Rgba<u8>, adjustment: Adjustment) -> Rgba<u8> {
    let [r, g, b, a] = pixel.0;
    let [r, g, b] = match adjustment {
        Adjustment::BrightnessContrast {
            brightness,
            contrast,
        } => [r, g, b].map(|channel| brightness_contrast(channel, brightness, contrast)),
        Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
        } => hue_saturation(r, g, b, hue, saturation, lightness),
        Adjustment::Desaturate => {
            let gray =
                ((u16::from(r) * 77 + u16::from(g) * 150 + u16::from(b) * 29 + 128) >> 8) as u8;
            [gray, gray, gray]
        }
        Adjustment::Invert => [255 - r, 255 - g, 255 - b],
    };
    Rgba([r, g, b, a])
}

pub fn apply_pixels(
    source: &RgbaImage,
    adjustment: Adjustment,
    coverage: impl Fn(u32, u32) -> u8,
    constrain: impl Fn(Rgba<u8>) -> Rgba<u8>,
) -> Option<Adjusted> {
    if adjustment.is_identity() {
        return None;
    }
    let (width, height) = source.dimensions();
    let mut image = None;
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let amount = coverage(x, y);
            if amount == 0 {
                continue;
            }
            let before = *source.get_pixel(x, y);
            let adjusted = constrain(adjust_pixel(before, adjustment));
            let after = blend_coverage(before, adjusted, amount);
            if before == after {
                continue;
            }
            image
                .get_or_insert_with(|| source.clone())
                .put_pixel(x, y, after);
            bounds = Some(match bounds {
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x), bottom.max(y))
                }
                None => (x, y, x, y),
            });
        }
    }
    let (left, top, right, bottom) = bounds?;
    Some(Adjusted {
        pixels: image?,
        left,
        top,
        right,
        bottom,
    })
}

pub fn reduced_preview(
    source: &RgbaImage,
    adjustment: Adjustment,
    coverage: impl Fn(u32, u32) -> u8,
    constrain: impl Fn(Rgba<u8>) -> Rgba<u8>,
    max_edge: u32,
) -> RgbaImage {
    let (width, height) = source.dimensions();
    let (preview_width, preview_height) = preview_size(width, height, max_edge);
    RgbaImage::from_fn(preview_width, preview_height, |x, y| {
        let sx = sample_coord(x, preview_width, width);
        let sy = sample_coord(y, preview_height, height);
        let before = *source.get_pixel(sx, sy);
        let amount = coverage(sx, sy);
        if amount == 0 || adjustment.is_identity() {
            return before;
        }
        blend_coverage(before, constrain(adjust_pixel(before, adjustment)), amount)
    })
}

pub fn preview_size(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let edge = width.max(height);
    if edge <= max_edge || max_edge == 0 || edge == 0 {
        return (width.max(1), height.max(1));
    }
    let scaled = |value: u32| {
        ((u64::from(value) * u64::from(max_edge) + u64::from(edge) / 2) / u64::from(edge)).max(1)
            as u32
    };
    (scaled(width), scaled(height))
}

fn sample_coord(index: u32, preview: u32, source: u32) -> u32 {
    if preview == 0 || source == 0 {
        return 0;
    }
    ((u64::from(index) * u64::from(source)) / u64::from(preview)).min(u64::from(source - 1)) as u32
}

fn blend_coverage(before: Rgba<u8>, adjusted: Rgba<u8>, coverage: u8) -> Rgba<u8> {
    if coverage == 0 {
        return before;
    }
    let color = if coverage == 255 {
        adjusted
    } else {
        let mix = |from: u8, to: u8| {
            ((u32::from(from) * u32::from(255 - coverage)
                + u32::from(to) * u32::from(coverage)
                + 127)
                / 255) as u8
        };
        Rgba([
            mix(before.0[0], adjusted.0[0]),
            mix(before.0[1], adjusted.0[1]),
            mix(before.0[2], adjusted.0[2]),
            adjusted.0[3],
        ])
    };
    Rgba([color.0[0], color.0[1], color.0[2], before.0[3]])
}

fn brightness_contrast(channel: u8, brightness: i16, contrast: i16) -> u8 {
    let contrast = f64::from(contrast.clamp(-100, 100));
    let mapped = contrast / 100.0 * 255.0;
    let factor = (259.0 * (mapped + 255.0)) / (255.0 * (259.0 - mapped));
    let bright = f64::from(brightness.clamp(-100, 100)) / 100.0 * 255.0;
    let value = factor * (f64::from(channel) - 128.0) + 128.0 + bright;
    value.round().clamp(0.0, 255.0) as u8
}

fn hue_saturation(r: u8, g: u8, b: u8, hue: i16, saturation: i16, lightness: i16) -> [u8; 3] {
    let (mut h, mut s, mut l) = rgb_to_hsl(r, g, b);
    h = (h + f64::from(hue.clamp(-180, 180)) / 360.0).rem_euclid(1.0);
    let sat = f64::from(saturation.clamp(-100, 100)) / 100.0;
    s = if sat < 0.0 {
        s * (1.0 + sat)
    } else if s == 0.0 {
        0.0
    } else {
        s + (1.0 - s) * sat
    };
    l = (l + f64::from(lightness.clamp(-100, 100)) / 100.0).clamp(0.0, 1.0);
    hsl_to_rgb(h, s.clamp(0.0, 1.0), l)
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let [r, g, b] = [r, g, b].map(|channel| f64::from(channel) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let delta = max - min;
    if delta < 1e-8 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let h = if max == r {
        (g - b) / delta + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [u8; 3] {
    if s <= 0.0 {
        let gray = (l * 255.0).round().clamp(0.0, 255.0) as u8;
        return [gray, gray, gray];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    [
        hue_channel(p, q, h + 1.0 / 3.0),
        hue_channel(p, q, h),
        hue_channel(p, q, h - 1.0 / 3.0),
    ]
}

fn hue_channel(p: f64, q: f64, mut t: f64) -> u8 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    let value = if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    };
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(pixel: Rgba<u8>) -> Rgba<u8> {
        pixel
    }

    #[test]
    fn identity_settings_leave_every_channel_unchanged() {
        let pixel = Rgba([12, 34, 56, 78]);
        for adjustment in [
            Adjustment::BrightnessContrast {
                brightness: 0,
                contrast: 0,
            },
            Adjustment::HueSaturation {
                hue: 0,
                saturation: 0,
                lightness: 0,
            },
        ] {
            assert!(adjustment.is_identity());
            assert!(
                apply_pixels(
                    &RgbaImage::from_pixel(2, 2, pixel),
                    adjustment,
                    |_, _| 255,
                    plain
                )
                .is_none()
            );
        }
    }

    #[test]
    fn brightness_contrast_hue_desaturate_and_invert_match_their_fixtures() {
        let red = Rgba([255, 0, 0, 200]);
        assert_eq!(
            adjust_pixel(
                Rgba([0, 10, 255, 40]),
                Adjustment::BrightnessContrast {
                    brightness: 100,
                    contrast: 0,
                },
            )
            .0,
            [255, 255, 255, 40]
        );
        assert_eq!(
            adjust_pixel(
                Rgba([200, 128, 64, 9]),
                Adjustment::BrightnessContrast {
                    brightness: 0,
                    contrast: 0,
                },
            )
            .0,
            [200, 128, 64, 9]
        );
        assert_eq!(
            brightness_contrast(64, 0, -100),
            128,
            "full negative contrast flattens to the midpoint"
        );
        assert_eq!(
            adjust_pixel(
                red,
                Adjustment::HueSaturation {
                    hue: 120,
                    saturation: 0,
                    lightness: 0,
                },
            )
            .0,
            [0, 255, 0, 200]
        );
        assert_eq!(
            adjust_pixel(
                red,
                Adjustment::HueSaturation {
                    hue: 0,
                    saturation: -100,
                    lightness: 0,
                },
            )
            .0,
            [128, 128, 128, 200]
        );
        assert_eq!(
            adjust_pixel(
                Rgba([255, 40, 0, 255]),
                Adjustment::BrightnessContrast {
                    brightness: -100,
                    contrast: 0,
                },
            )
            .0,
            [0, 0, 0, 255]
        );
        assert_eq!(
            adjust_pixel(
                red,
                Adjustment::HueSaturation {
                    hue: 0,
                    saturation: 0,
                    lightness: 100,
                },
            )
            .0,
            [255, 255, 255, 200]
        );
        assert_eq!(
            adjust_pixel(
                Rgba([128, 128, 128, 255]),
                Adjustment::HueSaturation {
                    hue: 40,
                    saturation: 100,
                    lightness: 0,
                },
            )
            .0,
            [128, 128, 128, 255]
        );
        assert_eq!(
            adjust_pixel(red, Adjustment::Desaturate).0,
            [77, 77, 77, 200]
        );
        assert_eq!(
            adjust_pixel(Rgba([1, 2, 3, 9]), Adjustment::Invert).0,
            [254, 253, 252, 9]
        );
        assert_eq!(
            adjust_pixel(
                adjust_pixel(Rgba([1, 2, 3, 9]), Adjustment::Invert),
                Adjustment::Invert,
            )
            .0,
            [1, 2, 3, 9]
        );
    }

    #[test]
    fn coverage_blends_the_effect_and_keeps_the_original_alpha() {
        let source = RgbaImage::from_pixel(1, 3, Rgba([0, 0, 0, 40]));
        let adjusted = apply_pixels(
            &source,
            Adjustment::Invert,
            |_, y| if y == 0 { 0 } else { 128 },
            plain,
        )
        .unwrap();
        assert_eq!(adjusted.pixels.get_pixel(0, 0).0, [0, 0, 0, 40]);
        assert_eq!(adjusted.pixels.get_pixel(0, 1).0, [128, 128, 128, 40]);
        assert_eq!(adjusted.top, 1);
    }

    #[test]
    fn a_stale_preview_does_not_replace_the_latest_slider_change() {
        let mut preview = PreviewGeneration::default();
        let first = preview.request();
        let second = preview.request();
        assert!(!preview.finish(first));
        assert_eq!(preview.shown(), 0);
        assert!(preview.finish(second));
        assert_eq!(preview.shown(), second);
        assert!(!preview.finish(first));
        assert_eq!(preview.shown(), second);
    }

    #[test]
    fn the_reduced_preview_samples_the_source_and_stays_inside_the_edge() {
        let mut source = RgbaImage::new(8, 2);
        source.put_pixel(0, 0, Rgba([10, 0, 0, 255]));
        source.put_pixel(2, 0, Rgba([20, 0, 0, 255]));
        let preview = reduced_preview(&source, Adjustment::Invert, |_, _| 255, plain, 4);
        assert_eq!(preview.dimensions(), (4, 1));
        assert_eq!(preview.get_pixel(0, 0).0, [245, 255, 255, 255]);
        assert_eq!(preview.get_pixel(1, 0).0, [235, 255, 255, 255]);
    }
}
