use crate::preview::PixelRect;
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

const KAPPA: f64 = 0.552_284_749_830_793_6;

pub fn outlines_from_selection(selection: &Selection, width: u32, height: u32) -> Vec<Vec<Anchor>> {
    let one = |anchors: Vec<Anchor>| {
        if anchors.len() >= 3 {
            vec![anchors]
        } else {
            Vec::new()
        }
    };
    match selection {
        Selection::Unrestricted | Selection::Empty => Vec::new(),
        Selection::Full => one(rectangle_anchors(PixelRect::new(0, 0, width, height))),
        Selection::Rect(rect) => one(rectangle_anchors(*rect)),
        Selection::Ellipse(ellipse) => one(ellipse_anchors(ellipse.rect())),
        Selection::RoundedRect(shape) => one(rounded_rect_anchors(shape.rect(), shape.radius())),
        Selection::Inverse(hole) => inverse_anchors(*hole, width, height),
        Selection::Mask(_) => polyline_outlines(selection, width, height),
    }
}

fn rectangle_anchors(rect: PixelRect) -> Vec<Anchor> {
    if rect.width == 0 || rect.height == 0 {
        return Vec::new();
    }
    let (left, top) = (rect.x as f64, rect.y as f64);
    let (right, bottom) = (rect.right() as f64, rect.bottom() as f64);
    vec![
        Anchor::corner(AnchorId(0), (left, top)),
        Anchor::corner(AnchorId(0), (right, top)),
        Anchor::corner(AnchorId(0), (right, bottom)),
        Anchor::corner(AnchorId(0), (left, bottom)),
    ]
}

fn ellipse_anchors(rect: PixelRect) -> Vec<Anchor> {
    if rect.width == 0 || rect.height == 0 {
        return Vec::new();
    }
    let (left, top) = (rect.x as f64, rect.y as f64);
    let (right, bottom) = (rect.right() as f64, rect.bottom() as f64);
    let (cx, cy) = ((left + right) * 0.5, (top + bottom) * 0.5);
    let (kx, ky) = ((right - left) * 0.5 * KAPPA, (bottom - top) * 0.5 * KAPPA);
    vec![
        curved_anchor((cx, top), (cx - kx, top), (cx + kx, top)),
        curved_anchor((right, cy), (right, cy - ky), (right, cy + ky)),
        curved_anchor((cx, bottom), (cx + kx, bottom), (cx - kx, bottom)),
        curved_anchor((left, cy), (left, cy + ky), (left, cy - ky)),
    ]
}

fn rounded_rect_anchors(rect: PixelRect, radius: u32) -> Vec<Anchor> {
    if rect.width == 0 || rect.height == 0 {
        return Vec::new();
    }
    let radius = radius.min(rect.width.min(rect.height) / 2);
    if radius == 0 {
        return rectangle_anchors(rect);
    }
    let (left, top) = (rect.x as f64, rect.y as f64);
    let (right, bottom) = (rect.right() as f64, rect.bottom() as f64);
    let r = f64::from(radius);
    let k = r * KAPPA;
    vec![
        curved_anchor((left + r, top), (left + r - k, top), (left + r, top)),
        curved_anchor((right - r, top), (right - r, top), (right - r + k, top)),
        curved_anchor((right, top + r), (right, top + r - k), (right, top + r)),
        curved_anchor(
            (right, bottom - r),
            (right, bottom - r),
            (right, bottom - r + k),
        ),
        curved_anchor(
            (right - r, bottom),
            (right - r + k, bottom),
            (right - r, bottom),
        ),
        curved_anchor(
            (left + r, bottom),
            (left + r, bottom),
            (left + r - k, bottom),
        ),
        curved_anchor(
            (left, bottom - r),
            (left, bottom - r + k),
            (left, bottom - r),
        ),
        curved_anchor((left, top + r), (left, top + r), (left, top + r - k)),
    ]
}

fn inverse_anchors(hole: PixelRect, width: u32, height: u32) -> Vec<Vec<Anchor>> {
    let right = i64::from(width);
    let bottom = i64::from(height);
    if hole.x > 0 && hole.y > 0 && hole.right() < right && hole.bottom() < bottom {
        let outer = rectangle_anchors(PixelRect::new(0, 0, width, height));
        let mut inner = rectangle_anchors(hole);
        inner.reverse();
        if outer.len() >= 3 && inner.len() >= 3 {
            return vec![outer, inner];
        }
    }
    polyline_outlines(&Selection::Inverse(hole), width, height)
}

fn polyline_outlines(selection: &Selection, width: u32, height: u32) -> Vec<Vec<Anchor>> {
    selection
        .stroke_polylines(width, height)
        .into_iter()
        .filter_map(|points| polyline_anchors(&points))
        .take(MAX_PATHS)
        .collect()
}

fn curved_anchor(point: (f64, f64), handle_in: (f64, f64), handle_out: (f64, f64)) -> Anchor {
    Anchor {
        id: AnchorId(0),
        point,
        handle_in,
        handle_out,
    }
}

fn polyline_anchors(points: &[(f64, f64)]) -> Option<Vec<Anchor>> {
    let mut points = points.to_vec();
    if points
        .first()
        .is_some_and(|first| points.last().is_some_and(|last| near(*first, *last)))
    {
        points.pop();
    }
    points.dedup_by(|next, previous| near(*next, *previous));
    points = remove_collinear(points);
    if points.len() < 3 {
        return None;
    }
    let points = simplify_closed(&points);
    Some(
        points
            .into_iter()
            .map(|point| Anchor::corner(AnchorId(0), point))
            .collect(),
    )
}

fn remove_collinear(mut points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    loop {
        if points.len() <= 3 {
            return points;
        }
        let mut kept = Vec::with_capacity(points.len());
        let mut removed = false;
        for index in 0..points.len() {
            let previous = points[(index + points.len() - 1) % points.len()];
            let current = points[index];
            let next = points[(index + 1) % points.len()];
            let (ax, ay) = (current.0 - previous.0, current.1 - previous.1);
            let (bx, by) = (next.0 - current.0, next.1 - current.1);
            let cross = ax * by - ay * bx;
            let forward = ax * bx + ay * by >= 0.0;
            if cross.abs() <= 1e-9 && forward {
                removed = true;
            } else {
                kept.push(current);
            }
        }
        if !removed || kept.len() < 3 {
            return points;
        }
        points = kept;
    }
}

fn simplify_closed(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if points.len() <= MAX_ANCHORS {
        return points.to_vec();
    }
    let mut tolerance = 0.5;
    for _ in 0..20 {
        let simplified = simplify_closed_at(points, tolerance);
        if simplified.len() >= 3 && simplified.len() <= MAX_ANCHORS {
            return simplified;
        }
        tolerance *= 2.0;
    }
    let step = points.len() as f64 / MAX_ANCHORS as f64;
    (0..MAX_ANCHORS)
        .map(|index| points[(index as f64 * step) as usize])
        .collect()
}

fn simplify_closed_at(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    if points.len() <= 3 {
        return points.to_vec();
    }
    let anchor = points[0];
    let far = points
        .iter()
        .enumerate()
        .skip(1)
        .max_by(|(_, left), (_, right)| {
            distance(anchor, **left)
                .partial_cmp(&distance(anchor, **right))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or(0, |(index, _)| index);
    if far == 0 {
        return points.to_vec();
    }
    let mut left = simplify_open(&points[..=far], tolerance);
    let mut closing = points[far..].to_vec();
    closing.push(points[0]);
    let right = simplify_open(&closing, tolerance);
    left.pop();
    left.extend(right.into_iter().skip(1));
    left.pop();
    left
}

fn simplify_open(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut pending = vec![(0, points.len() - 1)];
    while let Some((first, last)) = pending.pop() {
        let start = points[first];
        let end = points[last];
        let mut split = 0;
        let mut farthest = 0.0;
        for (index, point) in points.iter().enumerate().take(last).skip(first + 1) {
            let span = distance_to_chord(*point, start, end);
            if span > farthest {
                farthest = span;
                split = index;
            }
        }
        if farthest > tolerance && split > first && split < last {
            keep[split] = true;
            pending.push((first, split));
            pending.push((split, last));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(*point))
        .collect()
}

fn near(left: (f64, f64), right: (f64, f64)) -> bool {
    (left.0 - right.0).abs() <= 1e-9 && (left.1 - right.1).abs() <= 1e-9
}

fn distance(left: (f64, f64), right: (f64, f64)) -> f64 {
    (left.0 - right.0).hypot(left.1 - right.1)
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

    fn paths_from_outlines(outlines: Vec<Vec<Anchor>>) -> Vec<VectorPath> {
        let mut anchor = 1;
        outlines
            .into_iter()
            .enumerate()
            .map(|(index, mut anchors)| {
                for item in &mut anchors {
                    item.id = AnchorId(anchor);
                    anchor += 1;
                }
                VectorPath {
                    id: PathId(index as u64 + 1),
                    visible: true,
                    closed: true,
                    anchors,
                }
            })
            .collect()
    }

    fn assert_same_coverage(actual: &Selection, expected: &Selection, width: u32, height: u32) {
        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    actual.covers(i64::from(x), i64::from(y), width, height),
                    expected.covers(i64::from(x), i64::from(y), width, height),
                    "coverage differs at {x}, {y}"
                );
            }
        }
    }

    #[test]
    fn selection_outlines_round_trip_rectangles_ellipses_and_holes() {
        let (width, height) = (16, 12);
        let rectangle = Selection::rectangle((2, 2), (9, 8), width, height);
        let paths = paths_from_outlines(outlines_from_selection(&rectangle, width, height));
        assert_eq!(selection(&paths, width, height), rectangle);

        let ellipse = Selection::ellipse_rect(PixelRect::new(2, 1, 9, 8), width, height);
        let paths = paths_from_outlines(outlines_from_selection(&ellipse, width, height));
        assert_eq!(paths[0].anchors.len(), 4);
        assert_same_coverage(&selection(&paths, width, height), &ellipse, width, height);

        let rounded = Selection::rounded_rect_rect(PixelRect::new(2, 1, 9, 8), 3, width, height);
        let paths = paths_from_outlines(outlines_from_selection(&rounded, width, height));
        assert_eq!(paths[0].anchors.len(), 8);
        assert_same_coverage(&selection(&paths, width, height), &rounded, width, height);

        let inverse = Selection::rectangle((4, 3), (9, 7), width, height).invert(width, height);
        let paths = paths_from_outlines(outlines_from_selection(&inverse, width, height));
        assert_eq!(paths.len(), 2);
        assert_same_coverage(&selection(&paths, width, height), &inverse, width, height);

        let edge_inverse =
            Selection::rectangle((0, 3), (5, 8), width, height).invert(width, height);
        let paths = paths_from_outlines(outlines_from_selection(&edge_inverse, width, height));
        assert_same_coverage(
            &selection(&paths, width, height),
            &edge_inverse,
            width,
            height,
        );

        use crate::selection::SelectionOp;
        let donut = Selection::rectangle((2, 2), (13, 10), width, height).combine(
            Selection::rectangle((5, 4), (10, 8), width, height),
            SelectionOp::Subtract,
            width,
            height,
        );
        let paths = paths_from_outlines(outlines_from_selection(&donut, width, height));
        assert!(paths.len() >= 2);
        assert_same_coverage(&selection(&paths, width, height), &donut, width, height);
    }

    #[test]
    fn complex_selection_outlines_stay_within_the_anchor_limit() {
        let points: Vec<(f64, f64)> = (0..180)
            .map(|index| {
                let angle = index as f64 / 180.0 * std::f64::consts::TAU;
                let radius = 24.0 + (index as f64 * 0.7).sin() * 4.0;
                (32.0 + angle.cos() * radius, 32.0 + angle.sin() * radius)
            })
            .collect();
        let selection = Selection::polygons(&[&points], 64, 64);
        let outlines = outlines_from_selection(&selection, 64, 64);
        assert_eq!(outlines.len(), 1);
        assert!(outlines[0].len() <= MAX_ANCHORS);
        assert!(selection.covers(32, 32, 64, 64));
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
