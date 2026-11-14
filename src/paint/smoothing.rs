use std::time::Duration;

pub const TICK: Duration = Duration::from_millis(8);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Smoothing {
    #[default]
    Off,
    Light,
    Strong,
}

impl Smoothing {
    fn response(self) -> Duration {
        Duration::from_millis(match self {
            Self::Off => 0,
            Self::Light => 16,
            Self::Strong => 32,
        })
    }

    fn settle(self) -> Duration {
        self.response() * 4
    }
}

/// A causal, time-based low pass with a finite settling deadline. Coordinates
/// are layer-local; timing is relative to the beginning of one stroke.
/// No input queue grows with pointer frequency or the duration of a gesture.
pub struct StrokeSmoother {
    mode: Smoothing,
    position: (f64, f64),
    target: (f64, f64),
    updated: Duration,
    input_at: Duration,
}

impl StrokeSmoother {
    pub fn new(mode: Smoothing, point: (f64, f64)) -> Option<Self> {
        if !point.0.is_finite() || !point.1.is_finite() {
            return None;
        }
        Some(Self {
            mode,
            position: point,
            target: point,
            updated: Duration::ZERO,
            input_at: Duration::ZERO,
        })
    }

    pub fn pending(&self) -> bool {
        self.position != self.target
    }

    pub fn input(&mut self, point: (f64, f64), at: Duration) -> Vec<(f64, f64)> {
        if !point.0.is_finite() || !point.1.is_finite() || at < self.updated {
            return Vec::new();
        }
        let mut points = self.advance(at);
        if self.target != point {
            self.target = point;
            self.input_at = at;
            if self.mode == Smoothing::Off {
                self.position = point;
                points.push(point);
            }
        }
        points
    }

    pub fn advance(&mut self, at: Duration) -> Vec<(f64, f64)> {
        let mut points = Vec::new();
        while self.updated < at && self.pending() {
            let next = self.updated.saturating_add(TICK).min(at);
            if self.mode == Smoothing::Off
                || next == Duration::MAX
                || next.saturating_sub(self.input_at) >= self.mode.settle()
            {
                self.position = self.target;
            } else {
                let remaining = (-(next - self.updated).as_secs_f64()
                    / self.mode.response().as_secs_f64())
                .exp();
                // A convex blend avoids overflowing a difference between large
                // finite coordinates and never overshoots either endpoint.
                self.position = (
                    self.position.0 * remaining + self.target.0 * (1. - remaining),
                    self.position.1 * remaining + self.target.1 * (1. - remaining),
                );
            }
            self.updated = next;
            points.push(self.position);
        }
        self.updated = self.updated.max(at);
        points
    }

    /// Drain the small remaining tail synchronously on release. This uses the
    /// same curve as the live timer, reaches the exact endpoint, and adds no
    /// asynchronous continuation to a committed stroke.
    pub fn finish(&mut self) -> Vec<(f64, f64)> {
        let mut points = self.advance(self.updated.saturating_add(self.mode.settle()));
        if self.pending() {
            self.position = self.target;
            points.push(self.position);
        }
        points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn elapsed_time_controls_response_and_timer_density_cannot_strengthen_it() {
        for (mode, response) in [(Smoothing::Light, 16.), (Smoothing::Strong, 32.)] {
            for interval in [1, 4, 8, 31] {
                let mut filter = StrokeSmoother::new(mode, (2., -4.)).unwrap();
                assert!(filter.input((18., 12.), Duration::ZERO).is_empty());
                for time in (interval..31).step_by(interval as usize) {
                    filter.advance(ms(time));
                }
                filter.advance(ms(31));
                let progress = 1. - (-31_f64 / response).exp();
                assert!((filter.position.0 - (2. + 16. * progress)).abs() < 1e-12);
                assert!((filter.position.1 - (-4. + 16. * progress)).abs() < 1e-12);
                assert!(filter.pending());
            }
        }
    }

    #[test]
    fn identical_input_does_not_postpone_the_exact_finite_settling_deadline() {
        for (mode, deadline) in [(Smoothing::Light, 64), (Smoothing::Strong, 128)] {
            let mut filter = StrokeSmoother::new(mode, (0., 0.)).unwrap();
            filter.input((30., -10.), ms(10));
            for time in (14..10 + deadline).step_by(4) {
                filter.input((30., -10.), ms(time));
            }
            filter.advance(ms(10 + deadline));
            assert_eq!(filter.position, (30., -10.));
            assert!(!filter.pending());
            assert!(filter.advance(ms(10_000)).is_empty());
            assert!(filter.finish().is_empty());
        }
    }

    #[test]
    fn short_strokes_and_release_tails_reach_the_target_without_waiting_or_overshooting() {
        let mut filter = StrokeSmoother::new(Smoothing::Strong, (1., 2.)).unwrap();
        filter.input((11., -8.), ms(1));
        let points = filter.finish();
        assert_eq!(points.len(), 16);
        assert_eq!(points.last(), Some(&(11., -8.)));
        assert!(
            points
                .iter()
                .all(|&(x, y)| (1. ..=11.).contains(&x) && (-8. ..=2.).contains(&y))
        );
        assert!(
            points
                .windows(2)
                .all(|pair| pair[0].0 <= pair[1].0 && pair[0].1 >= pair[1].1)
        );
        assert!(!filter.pending());
        // A late timer is bounded by the response window, not the length of the pause.
        filter.input((-20., 50.), ms(1_000_000));
        let points = filter.advance(Duration::MAX);
        assert!(points.len() <= 16);
        assert_eq!(points.last(), Some(&(-20., 50.)));
        filter.input((7., 8.), Duration::MAX);
        assert_eq!(filter.finish().last(), Some(&(7., 8.)));
        let mut saturated = StrokeSmoother::new(Smoothing::Strong, (0., 0.)).unwrap();
        saturated.input((1., 2.), Duration::MAX - ms(127));
        let points = saturated.finish();
        assert!(points.len() <= 16);
        assert_eq!(points.last(), Some(&(1., 2.)));
    }

    #[test]
    fn invalid_input_is_ignored_and_extreme_finite_coordinates_stay_finite() {
        assert!(StrokeSmoother::new(Smoothing::Light, (f64::NAN, 0.)).is_none());
        let mut filter = StrokeSmoother::new(Smoothing::Light, (-f64::MAX, f64::MAX)).unwrap();
        filter.input((f64::MAX, -f64::MAX), ms(1));
        let points = filter.advance(ms(33));
        assert!(points.iter().all(|&(x, y)| x.is_finite() && y.is_finite()));
        let previous = filter.position;
        for (point, at) in [
            ((f64::NAN, 0.), ms(40)),
            ((0., f64::INFINITY), ms(40)),
            ((0., 0.), ms(2)),
        ] {
            assert!(filter.input(point, at).is_empty());
            assert_eq!(filter.position, previous);
        }
        assert!(filter.advance(ms(2)).is_empty());
        assert_eq!(filter.finish().last(), Some(&(f64::MAX, -f64::MAX)));
    }

    #[test]
    fn smoothing_reduces_fast_alternating_jitter_and_off_follows_raw_points() {
        for mode in [Smoothing::Light, Smoothing::Strong] {
            let mut filter = StrokeSmoother::new(mode, (0., 0.)).unwrap();
            for time in 1..=100 {
                let y = if time % 2 == 0 { 10. } else { -10. };
                filter.input((time as f64, y), ms(time));
                assert!(filter.position.1.abs() < 1.);
            }
            assert_eq!(filter.finish().last(), Some(&(100., 10.)));
        }
        let mut raw = StrokeSmoother::new(Smoothing::Off, (1., 2.)).unwrap();
        assert_eq!(raw.input((5., 8.), ms(1)), vec![(5., 8.)]);
        assert_eq!(raw.input((2., 3.), ms(2)), vec![(2., 3.)]);
        assert!(!raw.pending());
        assert!(raw.finish().is_empty());
    }
}
