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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HandleMode {
    #[default]
    Independent,
    Aligned,
    Symmetric,
}

#[derive(Clone, Copy, Debug)]
pub struct SegmentHit {
    pub segment: usize,
    pub t: f64,
    pub distance: f64,
}

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

    pub fn move_handle(&mut self, incoming: bool, next: (f64, f64), mode: HandleMode) -> bool {
        if !finite(next) {
            return false;
        }
        if next
            == if incoming {
                self.handle_in
            } else {
                self.handle_out
            }
        {
            return false;
        }
        let mut edited = *self;
        let other = if incoming {
            self.handle_out
        } else {
            self.handle_in
        };
        let delta = (next.0 - self.point.0, next.1 - self.point.1);
        let length = delta.0.hypot(delta.1);
        let opposite = match mode {
            HandleMode::Independent => other,
            HandleMode::Symmetric => (self.point.0 - delta.0, self.point.1 - delta.1),
            HandleMode::Aligned if length > 1e-12 => {
                let original = distance(other, self.point);
                (
                    self.point.0 - delta.0 / length * original,
                    self.point.1 - delta.1 / length * original,
                )
            }
            HandleMode::Aligned => other,
        };
        if incoming {
            edited.handle_in = next;
            edited.handle_out = opposite;
        } else {
            edited.handle_out = next;
            edited.handle_in = opposite;
        }
        if !edited.finite() || edited == *self {
            return false;
        }
        *self = edited;
        true
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

    /// Translate from gesture-start anchors so repeated previews cannot accumulate drift.
    /// Invalid, duplicate, or stale targets reject the complete update.
    pub fn move_anchors_from(&mut self, bases: &[Anchor], delta: (f64, f64)) -> bool {
        if !delta.0.is_finite() || !delta.1.is_finite() || bases.len() > MAX_ANCHORS {
            return false;
        }
        let mut updates = Vec::with_capacity(bases.len());
        for (index, base) in bases.iter().enumerate() {
            if !base.finite()
                || bases[..index].iter().any(|other| other.id == base.id)
                || self.anchor(base.id).is_none()
            {
                return false;
            }
            let translate = |point: (f64, f64)| (point.0 + delta.0, point.1 + delta.1);
            let next = Anchor {
                id: base.id,
                point: translate(base.point),
                handle_in: translate(base.handle_in),
                handle_out: translate(base.handle_out),
            };
            if !next.finite() {
                return false;
            }
            updates.push(next);
        }
        let mut changed = false;
        for next in updates {
            let anchor = self.anchor_mut(next.id).expect("validated anchor");
            changed |= *anchor != next;
            *anchor = next;
        }
        changed
    }

    pub fn make_corner(&mut self, id: AnchorId) -> bool {
        let Some(anchor) = self.anchor_mut(id) else {
            return false;
        };
        let corner = Anchor::corner(id, anchor.point);
        if *anchor == corner {
            return false;
        }
        *anchor = corner;
        true
    }

    pub fn make_smooth(&mut self, id: AnchorId) -> bool {
        let Some(index) = self.anchors.iter().position(|anchor| anchor.id == id) else {
            return false;
        };
        let anchor = self.anchors[index];
        let previous = if index > 0 {
            Some(self.anchors[index - 1].point)
        } else if self.closed {
            self.anchors.last().map(|anchor| anchor.point)
        } else {
            None
        };
        let next = self
            .anchors
            .get(index + 1)
            .or_else(|| self.closed.then(|| &self.anchors[0]))
            .map(|anchor| anchor.point);
        let mut direction = (
            anchor.handle_out.0 - anchor.handle_in.0,
            anchor.handle_out.1 - anchor.handle_in.1,
        );
        if direction.0.hypot(direction.1) <= 1e-12 {
            let from = previous.unwrap_or(anchor.point);
            let to = next.unwrap_or(anchor.point);
            direction = (to.0 - from.0, to.1 - from.1);
        }
        let length = direction.0.hypot(direction.1);
        if !length.is_finite() || length <= 1e-12 {
            return false;
        }
        let incoming = distance(anchor.handle_in, anchor.point);
        let outgoing = distance(anchor.handle_out, anchor.point);
        let incoming = if incoming > 1e-12 {
            incoming
        } else {
            previous
                .or(next)
                .map_or(0., |point| distance(point, anchor.point) / 3.)
        };
        let outgoing = if outgoing > 1e-12 {
            outgoing
        } else {
            next.or(previous)
                .map_or(0., |point| distance(point, anchor.point) / 3.)
        };
        let direction = (direction.0 / length, direction.1 / length);
        let edited = Anchor {
            handle_in: (
                anchor.point.0 - direction.0 * incoming,
                anchor.point.1 - direction.1 * incoming,
            ),
            handle_out: (
                anchor.point.0 + direction.0 * outgoing,
                anchor.point.1 + direction.1 * outgoing,
            ),
            ..anchor
        };
        let tolerance = 1e-12 * (1. + incoming.max(outgoing));
        if !edited.finite()
            || (distance(edited.handle_in, anchor.handle_in) <= tolerance
                && distance(edited.handle_out, anchor.handle_out) <= tolerance)
        {
            return false;
        }
        self.anchors[index] = edited;
        true
    }

    pub fn split_segment(&mut self, segment: usize, t: f64, id: AnchorId) -> Option<Anchor> {
        let count = self.anchors.len();
        if !(2..MAX_ANCHORS).contains(&count)
            || segment >= count - usize::from(!self.closed)
            || !t.is_finite()
            || !(1e-6..=1. - 1e-6).contains(&t)
            || self.anchor(id).is_some()
        {
            return None;
        }
        let next = (segment + 1) % count;
        let first = self.anchors[segment];
        let last = self.anchors[next];
        let (left, right) = split_cubic_at(
            [first.point, first.handle_out, last.handle_in, last.point],
            t,
        );
        let anchor = Anchor {
            id,
            point: left[3],
            handle_in: left[2],
            handle_out: right[1],
        };
        if !anchor.finite() || !finite(left[1]) || !finite(right[2]) {
            return None;
        }
        self.anchors[segment].handle_out = left[1];
        self.anchors[next].handle_in = right[2];
        self.anchors.insert(segment + 1, anchor);
        Some(anchor)
    }

    pub fn nearest_segment(&self, point: (f64, f64), radius: f64) -> Option<SegmentHit> {
        if self.anchors.len() < 2 || !finite(point) || !radius.is_finite() || radius <= 0. {
            return None;
        }
        let count = self.anchors.len() - usize::from(!self.closed);
        let mut best = None;
        let mut limit = radius;
        for segment in 0..count {
            let first = self.anchors[segment];
            let last = self.anchors[(segment + 1) % self.anchors.len()];
            let curve = [first.point, first.handle_out, last.handle_in, last.point];
            if let Some((t, distance)) = nearest_cubic(curve, point, limit) {
                if !(1e-6..=1. - 1e-6).contains(&t) {
                    continue;
                }
                limit = distance;
                best = Some(SegmentHit {
                    segment,
                    t,
                    distance,
                });
            }
        }
        best
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
        point.0 * (f64::from(new.0) / f64::from(old.0)),
        point.1 * (f64::from(new.1) / f64::from(old.1)),
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
    split_cubic_at([p0, p1, p2, p3], 0.5)
}

fn split_cubic_at([p0, p1, p2, p3]: Cubic, t: f64) -> (Cubic, Cubic) {
    let lerp = |a: (f64, f64), b: (f64, f64)| ((1. - t) * a.0 + t * b.0, (1. - t) * a.1 + t * b.1);
    let p01 = lerp(p0, p1);
    let p12 = lerp(p1, p2);
    let p23 = lerp(p2, p3);
    let p012 = lerp(p01, p12);
    let p123 = lerp(p12, p23);
    let p0123 = lerp(p012, p123);
    ([p0, p01, p012, p0123], [p0123, p123, p23, p3])
}

fn nearest_cubic(curve: Cubic, point: (f64, f64), radius: f64) -> Option<(f64, f64)> {
    if !curve.into_iter().all(finite) {
        return None;
    }
    let mut stack = vec![(curve, 0., 1., MAX_DEPTH)];
    let mut best = None;
    let mut limit = radius;
    while let Some((part, from, to, depth)) = stack.pop() {
        let left = part.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let right = part.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let top = part.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let bottom = part.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        if distance(
            point,
            (point.0.clamp(left, right), point.1.clamp(top, bottom)),
        ) > limit
        {
            continue;
        }
        let flat = distance_to_chord(part[1], part[0], part[3])
            .max(distance_to_chord(part[2], part[0], part[3]));
        if depth > 0 && flat > (radius / 8.).max(1e-5) {
            let (left, right) = split_cubic(part[0], part[1], part[2], part[3]);
            let middle = (from + to) * 0.5;
            stack.push((right, middle, to, depth - 1));
            stack.push((left, from, middle, depth - 1));
            continue;
        }
        let delta = (part[3].0 - part[0].0, part[3].1 - part[0].1);
        let length = delta.0 * delta.0 + delta.1 * delta.1;
        let along = if length > 1e-12 {
            (((point.0 - part[0].0) * delta.0 + (point.1 - part[0].1) * delta.1) / length)
                .clamp(0., 1.)
        } else {
            0.5
        };
        // Even a straight cubic can have nonuniform or stationary parameter
        // speed. Invert its chord projection before refining distance, so a
        // zero derivative cannot trap Newton's method away from the click.
        let mut local = along;
        if length.is_finite() && length > 1e-12 {
            let projected = |at: (f64, f64)| {
                ((at.0 - part[0].0) * delta.0 + (at.1 - part[0].1) * delta.1) / length
            };
            let (p1, p2) = (projected(part[1]), projected(part[2]));
            let (mut low, mut high) = (0., 1.);
            for _ in 0..40 {
                local = (low + high) * 0.5;
                let projected = 3. * (1. - local).powi(2) * local * p1
                    + 3. * (1. - local) * local * local * p2
                    + local.powi(3);
                if projected < along {
                    low = local;
                } else {
                    high = local;
                }
            }
        }
        let projected_t = from + (to - from) * local;
        let mut t = projected_t;
        for _ in 0..8 {
            let at = split_cubic_at(curve, t).0[3];
            let d = |axis: usize| {
                let p = curve.map(|point| if axis == 0 { point.0 } else { point.1 });
                (
                    3. * ((1. - t).powi(2) * (p[1] - p[0])
                        + 2. * (1. - t) * t * (p[2] - p[1])
                        + t * t * (p[3] - p[2])),
                    6. * ((1. - t) * (p[2] - 2. * p[1] + p[0]) + t * (p[3] - 2. * p[2] + p[1])),
                )
            };
            let (dx, ddx) = d(0);
            let (dy, ddy) = d(1);
            let denominator = dx * dx + dy * dy + (at.0 - point.0) * ddx + (at.1 - point.1) * ddy;
            if !denominator.is_finite() || denominator.abs() <= 1e-12 {
                break;
            }
            let next =
                (t - ((at.0 - point.0) * dx + (at.1 - point.1) * dy) / denominator).clamp(from, to);
            if !next.is_finite() {
                break;
            }
            t = next;
        }
        for candidate in [from, projected_t, t, to] {
            let distance = distance(point, split_cubic_at(curve, candidate).0[3]);
            if distance.is_finite() && distance <= limit {
                limit = distance;
                best = Some((candidate, distance));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve_path() -> VectorPath {
        VectorPath {
            id: PathId(1),
            visible: true,
            closed: false,
            anchors: vec![
                Anchor {
                    id: AnchorId(1),
                    point: (0., 0.),
                    handle_in: (-2., 0.),
                    handle_out: (0., 20.),
                },
                Anchor {
                    id: AnchorId(2),
                    point: (20., 0.),
                    handle_in: (20., 20.),
                    handle_out: (22., 0.),
                },
            ],
        }
    }

    fn cubic_at(points: Cubic, t: f64) -> (f64, f64) {
        let weights = [
            (1. - t).powi(3),
            3. * t * (1. - t).powi(2),
            3. * t * t * (1. - t),
            t.powi(3),
        ];
        points
            .iter()
            .zip(weights)
            .fold((0., 0.), |sum, (point, weight)| {
                (sum.0 + point.0 * weight, sum.1 + point.1 * weight)
            })
    }

    #[test]
    fn grouped_anchor_motion_uses_its_baseline_and_preserves_unselected_geometry() {
        let mut path = curve_path();
        path.anchors.push(Anchor::corner(AnchorId(3), (20., -30.)));
        path.closed = true;
        let original = path.clone();
        let bases = vec![path.anchors[0], path.anchors[2]];
        assert!(!path.move_anchors_from(&bases, (0., 0.)));
        for delta in [(5.25, -7.5), (-3., 4.), (5.25, -7.5)] {
            assert!(path.move_anchors_from(&bases, delta));
            for base in &bases {
                let moved = path.anchor(base.id).unwrap();
                assert_eq!(
                    moved.point,
                    (base.point.0 + delta.0, base.point.1 + delta.1)
                );
                assert_eq!(
                    moved.handle_in,
                    (base.handle_in.0 + delta.0, base.handle_in.1 + delta.1)
                );
                assert_eq!(
                    moved.handle_out,
                    (base.handle_out.0 + delta.0, base.handle_out.1 + delta.1)
                );
            }
            assert_eq!(path.anchors[1], original.anchors[1]);
            assert!(path.closed);
            assert_eq!(path.id, original.id);
        }
        assert!(!path.move_anchors_from(&bases, (5.25, -7.5)));
        assert!(path.move_anchors_from(&bases, (0., 0.)));
        assert_eq!(path, original);
    }

    #[test]
    fn invalid_group_motion_is_atomic_for_stale_duplicate_and_nonfinite_targets() {
        let mut path = curve_path();
        let original = path.clone();
        let bases = path.anchors.clone();
        let stale = Anchor::corner(AnchorId(999), (0., 0.));
        let mut invalid = bases[1];
        invalid.handle_out.0 = f64::NAN;
        for selected in [
            vec![bases[0], stale],
            vec![bases[0], bases[0]],
            vec![bases[0], invalid],
        ] {
            assert!(!path.move_anchors_from(&selected, (3., 5.)));
            assert_eq!(path, original);
        }
        for delta in [(f64::NAN, 0.), (0., f64::INFINITY)] {
            assert!(!path.move_anchors_from(&bases, delta));
            assert_eq!(path, original);
        }
        let mut overflowing = bases.clone();
        overflowing[1].handle_in.0 = f64::MAX;
        assert!(!path.move_anchors_from(&overflowing, (f64::MAX, 0.)));
        assert_eq!(path, original);
        assert!(!path.move_anchors_from(&[], (1., 2.)));
    }

    #[test]
    fn inserting_an_anchor_splits_the_cubic_without_changing_its_geometry_or_ids() {
        let original = curve_path();
        let first = original.anchors[0];
        let last = original.anchors[1];
        let curve = [first.point, first.handle_out, last.handle_in, last.point];
        for t in [0.13, 0.37, 0.5, 0.91] {
            let mut path = original.clone();
            let added = path.split_segment(0, t, AnchorId(3)).unwrap();
            assert_eq!(path.anchors.len(), 3);
            assert_eq!(path.anchors[0].id, first.id);
            assert_eq!(path.anchors[1], added);
            assert_eq!(path.anchors[2].id, last.id);
            assert_eq!(path.anchors[0].handle_in, first.handle_in);
            assert_eq!(path.anchors[2].handle_out, last.handle_out);
            for sample in 0..=100 {
                let at = f64::from(sample) / 100.;
                let (segment, u) = if at <= t {
                    (0, at / t)
                } else {
                    (1, (at - t) / (1. - t))
                };
                let a = path.anchors[segment];
                let b = path.anchors[segment + 1];
                assert!(
                    distance(
                        cubic_at(curve, at),
                        cubic_at([a.point, a.handle_out, b.handle_in, b.point], u)
                    ) < 1e-10
                );
            }
            assert!(!path.closed);
        }
        let mut closed = square(2, (0., 0.), 10., false);
        let original = closed.clone();
        let added = closed.split_segment(3, 0.5, AnchorId(99)).unwrap();
        assert!(closed.closed);
        assert_eq!(closed.anchors[0].id, original.anchors[0].id);
        assert_eq!(closed.anchors[4], added);
        assert_eq!(added.point, (0., 5.));
        assert_eq!(selection(&[closed], 12, 12), selection(&[original], 12, 12));
    }

    #[test]
    fn segment_hits_follow_cubic_geometry_and_only_include_real_open_or_closed_edges() {
        let path = curve_path();
        let curve = [
            path.anchors[0].point,
            path.anchors[0].handle_out,
            path.anchors[1].handle_in,
            path.anchors[1].point,
        ];
        for t in [0.1, 0.23, 0.5, 0.78, 0.9] {
            let point = cubic_at(curve, t);
            let hit = path.nearest_segment(point, 0.1).unwrap();
            assert_eq!(hit.segment, 0);
            assert!((hit.t - t).abs() < 1e-8);
            assert!(hit.distance < 1e-7);
        }
        assert!(path.nearest_segment((10., 30.), 0.1).is_none());
        let stationary = VectorPath {
            anchors: vec![
                Anchor {
                    id: AnchorId(1),
                    point: (0., 0.),
                    handle_in: (0., 0.),
                    handle_out: (10., 0.),
                },
                Anchor {
                    id: AnchorId(2),
                    point: (10., 0.),
                    handle_in: (0., 0.),
                    handle_out: (10., 0.),
                },
            ],
            ..path.clone()
        };
        let hit = stationary.nearest_segment((5.1, 0.), 0.001).unwrap();
        assert!(hit.distance < 1e-7);
        for (point, radius) in [
            ((f64::NAN, 2.), 1.),
            ((1., 1.), 0.),
            ((1., 1.), f64::INFINITY),
        ] {
            assert!(path.nearest_segment(point, radius).is_none());
        }
        let mut rectangle = square(2, (0., 0.), 10., false);
        assert_eq!(rectangle.nearest_segment((0., 5.), 0.1).unwrap().segment, 3);
        rectangle.closed = false;
        assert!(rectangle.nearest_segment((0., 5.), 0.1).is_none());
    }

    #[test]
    fn invalid_or_over_budget_insertions_leave_the_path_intact_and_large_finite_splits_stay_finite()
    {
        let original = curve_path();
        for (segment, t, id) in [
            (0, 0., AnchorId(3)),
            (0, 1., AnchorId(3)),
            (0, f64::NAN, AnchorId(3)),
            (4, 0.5, AnchorId(3)),
            (0, 0.5, AnchorId(1)),
        ] {
            let mut path = original.clone();
            assert!(path.split_segment(segment, t, id).is_none());
            assert_eq!(path, original);
        }
        let mut full = VectorPath {
            anchors: (0..MAX_ANCHORS)
                .map(|index| Anchor::corner(AnchorId(index as u64), (index as f64, 0.)))
                .collect(),
            ..original.clone()
        };
        let before = full.clone();
        assert!(full.split_segment(0, 0.5, AnchorId(999)).is_none());
        assert_eq!(full, before);
        let mut huge = VectorPath {
            anchors: vec![
                Anchor::corner(AnchorId(1), (-1e308, 0.)),
                Anchor::corner(AnchorId(2), (1e308, 0.)),
            ],
            ..original
        };
        assert!(huge.split_segment(0, 0.5, AnchorId(3)).unwrap().finite());
        assert!(huge.anchors.iter().all(|anchor| anchor.finite()));
    }

    #[test]
    fn handle_drag_modes_keep_opposite_lengths_or_mirror_them_without_nonfinite_edits() {
        let original = Anchor {
            id: AnchorId(1),
            point: (4., 5.),
            handle_in: (1., 5.),
            handle_out: (6., 5.),
        };
        for (mode, expected) in [
            (HandleMode::Independent, (1., 5.)),
            (HandleMode::Aligned, (4., 2.)),
            (HandleMode::Symmetric, (4., 1.)),
        ] {
            let mut anchor = original;
            assert!(anchor.move_handle(false, (4., 9.), mode));
            assert_eq!(anchor.handle_out, (4., 9.));
            assert_eq!(anchor.handle_in, expected);
            let before = anchor;
            assert!(!anchor.move_handle(true, (f64::NAN, 5.), mode));
            assert_eq!(anchor, before);
        }
        let mut collapsed = original;
        collapsed.move_handle(false, original.point, HandleMode::Aligned);
        assert_eq!(collapsed.handle_in, original.handle_in);
        collapsed = original;
        collapsed.move_handle(false, original.point, HandleMode::Symmetric);
        assert_eq!(collapsed, Anchor::corner(original.id, original.point));
        let mut unchanged = original;
        assert!(!unchanged.move_handle(false, original.handle_out, HandleMode::Symmetric));
        assert_eq!(unchanged, original);
    }

    #[test]
    fn corner_and_smooth_commands_handle_endpoints_and_preserve_existing_handle_lengths() {
        let mut path = square(2, (0., 0.), 10., false);
        let id = path.anchors[1].id;
        assert!(path.make_smooth(id));
        let smooth = *path.anchor(id).unwrap();
        assert!((distance(smooth.point, smooth.handle_in) - 10. / 3.).abs() < 1e-10);
        assert!((distance(smooth.point, smooth.handle_out) - 10. / 3.).abs() < 1e-10);
        assert!(!path.make_smooth(id));
        assert!(path.make_corner(id));
        assert_eq!(*path.anchor(id).unwrap(), Anchor::corner(id, smooth.point));
        assert!(!path.make_corner(id));
        let mut open = curve_path();
        let first = open.anchors[0].id;
        let incoming = distance(open.anchors[0].handle_in, open.anchors[0].point);
        let outgoing = distance(open.anchors[0].handle_out, open.anchors[0].point);
        assert!(open.make_smooth(first));
        assert!(
            (distance(open.anchors[0].handle_in, open.anchors[0].point) - incoming).abs() < 1e-10
        );
        assert!(
            (distance(open.anchors[0].handle_out, open.anchors[0].point) - outgoing).abs() < 1e-10
        );
        assert!(!open.make_smooth(first));
        let mut single = VectorPath {
            anchors: vec![Anchor::corner(AnchorId(9), (1., 1.))],
            ..open
        };
        assert!(!single.make_smooth(AnchorId(9)));
    }

    #[test]
    fn edited_path_geometry_round_trips_in_the_existing_project_format() {
        use image::RgbaImage;
        let mut path = curve_path();
        let added = path.split_segment(0, 0.37, AnchorId(3)).unwrap();
        path.make_corner(added.id);
        path.make_smooth(added.id);
        let mut content =
            crate::document::DocumentContent::single_layer("path".into(), RgbaImage::new(2, 2));
        assert!(content.upsert_path(path.clone()));
        let mut bytes = Vec::new();
        crate::project::write(&mut bytes, &content).unwrap();
        let manifest_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes[12..12 + manifest_size]).unwrap();
        assert_eq!(manifest["version"], 6);
        let reopened = crate::project::read(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(reopened.paths(), &[path]);
    }

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

    #[test]
    fn simplifying_a_closed_outline_preserves_the_shared_farthest_vertex() {
        let square = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        assert_eq!(simplify_closed_at(&square, 0.5), square);
    }
}
