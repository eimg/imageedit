use image::{Rgba, RgbaImage};

pub const PREVIEW_EDGE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    ColorBalance(ColorBalance),
    Levels(Levels),
    Curves(Curves),
}

impl Adjustment {
    pub fn valid(self) -> bool {
        match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => (-100..=100).contains(&brightness) && (-100..=100).contains(&contrast),
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
            } => {
                (-180..=180).contains(&hue)
                    && (-100..=100).contains(&saturation)
                    && (-100..=100).contains(&lightness)
            }
            Self::Desaturate | Self::Invert => true,
            Self::ColorBalance(value) => [value.shadows, value.midtones, value.highlights]
                .into_iter()
                .flatten()
                .all(|value| (-100..=100).contains(&value)),
            Self::Levels(value) => [value.value, value.red, value.green, value.blue]
                .into_iter()
                .all(|stops| {
                    stops.input_black < stops.input_white
                        && (GAMMA_MIN..=GAMMA_MAX).contains(&stops.gamma)
                }),
            Self::Curves(value) => [value.value, value.red, value.green, value.blue]
                .into_iter()
                .all(|curve| {
                    (2..=MAX_CURVE_POINTS).contains(&(curve.count as usize))
                        && curve.points().first().is_some_and(|point| point.0 == 0)
                        && curve.points().last().is_some_and(|point| point.0 == 255)
                        && curve.points().windows(2).all(|pair| pair[0].0 < pair[1].0)
                }),
        }
    }

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
            Self::ColorBalance(balance) => balance.is_identity(),
            Self::Levels(levels) => levels.is_identity(),
            Self::Curves(curves) => curves.is_identity(),
        }
    }
}

pub const MAX_CURVE_POINTS: usize = 16;
pub const GAMMA_ONE: u16 = 100;
pub const GAMMA_MIN: u16 = 10;
pub const GAMMA_MAX: u16 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BalanceRange {
    Shadows,
    Midtones,
    Highlights,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MapChannel {
    Value,
    Red,
    Green,
    Blue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ColorBalance {
    pub shadows: [i16; 3],
    pub midtones: [i16; 3],
    pub highlights: [i16; 3],
    pub preserve_luminosity: bool,
}

impl Default for ColorBalance {
    fn default() -> Self {
        Self {
            shadows: [0; 3],
            midtones: [0; 3],
            highlights: [0; 3],
            preserve_luminosity: true,
        }
    }
}

impl ColorBalance {
    pub fn is_identity(self) -> bool {
        self.shadows == [0; 3] && self.midtones == [0; 3] && self.highlights == [0; 3]
    }

    pub fn shift(self, range: BalanceRange) -> [i16; 3] {
        match range {
            BalanceRange::Shadows => self.shadows,
            BalanceRange::Midtones => self.midtones,
            BalanceRange::Highlights => self.highlights,
        }
    }

    pub fn set_shift(mut self, range: BalanceRange, index: usize, value: i16) -> Self {
        let Some(slot) = (match range {
            BalanceRange::Shadows => self.shadows.get_mut(index),
            BalanceRange::Midtones => self.midtones.get_mut(index),
            BalanceRange::Highlights => self.highlights.get_mut(index),
        }) else {
            return self;
        };
        *slot = value.clamp(-100, 100);
        self
    }

    pub fn set_preserve_luminosity(mut self, preserve: bool) -> Self {
        self.preserve_luminosity = preserve;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LevelStops {
    pub input_black: u8,
    pub input_white: u8,
    pub gamma: u16,
    pub output_black: u8,
    pub output_white: u8,
}

impl LevelStops {
    pub const IDENTITY: Self = Self {
        input_black: 0,
        input_white: 255,
        gamma: GAMMA_ONE,
        output_black: 0,
        output_white: 255,
    };

    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    pub fn set_input_black(mut self, value: u8) -> Self {
        self.input_black = value.min(self.input_white.saturating_sub(1));
        self
    }

    pub fn set_input_white(mut self, value: u8) -> Self {
        self.input_white = value.max(self.input_black.saturating_add(1));
        self
    }

    pub fn set_gamma(mut self, gamma: u16) -> Self {
        self.gamma = gamma.clamp(GAMMA_MIN, GAMMA_MAX);
        self
    }

    pub fn set_output_black(mut self, value: u8) -> Self {
        self.output_black = value;
        self
    }

    pub fn set_output_white(mut self, value: u8) -> Self {
        self.output_white = value;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Levels {
    pub value: LevelStops,
    pub red: LevelStops,
    pub green: LevelStops,
    pub blue: LevelStops,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            value: LevelStops::IDENTITY,
            red: LevelStops::IDENTITY,
            green: LevelStops::IDENTITY,
            blue: LevelStops::IDENTITY,
        }
    }
}

impl Levels {
    pub fn is_identity(self) -> bool {
        [self.value, self.red, self.green, self.blue]
            .into_iter()
            .all(LevelStops::is_identity)
    }

    pub fn stops(self, channel: MapChannel) -> LevelStops {
        match channel {
            MapChannel::Value => self.value,
            MapChannel::Red => self.red,
            MapChannel::Green => self.green,
            MapChannel::Blue => self.blue,
        }
    }

    pub fn set_stops(mut self, channel: MapChannel, stops: LevelStops) -> Self {
        match channel {
            MapChannel::Value => self.value = stops,
            MapChannel::Red => self.red = stops,
            MapChannel::Green => self.green = stops,
            MapChannel::Blue => self.blue = stops,
        }
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Curve {
    count: u8,
    points: [(u8, u8); MAX_CURVE_POINTS],
}

impl Default for Curve {
    fn default() -> Self {
        Self::identity()
    }
}

impl Curve {
    pub const fn identity() -> Self {
        let mut points = [(0, 0); MAX_CURVE_POINTS];
        points[1] = (255, 255);
        Self { count: 2, points }
    }

    pub fn points(&self) -> &[(u8, u8)] {
        &self.points[..self.count as usize]
    }

    pub fn point_count(self) -> usize {
        self.count as usize
    }

    #[cfg(test)]
    pub fn endpoints(self) -> ((u8, u8), (u8, u8)) {
        let last = self.point_count() - 1;
        (self.points[0], self.points[last])
    }

    pub fn is_identity(self) -> bool {
        self.points().iter().all(|(x, y)| x == y)
    }

    pub fn map(self, channel: u8) -> u8 {
        let points = self.points();
        if channel <= points[0].0 {
            return points[0].1;
        }
        for pair in points.windows(2) {
            let (x0, y0) = pair[0];
            let (x1, y1) = pair[1];
            if channel <= x1 {
                return interpolate(x0, y0, x1, y1, channel);
            }
        }
        points[points.len() - 1].1
    }

    pub fn try_add(mut self, x: u8, y: u8) -> Option<Self> {
        let count = self.point_count();
        if count >= MAX_CURVE_POINTS || self.points().iter().any(|point| point.0 == x) {
            return None;
        }
        let insert = self
            .points()
            .iter()
            .position(|point| point.0 > x)
            .unwrap_or(count);
        for index in (insert..count).rev() {
            self.points[index + 1] = self.points[index];
        }
        self.points[insert] = (x, y);
        self.count += 1;
        Some(self)
    }

    pub fn move_point(mut self, index: usize, x: u8, y: u8) -> Self {
        let count = self.point_count();
        if index >= count {
            return self;
        }
        if index == 0 {
            self.points[0].1 = y;
            return self;
        }
        if index + 1 == count {
            self.points[index].1 = y;
            return self;
        }
        let left = self.points[index - 1].0.saturating_add(1);
        let right = self.points[index + 1].0.saturating_sub(1);
        if left > right {
            return self;
        }
        self.points[index] = (x.clamp(left, right), y);
        self
    }

    pub fn remove_point(mut self, index: usize) -> Self {
        let count = self.point_count();
        if index == 0 || index + 1 >= count {
            return self;
        }
        for next in index + 1..count {
            self.points[next - 1] = self.points[next];
        }
        self.count -= 1;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Curves {
    pub value: Curve,
    pub red: Curve,
    pub green: Curve,
    pub blue: Curve,
}

impl Curves {
    pub fn is_identity(self) -> bool {
        [self.value, self.red, self.green, self.blue]
            .into_iter()
            .all(Curve::is_identity)
    }

    pub fn curve(self, channel: MapChannel) -> Curve {
        match channel {
            MapChannel::Value => self.value,
            MapChannel::Red => self.red,
            MapChannel::Green => self.green,
            MapChannel::Blue => self.blue,
        }
    }

    pub fn set_curve(mut self, channel: MapChannel, curve: Curve) -> Self {
        match channel {
            MapChannel::Value => self.value = curve,
            MapChannel::Red => self.red = curve,
            MapChannel::Green => self.green = curve,
            MapChannel::Blue => self.blue = curve,
        }
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelLut {
    pub red: [u8; 256],
    pub green: [u8; 256],
    pub blue: [u8; 256],
}

impl ChannelLut {
    #[cfg(test)]
    pub fn is_identity(self) -> bool {
        (0..256).all(|index| {
            let index = index as u8;
            self.red[index as usize] == index
                && self.green[index as usize] == index
                && self.blue[index as usize] == index
        })
    }

    pub fn map(self, red: u8, green: u8, blue: u8) -> [u8; 3] {
        [
            self.red[red as usize],
            self.green[green as usize],
            self.blue[blue as usize],
        ]
    }

    fn from_levels(levels: Levels) -> Self {
        let mut lut = Self {
            red: [0; 256],
            green: [0; 256],
            blue: [0; 256],
        };
        for value in 0..=255 {
            let shared = level_map(value, levels.value);
            lut.red[value as usize] = level_map(shared, levels.red);
            lut.green[value as usize] = level_map(shared, levels.green);
            lut.blue[value as usize] = level_map(shared, levels.blue);
        }
        lut
    }

    fn from_curves(curves: Curves) -> Self {
        let mut lut = Self {
            red: [0; 256],
            green: [0; 256],
            blue: [0; 256],
        };
        for value in 0..=255 {
            let shared = curves.value.map(value);
            lut.red[value as usize] = curves.red.map(shared);
            lut.green[value as usize] = curves.green.map(shared);
            lut.blue[value as usize] = curves.blue.map(shared);
        }
        lut
    }
}

#[cfg(test)]
pub fn channel_lut(adjustment: Adjustment) -> Option<ChannelLut> {
    match adjustment {
        Adjustment::Levels(levels) => Some(ChannelLut::from_levels(levels)),
        Adjustment::Curves(curves) => Some(ChannelLut::from_curves(curves)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Histogram {
    pub red: [u32; 256],
    pub green: [u32; 256],
    pub blue: [u32; 256],
    pub value: [u32; 256],
    samples: u32,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            red: [0; 256],
            green: [0; 256],
            blue: [0; 256],
            value: [0; 256],
            samples: 0,
        }
    }
}

impl Histogram {
    pub fn is_empty(self) -> bool {
        self.samples == 0
    }

    pub fn bins(self, channel: MapChannel) -> [u32; 256] {
        match channel {
            MapChannel::Value => self.value,
            MapChannel::Red => self.red,
            MapChannel::Green => self.green,
            MapChannel::Blue => self.blue,
        }
    }

    #[cfg(test)]
    pub fn is_flat(self, channel: MapChannel) -> bool {
        let bins = self.bins(channel);
        let count = bins[0];
        count > 0 && bins.iter().all(|bin| *bin == count)
    }

    #[cfg(test)]
    pub fn range(self, channel: MapChannel) -> Option<(u8, u8)> {
        let bins = self.bins(channel);
        let first = bins.iter().position(|bin| *bin > 0)?;
        let last = bins.iter().rposition(|bin| *bin > 0)?;
        Some((first as u8, last as u8))
    }
}

pub fn histogram(image: &RgbaImage, coverage: impl Fn(u32, u32) -> u8) -> Histogram {
    let mut histogram = Histogram::default();
    for (x, y, pixel) in image.enumerate_pixels() {
        if coverage(x, y) == 0 || pixel.0[3] == 0 {
            continue;
        }
        let [red, green, blue, _] = pixel.0;
        histogram.red[red as usize] += 1;
        histogram.green[green as usize] += 1;
        histogram.blue[blue as usize] += 1;
        let value = ((u16::from(red) * 77 + u16::from(green) * 150 + u16::from(blue) * 29 + 128)
            >> 8) as usize;
        histogram.value[value] += 1;
        histogram.samples += 1;
    }
    histogram
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

    pub fn pending(self) -> bool {
        self.latest != self.shown
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

#[cfg(test)]
pub fn adjust_pixel(pixel: Rgba<u8>, adjustment: Adjustment) -> Rgba<u8> {
    Prepared::from(adjustment).map(pixel)
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
    let prepared = Prepared::from(adjustment);
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
            let adjusted = constrain(prepared.map(before));
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

pub(crate) fn blend_coverage(before: Rgba<u8>, adjusted: Rgba<u8>, coverage: u8) -> Rgba<u8> {
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

pub(crate) enum Prepared {
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
    ColorBalance(ColorBalance),
    Lut(Box<ChannelLut>),
}

impl Prepared {
    pub(crate) fn from(adjustment: Adjustment) -> Self {
        match adjustment {
            Adjustment::BrightnessContrast {
                brightness,
                contrast,
            } => Self::BrightnessContrast {
                brightness,
                contrast,
            },
            Adjustment::HueSaturation {
                hue,
                saturation,
                lightness,
            } => Self::HueSaturation {
                hue,
                saturation,
                lightness,
            },
            Adjustment::Desaturate => Self::Desaturate,
            Adjustment::Invert => Self::Invert,
            Adjustment::ColorBalance(balance) => Self::ColorBalance(balance),
            Adjustment::Levels(levels) => Self::Lut(Box::new(ChannelLut::from_levels(levels))),
            Adjustment::Curves(curves) => Self::Lut(Box::new(ChannelLut::from_curves(curves))),
        }
    }

    pub(crate) fn map(&self, pixel: Rgba<u8>) -> Rgba<u8> {
        let [red, green, blue, alpha] = pixel.0;
        let [red, green, blue] = match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => [red, green, blue]
                .map(|channel| brightness_contrast(channel, *brightness, *contrast)),
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
            } => hue_saturation(red, green, blue, *hue, *saturation, *lightness),
            Self::Desaturate => {
                let gray =
                    ((u16::from(red) * 77 + u16::from(green) * 150 + u16::from(blue) * 29 + 128)
                        >> 8) as u8;
                [gray, gray, gray]
            }
            Self::Invert => [255 - red, 255 - green, 255 - blue],
            Self::ColorBalance(balance) => color_balance(red, green, blue, *balance),
            Self::Lut(lut) => lut.map(red, green, blue),
        };
        Rgba([red, green, blue, alpha])
    }
}

fn level_map(value: u8, stops: LevelStops) -> u8 {
    let black = stops.input_black;
    let white = stops.input_white.max(black.saturating_add(1));
    if stops.gamma == GAMMA_ONE {
        return level_map_linear(value, black, white, stops.output_black, stops.output_white);
    }
    let t = if value <= black {
        0.0
    } else if value >= white {
        1.0
    } else {
        f64::from(value - black) / f64::from(white - black)
    };
    let gamma = f64::from(stops.gamma.clamp(GAMMA_MIN, GAMMA_MAX)) / f64::from(GAMMA_ONE);
    let t = if t == 0.0 || t == 1.0 {
        t
    } else {
        t.powf(1.0 / gamma)
    };
    let out_black = f64::from(stops.output_black);
    let out_white = f64::from(stops.output_white);
    (out_black + t * (out_white - out_black))
        .round()
        .clamp(0.0, 255.0) as u8
}

fn level_map_linear(value: u8, black: u8, white: u8, out_black: u8, out_white: u8) -> u8 {
    if value <= black {
        return out_black;
    }
    if value >= white {
        return out_white;
    }
    let span = i32::from(white - black);
    let num = i32::from(value - black) * (i32::from(out_white) - i32::from(out_black));
    let half = span / 2;
    let rounded = if num >= 0 { num + half } else { num - half };
    (i32::from(out_black) + rounded / span).clamp(0, 255) as u8
}

fn interpolate(x0: u8, y0: u8, x1: u8, y1: u8, x: u8) -> u8 {
    if x <= x0 || x0 == x1 {
        return y0;
    }
    if x >= x1 {
        return y1;
    }
    let dx = i32::from(x1 - x0);
    let dy = i32::from(y1) - i32::from(y0);
    let num = i32::from(x - x0) * dy;
    let half = dx / 2;
    let rounded = if num >= 0 { num + half } else { num - half };
    (i32::from(y0) + rounded / dx).clamp(0, 255) as u8
}

fn color_balance(red: u8, green: u8, blue: u8, balance: ColorBalance) -> [u8; 3] {
    if balance.is_identity() {
        return [red, green, blue];
    }
    let channel = |value: u8| f64::from(value) / 255.0;
    let original = [channel(red), channel(green), channel(blue)];
    let lightness = (f64::from(red) * 77.0 + f64::from(green) * 150.0 + f64::from(blue) * 29.0)
        / (255.0 * 256.0);
    let amount = |value: i16| f64::from(value.clamp(-100, 100)) / 100.0;
    let mut adjusted = [
        transfer(
            original[0],
            lightness,
            [
                amount(balance.shadows[0]),
                amount(balance.midtones[0]),
                amount(balance.highlights[0]),
            ],
        ),
        transfer(
            original[1],
            lightness,
            [
                amount(balance.shadows[1]),
                amount(balance.midtones[1]),
                amount(balance.highlights[1]),
            ],
        ),
        transfer(
            original[2],
            lightness,
            [
                amount(balance.shadows[2]),
                amount(balance.midtones[2]),
                amount(balance.highlights[2]),
            ],
        ),
    ];
    if balance.preserve_luminosity {
        adjusted = preserve_luminosity(original, adjusted);
    }
    adjusted.map(|value| (value * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn transfer(value: f64, lightness: f64, amount: [f64; 3]) -> f64 {
    let a = 0.25;
    let b = 0.333;
    let scale = 0.7;
    let shadows = amount[0] * ((lightness - b) / -a + 0.5).clamp(0.0, 1.0) * scale;
    let midtones = amount[1]
        * ((lightness - b) / a + 0.5).clamp(0.0, 1.0)
        * ((lightness + b - 1.0) / -a + 0.5).clamp(0.0, 1.0)
        * scale;
    let highlights = amount[2] * ((lightness + b - 1.0) / a + 0.5).clamp(0.0, 1.0) * scale;
    (value + shadows + midtones + highlights).clamp(0.0, 1.0)
}

fn preserve_luminosity(original: [f64; 3], adjusted: [f64; 3]) -> [f64; 3] {
    let luma = |color: [f64; 3]| (color[0] * 77.0 + color[1] * 150.0 + color[2] * 29.0) / 256.0;
    let target = luma(original);
    let after = luma(adjusted);
    if after <= 1e-8 {
        return [target; 3];
    }
    let mut color = adjusted.map(|channel| channel * target / after);
    let high = color.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if high > 1.0 {
        // Compress around the target luminance instead of clipping individual channels.
        color = color.map(|channel| target + (channel - target) * (1.0 - target) / (high - target));
    }
    color
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
            Adjustment::ColorBalance(ColorBalance::default()),
            Adjustment::Levels(Levels::default()),
            Adjustment::Curves(Curves::default()),
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
        assert!(!preview.pending());
        let first = preview.request();
        let second = preview.request();
        assert!(preview.pending());
        assert!(!preview.finish(first));
        assert!(preview.pending());
        assert_eq!(preview.shown(), 0);
        assert!(preview.finish(second));
        assert!(!preview.pending());
        assert_eq!(preview.shown(), second);
        let reset = preview.request();
        assert!(preview.pending());
        assert!(!preview.finish(second));
        assert!(preview.pending());
        assert!(preview.finish(reset));
        assert!(!preview.pending());
        assert!(!preview.finish(first));
        assert_eq!(preview.shown(), reset);
    }

    #[test]
    fn levels_and_curves_use_one_lookup_and_match_fixed_points() {
        assert!(
            channel_lut(Adjustment::Levels(Levels::default()))
                .unwrap()
                .is_identity()
        );
        assert!(
            channel_lut(Adjustment::Curves(Curves::default()))
                .unwrap()
                .is_identity()
        );
        assert!(channel_lut(Adjustment::Invert).is_none());

        let levels = Levels::default()
            .set_stops(MapChannel::Value, LevelStops::IDENTITY.set_input_black(128));
        let lut = channel_lut(Adjustment::Levels(levels)).unwrap();
        assert_eq!(lut.red[0], 0);
        assert_eq!(lut.red[128], 0);
        assert_eq!(lut.red[192], 129);
        assert_eq!(lut.red[255], 255);
        assert_eq!(lut.green[192], lut.red[192]);

        let gamma =
            Levels::default().set_stops(MapChannel::Value, LevelStops::IDENTITY.set_gamma(200));
        let lut = channel_lut(Adjustment::Levels(gamma)).unwrap();
        assert_eq!(lut.blue[0], 0);
        assert_eq!(lut.blue[128], 181);
        assert_eq!(lut.blue[255], 255);

        let red_only = Levels::default().set_stops(
            MapChannel::Red,
            LevelStops::IDENTITY.set_output_black(0).set_output_white(0),
        );
        let lut = channel_lut(Adjustment::Levels(red_only)).unwrap();
        assert_eq!(lut.red[255], 0);
        assert_eq!(lut.green[255], 255);

        let curves = Curves::default().set_curve(
            MapChannel::Value,
            Curve::identity().try_add(128, 64).unwrap(),
        );
        let lut = channel_lut(Adjustment::Curves(curves)).unwrap();
        assert_eq!(lut.red[0], 0);
        assert_eq!(lut.red[64], 32);
        assert_eq!(lut.red[128], 64);
        assert_eq!(lut.red[255], 255);
        let inverted = Curve::identity().move_point(0, 9, 255).move_point(1, 0, 0);
        assert_eq!(inverted.map(0), 255);
        assert_eq!(inverted.map(128), 127);
        assert_eq!(inverted.map(255), 0);
        assert!(Curve::identity().try_add(40, 40).unwrap().is_identity());
        assert_eq!(LevelStops::IDENTITY.set_input_black(255).input_black, 254);
        assert_eq!(LevelStops::IDENTITY.set_input_white(0).input_white, 1);
        assert_eq!(LevelStops::IDENTITY.set_gamma(1).gamma, GAMMA_MIN);
        assert_eq!(LevelStops::IDENTITY.set_gamma(5000).gamma, GAMMA_MAX);
    }

    #[test]
    fn preserved_luminosity_survives_a_channel_clipping_at_white() {
        let before = Rgba([250, 250, 250, 99]);
        let balance = ColorBalance::default().set_shift(BalanceRange::Highlights, 0, -100);
        let after = adjust_pixel(before, Adjustment::ColorBalance(balance));
        let luminance = |p: Rgba<u8>| {
            (f64::from(p[0]) * 77.0 + f64::from(p[1]) * 150.0 + f64::from(p[2]) * 29.0) / 256.0
        };
        assert!((luminance(before) - luminance(after)).abs() <= 1.0);
        assert_eq!(after[3], before[3]);
    }

    #[test]
    fn color_balance_shifts_a_range_and_can_keep_luminosity() {
        let mut balance = ColorBalance::default().set_preserve_luminosity(false);
        balance.midtones[0] = 100;
        assert_eq!(
            adjust_pixel(
                Rgba([128, 128, 128, 255]),
                Adjustment::ColorBalance(balance)
            )
            .0,
            [255, 128, 128, 255]
        );
        balance.midtones[0] = 0;
        balance.shadows[0] = 100;
        assert_eq!(
            adjust_pixel(Rgba([0, 0, 0, 200]), Adjustment::ColorBalance(balance)).0,
            [179, 0, 0, 200]
        );
        balance.shadows[0] = 0;
        balance.highlights[0] = 100;
        assert_eq!(
            adjust_pixel(Rgba([0, 0, 0, 200]), Adjustment::ColorBalance(balance)).0,
            [0, 0, 0, 200]
        );
        let preserved = ColorBalance::default().set_shift(BalanceRange::Midtones, 0, 100);
        assert_eq!(
            adjust_pixel(
                Rgba([128, 128, 128, 255]),
                Adjustment::ColorBalance(preserved)
            )
            .0,
            [196, 99, 99, 255]
        );
    }

    #[test]
    fn curve_points_stay_inside_bounds_and_keep_x_in_order() {
        let curve = Curve::identity()
            .try_add(128, 200)
            .unwrap()
            .try_add(64, 20)
            .unwrap();
        assert_eq!(curve.points()[2], (128, 200));
        let moved = curve.move_point(2, 10, 180);
        assert_eq!(moved.points()[2].0, 65);
        let moved = curve.move_point(1, 250, 30);
        assert_eq!(moved.points()[1].0, 127);
        assert_eq!(curve.move_point(0, 100, 5).endpoints().0, (0, 5));
        assert_eq!(curve.move_point(3, 0, 9).endpoints().1, (255, 9));
        assert_eq!(curve.remove_point(0).point_count(), 4);
        assert_eq!(curve.remove_point(1).point_count(), 3);
        assert!(curve.try_add(64, 1).is_none());
        let mut full = Curve::identity();
        for x in 1..15 {
            full = full.try_add(x, 0).unwrap();
        }
        assert_eq!(full.point_count(), MAX_CURVE_POINTS);
        assert!(full.try_add(20, 0).is_none());
    }

    #[test]
    fn histograms_distinguish_empty_uniform_and_flat_images() {
        let clear = histogram(&RgbaImage::from_pixel(2, 2, Rgba([9, 9, 9, 0])), |_, _| 255);
        assert!(clear.is_empty());
        assert!(!clear.is_flat(MapChannel::Value));
        assert_eq!(clear.range(MapChannel::Red), None);
        assert_eq!(Curve::identity().endpoints(), ((0, 0), (255, 255)));
        assert_eq!(Curve::identity().map(0), 0);

        let red = histogram(
            &RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255])),
            |_, _| 255,
        );
        assert_eq!(red.range(MapChannel::Red), Some((255, 255)));
        assert_eq!(red.range(MapChannel::Green), Some((0, 0)));
        assert_eq!(red.range(MapChannel::Value), Some((77, 77)));
        assert!(!red.is_flat(MapChannel::Value));
        let endpoint = Curve::identity().move_point(0, 40, 10);
        assert_eq!(endpoint.endpoints().0, (0, 10));
        assert_eq!(endpoint.map(77), 84);

        let mut ramp = RgbaImage::new(256, 1);
        for x in 0..256 {
            ramp.put_pixel(x, 0, Rgba([x as u8, x as u8, x as u8, 255]));
        }
        let flat = histogram(&ramp, |_, _| 255);
        assert!(flat.is_flat(MapChannel::Value));
        assert!(flat.is_flat(MapChannel::Red));
        assert_eq!(flat.range(MapChannel::Value), Some((0, 255)));

        let mut mixed = RgbaImage::from_pixel(2, 1, Rgba([4, 4, 4, 0]));
        mixed.put_pixel(1, 0, Rgba([4, 5, 6, 255]));
        let counted = histogram(&mixed, |_, _| 255);
        assert_eq!(counted.range(MapChannel::Red), Some((4, 4)));
        assert_eq!(counted.bins(MapChannel::Red)[4], 1);
    }
}
