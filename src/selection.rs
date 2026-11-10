use crate::preview::PixelRect;

// A canvas-space selection. Unrestricted, empty, and full stay allocation-free.
// A rectangle and its inverse do too. A future dense mask has to stay within the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Selection {
    // No mask: every layer pixel can be edited, including pixels that sit off the canvas.
    #[default]
    Unrestricted,
    // A mask that selects nothing. Writes do not change pixels.
    Empty,
    // Every pixel of the canvas, and none of the off-canvas layer pixels.
    Full,
    Rect(PixelRect),
    // The canvas except this rectangle.
    Inverse(PixelRect),
}

impl Selection {
    pub fn rectangle(start: (u32, u32), end: (u32, u32), width: u32, height: u32) -> Self {
        let left = i64::from(start.0.min(end.0));
        let top = i64::from(start.1.min(end.1));
        let right = i64::from(start.0.max(end.0)) + 1;
        let bottom = i64::from(start.1.max(end.1)) + 1;
        Self::from_rect(
            PixelRect::new(left, top, (right - left) as u32, (bottom - top) as u32),
            width,
            height,
        )
    }

    pub fn invert(self, width: u32, height: u32) -> Self {
        match self {
            Self::Unrestricted | Self::Full => Self::Empty,
            Self::Empty => Self::Full,
            Self::Rect(rect) => {
                if covers_canvas(rect, width, height) {
                    Self::Empty
                } else {
                    Self::Inverse(rect)
                }
            }
            Self::Inverse(hole) => Self::from_rect(hole, width, height),
        }
    }

    pub fn covers(self, x: i64, y: i64, width: u32, height: u32) -> bool {
        let inside = x >= 0 && y >= 0 && x < i64::from(width) && y < i64::from(height);
        match self {
            Self::Unrestricted => true,
            Self::Empty => false,
            Self::Full => inside,
            Self::Rect(rect) => inside && contains(rect, x, y),
            Self::Inverse(hole) => inside && !contains(hole, x, y),
        }
    }

    // The crop command uses this only when the selected pixels have a smaller bounding box
    // than the canvas. Unrestricted and empty selections have no crop.
    pub fn crop_bounds(self, width: u32, height: u32) -> Option<PixelRect> {
        let bounds = self.bounds(width, height)?;
        (bounds.width < width || bounds.height < height).then_some(bounds)
    }

    pub fn rebase(self, origin_x: i64, origin_y: i64, width: u32, height: u32) -> Self {
        match self {
            Self::Unrestricted | Self::Empty => self,
            Self::Full => Self::Full,
            Self::Rect(rect) => {
                Self::from_rect(translate(rect, -origin_x, -origin_y), width, height)
            }
            Self::Inverse(hole) => {
                let hole = translate(hole, -origin_x, -origin_y);
                match clip(hole, width, height) {
                    Some(hole) if covers_canvas(hole, width, height) => Self::Empty,
                    Some(hole) => Self::Inverse(hole),
                    None => Self::Full,
                }
            }
        }
    }

    pub fn contours(self, width: u32, height: u32) -> Vec<PixelRect> {
        match self {
            Self::Unrestricted | Self::Empty => Vec::new(),
            Self::Full => vec![PixelRect::new(0, 0, width, height)],
            Self::Rect(rect) => vec![rect],
            Self::Inverse(hole) => vec![PixelRect::new(0, 0, width, height), hole],
        }
    }

    fn from_rect(rect: PixelRect, width: u32, height: u32) -> Self {
        match clip(rect, width, height) {
            Some(rect) if covers_canvas(rect, width, height) => Self::Full,
            Some(rect) => Self::Rect(rect),
            None => Self::Empty,
        }
    }

    fn bounds(self, width: u32, height: u32) -> Option<PixelRect> {
        match self {
            Self::Unrestricted | Self::Empty => None,
            Self::Full => Some(PixelRect::new(0, 0, width, height)),
            Self::Rect(rect) => clip(rect, width, height),
            Self::Inverse(hole) => inverse_bounds(hole, width, height),
        }
    }
}

fn translate(rect: PixelRect, dx: i64, dy: i64) -> PixelRect {
    PixelRect::new(rect.x + dx, rect.y + dy, rect.width, rect.height)
}

fn contains(rect: PixelRect, x: i64, y: i64) -> bool {
    x >= rect.x && y >= rect.y && x < rect.right() && y < rect.bottom()
}

fn covers_canvas(rect: PixelRect, width: u32, height: u32) -> bool {
    rect.x <= 0
        && rect.y <= 0
        && rect.right() >= i64::from(width)
        && rect.bottom() >= i64::from(height)
}

fn clip(rect: PixelRect, width: u32, height: u32) -> Option<PixelRect> {
    let right = rect.right().clamp(0, i64::from(width));
    let bottom = rect.bottom().clamp(0, i64::from(height));
    let x = rect.x.clamp(0, i64::from(width));
    let y = rect.y.clamp(0, i64::from(height));
    (x < right && y < bottom).then_some(PixelRect::new(
        x,
        y,
        (right - x) as u32,
        (bottom - y) as u32,
    ))
}

// The complement of a rectangle is the whole canvas unless that rectangle is a full-width
// or full-height band. A band leaves the rows or columns outside it.
fn inverse_bounds(hole: PixelRect, width: u32, height: u32) -> Option<PixelRect> {
    let Some(hole) = clip(hole, width, height) else {
        return Some(PixelRect::new(0, 0, width, height));
    };
    if covers_canvas(hole, width, height) {
        return None;
    }
    let spans_width = hole.x == 0 && hole.right() >= i64::from(width);
    let spans_height = hole.y == 0 && hole.bottom() >= i64::from(height);
    // A band in the middle leaves selected pixels on both sides, so the box is the whole canvas.
    let (x, right) = if spans_height && hole.x == 0 {
        (hole.right(), i64::from(width))
    } else if spans_height && hole.right() >= i64::from(width) {
        (0, hole.x)
    } else {
        (0, i64::from(width))
    };
    let (y, bottom) = if spans_width && hole.y == 0 {
        (hole.bottom(), i64::from(height))
    } else if spans_width && hole.bottom() >= i64::from(height) {
        (0, hole.y)
    } else {
        (0, i64::from(height))
    };
    (x < right && y < bottom).then_some(PixelRect::new(
        x,
        y,
        (right - x) as u32,
        (bottom - y) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_clips_to_the_canvas_and_collapses_when_it_covers_it() {
        assert_eq!(
            Selection::rectangle((1, 2), (4, 6), 10, 10),
            Selection::Rect(PixelRect::new(1, 2, 4, 5))
        );
        assert_eq!(Selection::rectangle((0, 0), (9, 9), 10, 8), Selection::Full);
    }

    #[test]
    fn invert_uses_the_empty_and_full_cases() {
        assert_eq!(Selection::Unrestricted.invert(4, 4), Selection::Empty);
        assert_eq!(Selection::Empty.invert(4, 4), Selection::Full);
        assert_eq!(Selection::Full.invert(4, 4), Selection::Empty);
        let rect = Selection::Rect(PixelRect::new(1, 1, 2, 2));
        assert_eq!(
            rect.invert(8, 8),
            Selection::Inverse(PixelRect::new(1, 1, 2, 2))
        );
        assert_eq!(rect.invert(8, 8).invert(8, 8), rect);
    }

    #[test]
    fn coverage_stops_at_the_rectangle_and_allows_every_pixel_when_unrestricted() {
        let rect = Selection::Rect(PixelRect::new(2, 1, 2, 1));
        assert!(rect.covers(2, 1, 6, 4));
        assert!(!rect.covers(4, 1, 6, 4));
        assert!(!rect.covers(-1, 0, 6, 4));
        assert!(Selection::Unrestricted.covers(-4, 9, 6, 4));
        assert!(!Selection::Full.covers(-1, 0, 6, 4));
        assert!(Selection::Full.covers(0, 0, 6, 4));
        assert!(!Selection::Empty.covers(0, 0, 6, 4));
        let inverse = Selection::Inverse(PixelRect::new(2, 1, 2, 1));
        assert!(!inverse.covers(2, 1, 6, 4));
        assert!(inverse.covers(0, 0, 6, 4));
    }

    #[test]
    fn crop_bounds_reject_unrestricted_empty_and_the_whole_canvas() {
        assert_eq!(Selection::Unrestricted.crop_bounds(8, 8), None);
        assert_eq!(Selection::Empty.crop_bounds(8, 8), None);
        assert_eq!(Selection::Full.crop_bounds(8, 8), None);
        assert_eq!(
            Selection::Rect(PixelRect::new(1, 2, 3, 4)).crop_bounds(8, 8),
            Some(PixelRect::new(1, 2, 3, 4))
        );
        assert_eq!(
            Selection::Inverse(PixelRect::new(0, 0, 8, 3)).crop_bounds(8, 8),
            Some(PixelRect::new(0, 3, 8, 5))
        );
        assert_eq!(
            Selection::Inverse(PixelRect::new(2, 0, 2, 8)).crop_bounds(8, 8),
            None
        );
    }

    #[test]
    fn crop_rebases_a_rectangle_and_drops_it_when_the_crop_misses_it() {
        let rect = Selection::Rect(PixelRect::new(4, 2, 3, 2));
        assert_eq!(rect.rebase(4, 2, 3, 2), Selection::Full);
        assert_eq!(
            rect.rebase(4, 1, 3, 2),
            Selection::Rect(PixelRect::new(0, 1, 3, 1))
        );
        assert_eq!(rect.rebase(0, 0, 4, 2), Selection::Empty);
        assert_eq!(
            Selection::Inverse(PixelRect::new(1, 1, 2, 2)).rebase(4, 0, 4, 4),
            Selection::Full
        );
    }
}
