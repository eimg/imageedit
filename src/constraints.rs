use crate::preview::PixelRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AspectRatio {
    pub width: u32,
    pub height: u32,
}

impl AspectRatio {
    pub const SQUARE: Self = Self {
        width: 1,
        height: 1,
    };

    /// Largest centered box of this ratio contained in the existing geometry.
    /// Signed positions retain off-canvas selection geometry.
    pub fn inscribe(self, rect: PixelRect) -> PixelRect {
        let (width, height) = self.fit(true, rect.width, (rect.width, rect.height));
        PixelRect::new(
            rect.x.saturating_add(i64::from((rect.width - width) / 2)),
            rect.y.saturating_add(i64::from((rect.height - height) / 2)),
            width,
            height,
        )
    }

    pub fn fit(self, width_axis: bool, value: u32, limit: (u32, u32)) -> (u32, u32) {
        if limit.0 == 0 || limit.1 == 0 {
            return (0, 0);
        }
        let factor = (value.max(1) as f64
            / if width_axis { self.width } else { self.height }.max(1) as f64)
            .min(limit.0 as f64 / self.width.max(1) as f64)
            .min(limit.1 as f64 / self.height.max(1) as f64);
        (
            (self.width.max(1) as f64 * factor)
                .round()
                .clamp(1., limit.0.max(1) as f64) as u32,
            (self.height.max(1) as f64 * factor)
                .round()
                .clamp(1., limit.1.max(1) as f64) as u32,
        )
    }

    pub fn drag(self, start: (i64, i64), end: (i64, i64), canvas: Option<(u32, u32)>) -> PixelRect {
        if canvas.is_some_and(|size| size.0 == 0 || size.1 == 0) {
            return PixelRect::new(0, 0, 0, 0);
        }
        let left = end.0 < start.0;
        let top = end.1 < start.1;
        let width = end
            .0
            .saturating_sub(start.0)
            .unsigned_abs()
            .saturating_add(1)
            .min(u64::from(u32::MAX)) as u32;
        let height = end
            .1
            .saturating_sub(start.1)
            .unsigned_abs()
            .saturating_add(1)
            .min(u64::from(u32::MAX)) as u32;
        let limit = canvas.map_or(
            (crate::document::MAX_NEW_EDGE, crate::document::MAX_NEW_EDGE),
            |(w, h)| {
                (
                    if left {
                        start.0.saturating_add(1)
                    } else {
                        i64::from(w).saturating_sub(start.0)
                    }
                    .clamp(1, i64::from(w)) as u32,
                    if top {
                        start.1.saturating_add(1)
                    } else {
                        i64::from(h).saturating_sub(start.1)
                    }
                    .clamp(1, i64::from(h)) as u32,
                )
            },
        );
        let width_axis =
            width as f64 / self.width.max(1) as f64 >= height as f64 / self.height.max(1) as f64;
        let (width, height) = self.fit(width_axis, if width_axis { width } else { height }, limit);
        PixelRect::new(
            if left {
                start.0.saturating_sub(i64::from(width) - 1)
            } else {
                start.0
            },
            if top {
                start.1.saturating_sub(i64::from(height) - 1)
            } else {
                start.1
            },
            width,
            height,
        )
    }

    pub fn resize(
        self,
        rect: PixelRect,
        edges: (bool, bool, bool, bool),
        delta: (i64, i64),
        canvas: (u32, u32),
    ) -> PixelRect {
        if canvas.0 == 0 || canvas.1 == 0 {
            return PixelRect::new(0, 0, 0, 0);
        }
        let (left, right, top, bottom) = edges;
        let width = (i64::from(rect.width).saturating_add(if left {
            delta.0.saturating_neg()
        } else if right {
            delta.0
        } else {
            0
        }))
        .clamp(1, i64::from(u32::MAX)) as u32;
        let height = (i64::from(rect.height).saturating_add(if top {
            delta.1.saturating_neg()
        } else if bottom {
            delta.1
        } else {
            0
        }))
        .clamp(1, i64::from(u32::MAX)) as u32;
        let horizontal = left || right;
        let vertical = top || bottom;
        let width_axis = horizontal
            && (!vertical
                || (width as f64 / rect.width.max(1) as f64 - 1.).abs()
                    >= (height as f64 / rect.height.max(1) as f64 - 1.).abs());
        let cx = rect.x as f64 + rect.width as f64 / 2.;
        let cy = rect.y as f64 + rect.height as f64 / 2.;
        let max_w = if left {
            rect.right() as f64
        } else if right {
            canvas.0 as f64 - rect.x as f64
        } else {
            2. * cx.min(canvas.0 as f64 - cx)
        };
        let max_h = if top {
            rect.bottom() as f64
        } else if bottom {
            canvas.1 as f64 - rect.y as f64
        } else {
            2. * cy.min(canvas.1 as f64 - cy)
        };
        let (w, h) = self.fit(
            width_axis,
            if width_axis { width } else { height },
            (max_w.max(1.) as u32, max_h.max(1.) as u32),
        );
        PixelRect::new(
            if left {
                rect.right() - i64::from(w)
            } else if right {
                rect.x
            } else {
                (cx - w as f64 / 2.).round() as i64
            },
            if top {
                rect.bottom() - i64::from(h)
            } else if bottom {
                rect.y
            } else {
                (cy - h as f64 / 2.).round() as i64
            },
            w,
            h,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_ratio_inscribes_the_existing_box_without_moving_its_center() {
        assert_eq!(
            AspectRatio::SQUARE.inscribe(PixelRect::new(10, 20, 30, 10)),
            PixelRect::new(20, 20, 10, 10)
        );
        let ratio = AspectRatio {
            width: 4,
            height: 3,
        };
        let original = PixelRect::new(-8, 7, 24, 24);
        let fitted = ratio.inscribe(original);
        assert_eq!(fitted, PixelRect::new(-8, 10, 24, 18));
        assert_eq!(ratio.inscribe(fitted), fitted);
        assert_eq!(
            ratio.inscribe(PixelRect::new(2, 3, 0, 0)),
            PixelRect::new(2, 3, 0, 0)
        );
    }

    #[test]
    fn fixed_ratio_drags_anchor_the_start_and_stay_inside_crop_limits() {
        let ratio = AspectRatio {
            width: 2,
            height: 1,
        };
        assert_eq!(
            ratio.drag((10, 20), (29, 21), Some((100, 80))),
            PixelRect::new(10, 20, 20, 10)
        );
        let reversed = ratio.drag((10, 20), (-100, -100), Some((100, 80)));
        assert_eq!(reversed, PixelRect::new(0, 15, 11, 6));
        assert_eq!(reversed.right(), 11);
        assert_eq!(reversed.bottom(), 21);
        let outside = ratio.drag((-10, -10), (9, 0), None);
        assert_eq!(outside.x, -10);
        assert_eq!(outside.y, -10);
        assert_eq!((outside.width, outside.height), (22, 11));
        assert_eq!(ratio.fit(true, 19, (100, 100)), (19, 10));
        assert_eq!(ratio.fit(false, 1000, (18, 40)), (18, 9));
        assert_eq!(ratio.fit(true, 10, (0, 10)), (0, 0));
    }

    #[test]
    fn ratio_handle_resize_keeps_opposite_corners_and_centers_the_other_axis() {
        let ratio = AspectRatio {
            width: 2,
            height: 1,
        };
        let rect = PixelRect::new(10, 10, 20, 10);
        let corner = ratio.resize(rect, (true, false, true, false), (-10, -5), (100, 100));
        assert_eq!(corner, PixelRect::new(0, 5, 30, 15));
        assert_eq!(
            (corner.right(), corner.bottom()),
            (rect.right(), rect.bottom())
        );
        assert_eq!(
            ratio.resize(rect, (false, true, false, false), (10, 0), (100, 100)),
            PixelRect::new(10, 8, 30, 15)
        );
        for edges in [
            (true, false, false, false),
            (false, true, false, false),
            (false, false, true, false),
            (false, false, false, true),
            (true, false, true, false),
            (false, true, false, true),
        ] {
            for delta in [
                (i64::MIN, i64::MIN),
                (i64::MAX, i64::MAX),
                (-1000, 1000),
                (1000, -1000),
                (0, 0),
            ] {
                let resized = ratio.resize(rect, edges, delta, (100, 100));
                assert!(
                    resized.x >= 0
                        && resized.y >= 0
                        && resized.right() <= 100
                        && resized.bottom() <= 100
                );
                assert!(resized.width > 0 && resized.height > 0);
            }
        }
    }
}
