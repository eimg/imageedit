use crate::selection::Selection;
use crate::transform::{Mirror, Turn};

pub const MAX_PATHS: usize = 32;
pub const MAX_ANCHORS: usize = 64;
const FLATNESS: f64 = 0.25;
const MAX_DEPTH: u32 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PathId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnchorId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub id: AnchorId,
    pub point: (f64, f64),
    pub handle_in: (f64, f64),
    pub handle_out: (f64, f64),
}

impl Anchor {
    pub fn corner(id: AnchorId, point: (f64, f64)) -> Self {
        Self {
            id,
            point,
            handle_in: point,
            handle_out: point,
        }
    }

    pub fn finite(self) -> bool {
        finite(self.point) && finite(self.handle_in) && finite(self.handle_out)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorPath {
    pub id: PathId,
    pub visible: bool,
    pub closed: bool,
    pub anchors: Vec<Anchor>,
}

impl VectorPath {
    pub fn anchor(&self, id: AnchorId) -> Option<&Anchor> {
        self.anchors.iter().find(|anchor| anchor.id == id)
    }

    pub fn anchor_mut(&mut self, id: AnchorId) -> Option<&mut Anchor> {
        self.anchors.iter_mut().find(|anchor| anchor.id == id)
    }
}

pub fn finite(point: (f64, f64)) -> bool {
    point.0.is_finite() && point.1.is_finite()
}

pub fn stroke(path: &VectorPath) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let segments = path.anchors.len().saturating_sub(1);
    let segments = if path.closed {
        path.anchors.len()
    } else {
        segments
    };
    if path.anchors.is_empty() || segments == 0 {
        return path
            .anchors
            .first()
            .map(|anchor| vec![anchor.point])
            .unwrap_or_default();
    }
    points.push(path.anchors[0].point);
    for index in 0..segments {
        let start = &path.anchors[index];
        let end = &path.anchors[(index + 1) % path.anchors.len()];
        flatten_cubic(
            start.point,
            start.handle_out,
            end.handle_in,
            end.point,
            &mut points,
        );
    }
    points
}

pub fn contour(path: &VectorPath) -> Option<Vec<(f64, f64)>> {
    (path.closed && path.anchors.len() >= 3).then(|| stroke(path))
}

// Closed paths fill together with nonzero winding. Open paths do not fill.
pub fn selection(paths: &[VectorPath], width: u32, height: u32) -> Selection {
    let contours: Vec<Vec<(f64, f64)>> = paths
        .iter()
        .filter(|path| path.visible)
        .filter_map(contour)
        .collect();
    if contours.is_empty() {
        return Selection::Empty;
    }
    let rings: Vec<&[(f64, f64)]> = contours.iter().map(Vec::as_slice).collect();
    Selection::polygons(&rings, width, height)
}

pub fn turn_point(point: (f64, f64), canvas: (u32, u32), turn: Turn) -> (f64, f64) {
    let (x, y) = point;
    let (width, height) = (f64::from(canvas.0), f64::from(canvas.1));
    match turn {
        Turn::Right => (height - y, x),
        Turn::Left => (y, width - x),
        Turn::Half => (width - x, height - y),
    }
}

pub fn mirror_point(point: (f64, f64), canvas: (u32, u32), mirror: Mirror) -> (f64, f64) {
    let (x, y) = point;
    let (width, height) = (f64::from(canvas.0), f64::from(canvas.1));
    match mirror {
        Mirror::Horizontal => (width - x, y),
        Mirror::Vertical => (x, height - y),
    }
}

pub fn scale_point(point: (f64, f64), old: (u32, u32), new: (u32, u32)) -> (f64, f64) {
    (
        point.0 * f64::from(new.0) / f64::from(old.0),
        point.1 * f64::from(new.1) / f64::from(old.1),
    )
}

pub fn translate_point(point: (f64, f64), delta: (f64, f64)) -> (f64, f64) {
    (point.0 + delta.0, point.1 + delta.1)
}

fn flatten_cubic(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    points: &mut Vec<(f64, f64)>,
) {
    flatten_cubic_depth(p0, p1, p2, p3, points, MAX_DEPTH);
}

fn flatten_cubic_depth(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    points: &mut Vec<(f64, f64)>,
    depth: u32,
) {
    if depth == 0 || curve_flat(p0, p1, p2, p3) {
        points.push(p3);
        return;
    }
    let (left, right) = split_cubic(p0, p1, p2, p3);
    flatten_cubic_depth(left[0], left[1], left[2], left[3], points, depth - 1);
    flatten_cubic_depth(right[0], right[1], right[2], right[3], points, depth - 1);
}

fn curve_flat(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) -> bool {
    distance_to_chord(p1, p0, p3) <= FLATNESS && distance_to_chord(p2, p0, p3) <= FLATNESS
}

fn distance_to_chord(point: (f64, f64), start: (f64, f64), end: (f64, f64)) -> f64 {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let length = dx * dx + dy * dy;
    if length < 1e-12 {
        return (point.0 - start.0).hypot(point.1 - start.1);
    }
    let t = (((point.0 - start.0) * dx + (point.1 - start.1) * dy) / length).clamp(0.0, 1.0);
    (point.0 - (start.0 + t * dx)).hypot(point.1 - (start.1 + t * dy))
}

type Cubic = [(f64, f64); 4];

fn split_cubic(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) -> (Cubic, Cubic) {
    let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    let p01 = mid(p0, p1);
    let p12 = mid(p1, p2);
    let p23 = mid(p2, p3);
    let p012 = mid(p01, p12);
    let p123 = mid(p12, p23);
    let p0123 = mid(p012, p123);
    ([p0, p01, p012, p0123], [p0123, p123, p23, p3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(id: u64, origin: (f64, f64), size: f64, reverse: bool) -> VectorPath {
        let (x, y) = origin;
        let mut anchors = vec![
            Anchor::corner(AnchorId(id * 10 + 1), (x, y)),
            Anchor::corner(AnchorId(id * 10 + 2), (x + size, y)),
            Anchor::corner(AnchorId(id * 10 + 3), (x + size, y + size)),
            Anchor::corner(AnchorId(id * 10 + 4), (x, y + size)),
        ];
        if reverse {
            anchors.reverse();
        }
        VectorPath {
            id: PathId(id),
            visible: true,
            closed: true,
            anchors,
        }
    }

    #[test]
    fn a_corner_square_matches_the_polygon_and_a_curve_reaches_past_its_chord() {
        let path = square(1, (1.0, 4.0), 5.0, false);
        let flat = contour(&path).unwrap();
        let filled = selection(std::slice::from_ref(&path), 10, 10);
        assert_eq!(filled, Selection::polygon(&flat, 10, 10));
        assert!(filled.covers(3, 5, 10, 10));
        assert!(!selection(&[square(1, (1.0, 4.0), 5.0, false)], 10, 10).covers(3, 2, 10, 10));

        let mut curved = square(1, (1.0, 4.0), 5.0, false);
        curved.anchors[0].handle_out = (2.0, 1.0);
        curved.anchors[1].handle_in = (5.0, 1.0);
        assert!(selection(&[curved], 10, 10).covers(3, 2, 10, 10));
    }

    #[test]
    fn opposite_contours_cancel_their_overlap_and_an_open_path_does_not_fill() {
        let left = square(1, (1.0, 1.0), 4.0, false);
        let right = square(2, (3.0, 3.0), 4.0, false);
        let same = selection(&[left.clone(), right.clone()], 10, 10);
        assert!(same.covers(2, 2, 10, 10));
        assert!(same.covers(4, 4, 10, 10));
        assert!(same.covers(6, 6, 10, 10));
        let opposite = selection(&[left, square(2, (3.0, 3.0), 4.0, true)], 10, 10);
        assert!(opposite.covers(2, 2, 10, 10));
        assert!(!opposite.covers(4, 4, 10, 10));
        assert!(opposite.covers(6, 6, 10, 10));

        let mut open = square(3, (1.0, 1.0), 4.0, false);
        open.closed = false;
        assert_eq!(selection(&[open], 10, 10), Selection::Empty);
    }

    #[test]
    fn hidden_paths_do_not_contribute_to_a_selection() {
        let visible = square(1, (1.0, 1.0), 4.0, false);
        let mut hidden = square(2, (5.0, 5.0), 3.0, false);
        hidden.visible = false;
        let selected = selection(&[visible.clone(), hidden], 10, 10);
        assert_eq!(selected, selection(&[visible], 10, 10));
    }

    #[test]
    fn a_left_turn_moves_the_anchor_and_both_handles() {
        let point = turn_point((1.0, 2.0), (4, 6), Turn::Left);
        assert_eq!(point, (2.0, 3.0));
        assert_eq!(
            mirror_point((1.0, 2.0), (4, 6), Mirror::Horizontal),
            (3.0, 2.0)
        );
        assert_eq!(scale_point((2.0, 4.0), (4, 8), (8, 16)), (4.0, 8.0));
    }
}
