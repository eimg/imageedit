use image::Rgba;

pub const MAX_STOPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorStop {
    pub position: u16,
    pub color: Rgba<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct ColorRamp {
    stops: [ColorStop; MAX_STOPS],
    count: usize,
}

impl PartialEq for ColorRamp {
    fn eq(&self, other: &Self) -> bool {
        self.stops() == other.stops()
    }
}
impl Eq for ColorRamp {}

impl ColorRamp {
    pub fn new(start: Rgba<u8>, end: Rgba<u8>) -> Self {
        let mut stops = [ColorStop {
            position: 0,
            color: start,
        }; MAX_STOPS];
        stops[1] = ColorStop {
            position: 1000,
            color: end,
        };
        Self { stops, count: 2 }
    }

    pub fn stops(&self) -> &[ColorStop] {
        &self.stops[..self.count]
    }

    pub fn sample(&self, t: f64) -> Rgba<u8> {
        let position = if t.is_finite() {
            t.clamp(0., 1.) * 1000.
        } else {
            0.
        };
        let pair = self
            .stops()
            .windows(2)
            .find(|pair| position <= f64::from(pair[1].position))
            .unwrap();
        let t = (position - f64::from(pair[0].position))
            / f64::from(pair[1].position - pair[0].position);
        interpolate(pair[0].color, pair[1].color, t)
    }

    pub fn add_stop(&mut self) -> Option<usize> {
        if self.count == MAX_STOPS {
            return None;
        }
        let gap = self
            .stops()
            .windows(2)
            .enumerate()
            .max_by_key(|(index, pair)| {
                (
                    pair[1].position - pair[0].position,
                    std::cmp::Reverse(*index),
                )
            })?
            .0;
        let position = (self.stops[gap].position + self.stops[gap + 1].position) / 2;
        if position <= self.stops[gap].position {
            return None;
        }
        let color = self.sample(f64::from(position) / 1000.);
        let index = gap + 1;
        self.stops.copy_within(index..self.count, index + 1);
        self.stops[index] = ColorStop { position, color };
        self.count += 1;
        Some(index)
    }

    pub fn move_stop(&mut self, index: usize, position: u16) -> bool {
        if index == 0 || index >= self.count - 1 {
            return false;
        }
        let position = position.clamp(
            self.stops[index - 1].position + 1,
            self.stops[index + 1].position - 1,
        );
        if position == self.stops[index].position {
            return false;
        }
        self.stops[index].position = position;
        true
    }

    pub fn remove_stop(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.count - 1 {
            return false;
        }
        self.stops.copy_within(index + 1..self.count, index);
        self.count -= 1;
        true
    }

    pub fn set_color(&mut self, index: usize, color: Rgba<u8>) -> bool {
        let Some(stop) = self.stops[..self.count].get_mut(index) else {
            return false;
        };
        if stop.color == color {
            return false;
        }
        stop.color = color;
        true
    }

    pub fn map_colors(mut self, map: impl Fn(Rgba<u8>) -> Rgba<u8>) -> Self {
        for stop in &mut self.stops[..self.count] {
            stop.color = map(stop.color);
        }
        self
    }
}

pub fn interpolate(start: Rgba<u8>, end: Rgba<u8>, t: f64) -> Rgba<u8> {
    let t = t.clamp(0., 1.);
    let alpha = f64::from(start[3]) * (1. - t) + f64::from(end[3]) * t;
    let a = alpha.round().clamp(0., 255.) as u8;
    if a == 0 {
        return Rgba([0; 4]);
    }
    let channel = |index| {
        ((f64::from(start[index]) * f64::from(start[3]) * (1. - t)
            + f64::from(end[index]) * f64::from(end[3]) * t)
            / alpha)
            .round()
            .clamp(0., 255.) as u8
    };
    Rgba([channel(0), channel(1), channel(2), a])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_edits_keep_order_endpoints_and_a_bounded_capacity() {
        let original = ColorRamp::new(Rgba([255, 0, 0, 255]), Rgba([0, 0, 255, 255]));
        let mut ramp = original;
        let middle = ramp.add_stop().unwrap();
        assert_eq!(ramp.stops()[middle].position, 500);
        assert_eq!(ramp.stops()[middle].color, Rgba([128, 0, 128, 255]));
        assert!(ramp.move_stop(middle, 0));
        assert_eq!(ramp.stops()[middle].position, 1);
        assert!(ramp.move_stop(middle, 1000));
        assert_eq!(ramp.stops()[middle].position, 999);
        assert!(!ramp.move_stop(0, 50));
        assert!(!ramp.move_stop(usize::MAX, 50));
        assert!(!ramp.remove_stop(0));
        assert!(!ramp.remove_stop(ramp.stops().len() - 1));
        assert!(!ramp.remove_stop(usize::MAX));
        assert!(ramp.remove_stop(middle));
        assert_eq!(ramp, original);
        while ramp.add_stop().is_some() {}
        assert_eq!(ramp.stops().len(), MAX_STOPS);
        assert!(
            ramp.stops()
                .windows(2)
                .all(|pair| pair[0].position < pair[1].position)
        );
        assert_eq!(ramp.stops()[0].position, 0);
        assert_eq!(ramp.stops().last().unwrap().position, 1000);
    }

    #[test]
    fn transparent_stops_do_not_tint_visible_colors_or_darken_faint_alpha() {
        let ramp = ColorRamp::new(Rgba([255, 0, 0, 255]), Rgba([0, 0, 255, 0]));
        assert_eq!(ramp.sample(0.5), Rgba([255, 0, 0, 128]));
        assert_eq!(ramp.sample(1.), Rgba([0; 4]));
        assert_eq!(ramp.sample(-2.), Rgba([255, 0, 0, 255]));
        assert_eq!(ramp.sample(f64::NAN), ramp.sample(0.));
        let faint = ColorRamp::new(Rgba([20, 120, 240, 1]), Rgba([255, 0, 0, 0]));
        assert_eq!(faint.sample(0.25), Rgba([20, 120, 240, 1]));
        assert_eq!(faint.sample(0.75), Rgba([0; 4]));
    }

    #[test]
    fn piecewise_color_and_alpha_match_independent_integer_interpolation() {
        let mut ramp = ColorRamp::new(Rgba([240, 20, 60, 40]), Rgba([10, 80, 220, 220]));
        let index = ramp.add_stop().unwrap();
        ramp.move_stop(index, 370);
        ramp.set_color(index, Rgba([80, 210, 30, 130]));
        for position in 0..=1000u16 {
            let pair = ramp
                .stops()
                .windows(2)
                .find(|pair| position <= pair[1].position)
                .unwrap();
            let left = u64::from(pair[1].position - position);
            let right = u64::from(position - pair[0].position);
            let span = left + right;
            let alpha = u64::from(pair[0].color[3]) * left + u64::from(pair[1].color[3]) * right;
            let mut expected = [0u8; 4];
            expected[3] = ((alpha + span / 2) / span) as u8;
            for (channel, value) in expected[..3].iter_mut().enumerate() {
                let color = u64::from(pair[0].color[channel]) * u64::from(pair[0].color[3]) * left
                    + u64::from(pair[1].color[channel]) * u64::from(pair[1].color[3]) * right;
                *value = ((color + alpha / 2) / alpha) as u8;
            }
            let actual = ramp.sample(f64::from(position) / 1000.);
            for channel in 0..4 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 1,
                    "position {position}: {actual:?}, {expected:?}"
                );
            }
        }
    }

    #[test]
    fn color_mapping_preserves_positions_opacity_and_the_original_palette() {
        let mut ramp = ColorRamp::new(Rgba([240, 0, 0, 255]), Rgba([0, 0, 240, 0]));
        let middle = ramp.add_stop().unwrap();
        ramp.set_color(middle, Rgba([0, 180, 0, 100]));
        let mapped =
            ramp.map_colors(|color| crate::transform::ColorMode::Grayscale.constrain(color));
        for (old, new) in ramp.stops().iter().zip(mapped.stops()) {
            assert_eq!(old.position, new.position);
            assert_eq!(old.color[3], new.color[3]);
            assert_eq!(new.color[0], new.color[1]);
            assert_eq!(new.color[1], new.color[2]);
        }
        assert_ne!(ramp.stops()[middle].color, mapped.stops()[middle].color);
    }
}
