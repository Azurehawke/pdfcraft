//! RevuCraft's doors and windows: dropped into a wall, they take its thickness, line up with it
//! and cut the opening. A wall is either a thick line or polyline (drawn with a real-world width)
//! or one of the Wall tool's outlines (a four-sided polygon per run). The opening is cut out of
//! the wall's own markup and the door or window is drawn in its place as ordinary markup, all in
//! one undoable edit.

use pdfcraft_engine::geometry::{Geometry, Kind};
use pdfcraft_engine::{Edit, LineEnding, Shape};

use crate::draft_edit::{Pt, arc_points, piece, running_lengths};

/// Door and window lines on paper, points.
const PEN: f64 = 0.75;
/// A window is drawn at least this thick on paper (a wall drawn as a hairline still shows one).
const MIN_WINDOW: f64 = 4.0;
/// Pieces of wall shorter than this (points) are left out rather than kept as slivers.
const SLIVER: f64 = 0.5;

/// What is dropped into the wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opening {
    Door,
    Window,
}

/// How a wall is drawn.
#[derive(Clone, Debug, PartialEq)]
enum Body {
    /// A thick line or polyline: its points, and which segment was hit.
    Path { points: Vec<Pt>, segment: usize },
    /// A Wall-tool outline: the four corners, with the faces running from 0 to 1 and 3 to 2.
    Quad([Pt; 4]),
}

/// A wall under the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    pub index: usize,
    pub locked: bool,
    /// The centre line of the run that was hit.
    pub a: Pt,
    pub b: Pt,
    /// Its thickness, points.
    pub thickness: f64,
    body: Body,
}

impl Wall {
    /// A Wall-tool outline (a closed shape) rather than a thick line.
    pub fn is_outline(&self) -> bool {
        matches!(self.body, Body::Quad(_))
    }
}

/// An opening placed in a wall: its ends on the centre line, the unit direction along the wall
/// and the unit normal toward the side the pointer is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub from: Pt,
    pub to: Pt,
    pub along: Pt,
    pub normal: Pt,
    pub half: f64,
}

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: Pt, b: Pt) -> Pt {
    [a[0] + b[0], a[1] + b[1]]
}

fn mul(a: Pt, k: f64) -> Pt {
    [a[0] * k, a[1] * k]
}

fn len(a: Pt) -> f64 {
    a[0].hypot(a[1])
}

fn mid(a: Pt, b: Pt) -> Pt {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

fn cross(a: Pt, b: Pt) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn unit(a: Pt) -> Option<Pt> {
    let l = len(a);
    (l.is_finite() && l > 1e-9).then(|| [a[0] / l, a[1] / l])
}

/// The distance from `p` to the segment `a–b`.
fn to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let ab = sub(b, a);
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if l2 < 1e-18 { 0.0 } else { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0) };
    len(sub(p, add(a, mul(ab, t))))
}

/// Whether `p` is inside the convex quadrilateral `q`.
fn inside(q: &[Pt; 4], p: Pt) -> bool {
    let mut sign = 0.0;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let c = cross(sub(b, a), sub(p, a));
        if c.abs() < 1e-12 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

/// A Wall-tool outline read as a wall: the faces are the longer pair of opposite sides.
fn quad_wall(g: &Geometry) -> Option<Wall> {
    let [p0, p1, p2, p3] = <[Pt; 4]>::try_from(g.points.as_slice()).ok()?;
    let side = |a: Pt, b: Pt| len(sub(b, a));
    // Faces 0–1 and 3–2, or (turned by one corner) 1–2 and 0–3.
    let q = if side(p0, p1) + side(p3, p2) >= side(p1, p2) + side(p0, p3) { [p0, p1, p2, p3] } else { [p1, p2, p3, p0] };
    let (a, b) = (mid(q[0], q[3]), mid(q[1], q[2]));
    let along = unit(sub(b, a))?;
    // The thickness is the distance between the two faces, across the wall.
    let thickness = cross(along, sub(q[3], q[0])).abs().max(cross(along, sub(q[2], q[1])).abs());
    (thickness.is_finite() && thickness > 0.01).then(|| Wall { index: g.index, locked: g.locked, a, b, thickness, body: Body::Quad(q) })
}

/// The wall under `p` (within `tolerance` of its edge), nearest first.
pub fn wall_at(shapes: &[Geometry], p: Pt, tolerance: f64) -> Option<Wall> {
    let mut best: Option<(Wall, f64)> = None;
    for g in shapes {
        let found = match g.kind {
            Kind::Polygon if g.points.len() == 4 => quad_wall(g).and_then(|w| {
                let Body::Quad(q) = &w.body else { return None };
                let d = if inside(q, p) { 0.0 } else { to_segment(p, w.a, w.b) - w.thickness / 2.0 };
                (d <= tolerance).then_some((w, d))
            }),
            Kind::Line | Kind::PolyLine => {
                let thickness = if g.width.is_finite() { g.width.clamp(0.1, 500.0) } else { 1.0 };
                let mut hit: Option<(usize, f64)> = None;
                for (i, w) in g.points.windows(2).enumerate() {
                    let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
                    let d = (to_segment(p, *a, *b) - thickness / 2.0).max(0.0);
                    if d <= tolerance && hit.is_none_or(|(_, bd)| d < bd) {
                        hit = Some((i, d));
                    }
                }
                hit.and_then(|(segment, d)| {
                    let (a, b) = (*g.points.get(segment)?, *g.points.get(segment + 1)?);
                    Some((Wall { index: g.index, locked: g.locked, a, b, thickness, body: Body::Path { points: g.points.clone(), segment } }, d))
                })
            }
            _ => None,
        };
        if let Some((w, d)) = found
            && best.as_ref().is_none_or(|(_, bd)| d < *bd)
        {
            best = Some((w, d));
        }
    }
    best.map(|(w, _)| w)
}

/// Where an opening `width` points wide goes in `wall`, centred on the point nearest `p` and kept
/// inside the run. `None` when the run is shorter than the opening.
pub fn place(wall: &Wall, p: Pt, width: f64) -> Option<Placement> {
    let along = unit(sub(wall.b, wall.a))?;
    let run = len(sub(wall.b, wall.a));
    if !width.is_finite() || width <= 0.0 || run < width {
        return None;
    }
    let s = ((p[0] - wall.a[0]) * along[0] + (p[1] - wall.a[1]) * along[1]).clamp(width / 2.0, run - width / 2.0);
    let left = [-along[1], along[0]];
    let normal = if cross(along, sub(p, wall.a)) < 0.0 { mul(left, -1.0) } else { left };
    let from = add(wall.a, mul(along, s - width / 2.0));
    let to = add(wall.a, mul(along, s + width / 2.0));
    Some(Placement { from, to, along, normal, half: wall.thickness / 2.0 })
}

/// The wall's markup with the opening cut out: the vertex lists that replace it (the first in
/// place, the rest as copies).
pub fn cut(wall: &Wall, at: &Placement) -> Vec<Vec<Pt>> {
    let keep = |v: Vec<Pt>| -> Option<Vec<Pt>> {
        let length: f64 = v.windows(2).filter_map(|w| Some(len(sub(*w.get(1)?, *w.first()?)))).sum();
        (v.len() >= 2 && length >= SLIVER).then_some(v)
    };
    match &wall.body {
        Body::Path { points, segment } => {
            let lengths = running_lengths(points);
            let total = lengths.last().copied().unwrap_or(0.0);
            let start = lengths.get(*segment).copied().unwrap_or(0.0);
            // Line caps reach half the width past a line's end, so the pieces stop that much
            // short of the opening for it to show its full width.
            let s0 = (start + len(sub(at.from, wall.a)) - at.half).max(0.0);
            let s1 = (start + len(sub(at.to, wall.a)) + at.half).min(total);
            [piece(points, &lengths, 0.0, s0), piece(points, &lengths, s1, total)].into_iter().filter_map(keep).collect()
        }
        Body::Quad(q) => {
            // The face through corner 0 is on this side of the centre line.
            let left = [-at.along[1], at.along[0]];
            let n0 = if cross(at.along, sub(q[0], wall.a)) >= 0.0 { left } else { mul(left, -1.0) };
            let h = at.half;
            let face = |o: Pt, k: f64| add(o, mul(n0, h * k));
            let first = vec![q[0], face(at.from, 1.0), face(at.from, -1.0), q[3]];
            let second = vec![face(at.to, 1.0), q[1], q[2], face(at.to, -1.0)];
            // A piece with no length along the wall (the opening reaches the end) is left out.
            let kept = |v: &Vec<Pt>| match v.as_slice() {
                [p0, p1, p2, p3] => len(sub(*p1, *p0)) >= SLIVER || len(sub(*p2, *p3)) >= SLIVER,
                _ => false,
            };
            [first, second].into_iter().filter(kept).collect()
        }
    }
}

/// The markup that draws the door or window in the opening. A door hinges at the opening's
/// start (its end with `flip`) and swings toward the side the pointer is on.
pub fn drawing(kind: Opening, at: &Placement, flip: bool) -> Vec<Shape> {
    let line = |from: Pt, to: Pt| Shape::Line { from, to, start: LineEnding::None, end: LineEnding::None };
    match kind {
        Opening::Door => {
            let (hinge, strike) = if flip { (at.to, at.from) } else { (at.from, at.to) };
            let width = len(sub(strike, hinge));
            let h = add(hinge, mul(at.normal, at.half));
            let s = add(strike, mul(at.normal, at.half));
            let tip = add(h, mul(at.normal, width));
            let toward = unit(sub(strike, hinge)).unwrap_or(at.along);
            let k = std::f64::consts::FRAC_1_SQRT_2 * width;
            let m = add(h, add(mul(toward, k), mul(at.normal, k)));
            vec![line(h, tip), Shape::PolyLine { vertices: arc_points(tip, m, s), start: LineEnding::None, end: LineEnding::None }]
        }
        Opening::Window => {
            let half = at.half.max(MIN_WINDOW / 2.0);
            let n = at.normal;
            let c = |o: Pt, k: f64| add(o, mul(n, half * k));
            vec![
                Shape::Polygon { vertices: vec![c(at.from, -1.0), c(at.to, -1.0), c(at.to, 1.0), c(at.from, 1.0)], cloud: false },
                line(at.from, at.to),
            ]
        }
    }
}

/// The one edit that cuts the opening and draws the door or window.
pub fn edit(page: usize, wall: &Wall, at: &Placement, kind: Opening, flip: bool, color: [f64; 3], author: &str) -> Edit {
    let mut edits = Vec::new();
    let mut pieces = cut(wall, at).into_iter();
    match pieces.next() {
        Some(first) => {
            edits.push(Edit::SetVertices { page, index: wall.index, vertices: first, copy: false });
            edits.extend(pieces.map(|p| Edit::SetVertices { page, index: wall.index, vertices: p, copy: true }));
        }
        None => edits.push(Edit::DeleteAnnotation { page, index: wall.index }),
    }
    // A window over the cut is filled white so the wall's fill doesn't show through.
    let fill = (kind == Opening::Window).then_some([1.0, 1.0, 1.0]);
    for shape in drawing(kind, at, flip) {
        let fill = if matches!(shape, Shape::Polygon { .. }) { fill } else { None };
        edits.push(crate::draft::annotation(page, shape, color, PEN, fill, author));
    }
    let label = match kind {
        Opening::Door => "Add door",
        Opening::Window => "Add window",
    };
    Edit::Batch { label: label.into(), edits }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(points: Vec<Pt>, width: f64) -> Geometry {
        Geometry { index: 3, kind: Kind::PolyLine, points, locked: false, width, real_width: true }
    }

    #[test]
    fn a_door_cuts_a_thick_line_and_hinges_on_the_wall_face() {
        let wall = line(vec![[0.0, 0.0], [200.0, 0.0], [200.0, 100.0]], 6.0);
        let w = wall_at(&[wall], [50.0, 5.0], 4.0).expect("the wall is found");
        assert_eq!((w.thickness, w.a, w.b), (6.0, [0.0, 0.0], [200.0, 0.0]));
        let at = place(&w, [50.0, 5.0], 30.0).unwrap();
        assert_eq!((at.from, at.to, at.normal), ([35.0, 0.0], [65.0, 0.0], [0.0, 1.0]));
        let pieces = cut(&w, &at);
        assert_eq!(pieces, vec![vec![[0.0, 0.0], [32.0, 0.0]], vec![[68.0, 0.0], [200.0, 0.0], [200.0, 100.0]]]);
        let shapes = drawing(Opening::Door, &at, false);
        let Some(Shape::Line { from, to, .. }) = shapes.first() else { panic!("the leaf") };
        assert_eq!((*from, *to), ([35.0, 3.0], [35.0, 33.0]));
        let Some(Shape::PolyLine { vertices, .. }) = shapes.get(1) else { panic!("the swing") };
        let last = vertices.last().unwrap();
        assert!((last[0] - 65.0).abs() < 1e-6 && (last[1] - 3.0).abs() < 1e-6, "{last:?}");
        // Near an end, the opening stays inside the run; a short run is refused.
        assert_eq!(place(&w, [2.0, 0.0], 30.0).unwrap().from, [0.0, 0.0]);
        assert!(place(&w, [50.0, 0.0], 500.0).is_none());
    }

    #[test]
    fn a_window_cuts_a_wall_outline_into_two() {
        let quad = Geometry {
            index: 1,
            kind: Kind::Polygon,
            points: vec![[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]],
            locked: false,
            width: 0.75,
            real_width: false,
        };
        let w = wall_at(&[quad], [40.0, 1.0], 2.0).expect("inside the outline");
        assert!((w.thickness - 6.0).abs() < 1e-9);
        let at = place(&w, [40.0, -1.0], 20.0).unwrap();
        assert_eq!(at.normal, [0.0, -1.0]);
        let pieces = cut(&w, &at);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0], vec![[0.0, 3.0], [30.0, 3.0], [30.0, -3.0], [0.0, -3.0]]);
        assert_eq!(pieces[1], vec![[50.0, 3.0], [100.0, 3.0], [100.0, -3.0], [50.0, -3.0]]);
        let Edit::Batch { edits, .. } = edit(0, &w, &at, Opening::Window, false, [0.0; 3], "Steve") else { panic!("one edit") };
        assert_eq!(edits.len(), 4, "the cut wall, its copy, the frame and the glass");
        // Nothing there: no wall.
        assert!(wall_at(&[], [0.0, 0.0], 5.0).is_none());
    }
}
