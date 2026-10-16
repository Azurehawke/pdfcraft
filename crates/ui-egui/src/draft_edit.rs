//! RevuCraft's drafting edits: grips on the selected line, polyline or polygon; Move, Copy and
//! Rotate from a base point (clicked or typed); Offset, Trim and Extend; and the plane geometry
//! they share. Every change is one undoable edit on ordinary markup.

use egui::{Color32, Pos2, Stroke, vec2};
use pdfcraft_engine::geometry::{self, Geometry, Kind};
use pdfcraft_engine::{Document, Edit};

use crate::canvas::DocView;
use crate::comments::PageCx;
use crate::draft::{DraftSettings, DraftTool};
use crate::theme;

pub type Pt = [f64; 2];

/// Grip size on screen, and how near a press must be to take one.
const GRIP_PX: f32 = 8.0;

// ---- plane geometry ----------------------------------------------------------------------------

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

fn dist(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn lerp(a: Pt, b: Pt, t: f64) -> Pt {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn finite(p: Pt) -> bool {
    p[0].is_finite() && p[1].is_finite() && p[0].abs() < 1e7 && p[1].abs() < 1e7
}

/// Points without repeats (and without a closing repeat of the first, for a closed shape).
fn dedup(points: &[Pt], closed: bool) -> Vec<Pt> {
    let mut pts: Vec<Pt> = Vec::new();
    for p in points {
        if pts.last().is_none_or(|l| dist(*l, *p) > 1e-6) {
            pts.push(*p);
        }
    }
    if closed && pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| dist(*a, *b) < 1e-6) {
        pts.pop();
    }
    pts
}

/// Each vertex's offset `half` to the left of the direction of travel, mitred at the corners
/// (a hairpin gets no spike). Shared with walls.
pub(crate) fn mitre_offsets(pts: &[Pt], half: f64, closed: bool) -> Vec<Pt> {
    let n = pts.len();
    if n < 2 {
        return vec![[0.0, 0.0]; n];
    }
    let normal = |a: Pt, b: Pt| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let d = dx.hypot(dy).max(1e-12);
        [-dy / d, dx / d]
    };
    let get = |i: isize| -> Pt { pts.get(i.rem_euclid(n as isize) as usize).copied().unwrap_or([0.0, 0.0]) };
    (0..n as isize)
        .map(|i| {
            let has_prev = closed || i > 0;
            let has_next = closed || i + 1 < n as isize;
            let n_in = has_prev.then(|| normal(get(i - 1), get(i)));
            let n_out = has_next.then(|| normal(get(i), get(i + 1)));
            match (n_in, n_out) {
                (Some(a), Some(b)) => {
                    let m = [a[0] + b[0], a[1] + b[1]];
                    let len = m[0].hypot(m[1]);
                    let dot = if len < 1e-9 { 0.0 } else { (m[0] * a[0] + m[1] * a[1]) / len };
                    if dot < 0.25 { [a[0] * half, a[1] * half] } else { [m[0] / len * half / dot, m[1] / len * half / dot] }
                }
                (Some(a), None) | (None, Some(a)) => [a[0] * half, a[1] * half],
                (None, None) => [0.0, 0.0],
            }
        })
        .collect()
}

/// A copy of the path `dist` to its left (negative: to its right), corners mitred.
pub fn offset_path(points: &[Pt], dist: f64, closed: bool) -> Vec<Pt> {
    let pts = dedup(points, closed);
    if pts.len() < 2 || !dist.is_finite() {
        return Vec::new();
    }
    let offs = mitre_offsets(&pts, dist.abs(), closed);
    let sign = dist.signum();
    pts.iter().zip(offs).map(|(p, o)| [p[0] + sign * o[0], p[1] + sign * o[1]]).collect()
}

/// Where segments `a–b` and `c–d` cross, as fractions along each (`None` when parallel).
pub fn intersect(a: Pt, b: Pt, c: Pt, d: Pt) -> Option<(f64, f64)> {
    let r = sub(b, a);
    let s = sub(d, c);
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let q = sub(c, a);
    let t = (q[0] * s[1] - q[1] * s[0]) / den;
    let u = (q[0] * r[1] - q[1] * r[0]) / den;
    (t.is_finite() && u.is_finite()).then_some((t, u))
}

/// The distance from `p` to the segment `a–b`, and how far along it the nearest point is (0–1).
fn to_segment(p: Pt, a: Pt, b: Pt) -> (f64, f64) {
    let ab = sub(b, a);
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 < 1e-18 { 0.0 } else { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0) };
    (dist(p, lerp(a, b, t)), t)
}

/// Distance along an open path to each vertex (starting at 0).
fn running_lengths(points: &[Pt]) -> Vec<f64> {
    let mut out = Vec::with_capacity(points.len());
    let mut total = 0.0;
    for (i, p) in points.iter().enumerate() {
        if let Some(prev) = i.checked_sub(1).and_then(|j| points.get(j)) {
            total += dist(*prev, *p);
        }
        out.push(total);
    }
    out
}

/// The point `s` along an open path.
fn point_along(points: &[Pt], lengths: &[f64], s: f64) -> Option<Pt> {
    for (i, w) in points.windows(2).enumerate() {
        let (&[a, b], Some(&la), Some(&lb)) = (w, lengths.get(i), lengths.get(i + 1)) else { continue };
        if s <= lb + 1e-9 {
            let span = lb - la;
            return Some(if span < 1e-12 { a } else { lerp(a, b, ((s - la) / span).clamp(0.0, 1.0)) });
        }
    }
    points.last().copied()
}

/// How far along an open path the point nearest `p` is, and how far `p` is from it.
fn project(points: &[Pt], p: Pt) -> Option<(f64, f64)> {
    let lengths = running_lengths(points);
    let mut best: Option<(f64, f64)> = None;
    for (i, w) in points.windows(2).enumerate() {
        let (&[a, b], Some(&la)) = (w, lengths.get(i)) else { continue };
        let (d, t) = to_segment(p, a, b);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((la + t * dist(a, b), d));
        }
    }
    best
}

/// The part of an open path from `s0` to `s1` along it.
fn piece(points: &[Pt], lengths: &[f64], s0: f64, s1: f64) -> Vec<Pt> {
    let mut out = Vec::new();
    if let Some(p) = point_along(points, lengths, s0) {
        out.push(p);
    }
    for (p, l) in points.iter().zip(lengths) {
        if *l > s0 + 1e-9 && *l < s1 - 1e-9 {
            out.push(*p);
        }
    }
    if let Some(p) = point_along(points, lengths, s1) {
        out.push(p);
    }
    dedup(&out, false)
}

/// Where other shapes cross an open path, as distances along it (ends excluded).
fn crossings(points: &[Pt], others: &[(Pt, Pt)]) -> Vec<f64> {
    let lengths = running_lengths(points);
    let total = lengths.last().copied().unwrap_or(0.0);
    let mut cuts = Vec::new();
    for (i, w) in points.windows(2).enumerate() {
        let (&[a, b], Some(&la)) = (w, lengths.get(i)) else { continue };
        for (c, d) in others {
            if let Some((t, u)) = intersect(a, b, *c, *d)
                && (-1e-9..=1.0 + 1e-9).contains(&t)
                && (-1e-9..=1.0 + 1e-9).contains(&u)
            {
                let s = la + t.clamp(0.0, 1.0) * dist(a, b);
                if s > 1e-6 && s < total - 1e-6 {
                    cuts.push(s);
                }
            }
        }
    }
    cuts
}

/// Trim an open path where it is clicked (`at` along it): the part between the nearest
/// crossings either side goes. The pieces left (none when the whole path goes), or `None` when
/// nothing crosses it on either side.
pub fn trim(points: &[Pt], others: &[(Pt, Pt)], at: f64) -> Option<Vec<Vec<Pt>>> {
    let lengths = running_lengths(points);
    let total = lengths.last().copied().unwrap_or(0.0);
    let cuts = crossings(points, others);
    let before = cuts.iter().copied().filter(|s| *s < at).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))));
    let after = cuts.iter().copied().filter(|s| *s > at).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.min(s))));
    if before.is_none() && after.is_none() {
        return None;
    }
    let mut pieces = Vec::new();
    if let Some(b) = before {
        pieces.push(piece(points, &lengths, 0.0, b));
    }
    if let Some(a) = after {
        pieces.push(piece(points, &lengths, a, total));
    }
    pieces.retain(|p| p.len() >= 2 && running_lengths(p).last().is_some_and(|l| *l >= 1.0));
    Some(pieces)
}

/// The new end of an open path extended at its end (or its start) to the nearest shape its last
/// segment points at.
pub fn extend(points: &[Pt], at_end: bool, others: &[(Pt, Pt)]) -> Option<Pt> {
    let (end, prev) = if at_end { (*points.last()?, *points.get(points.len().checked_sub(2)?)?) } else { (*points.first()?, *points.get(1)?) };
    let d = dist(prev, end);
    if d < 1e-9 {
        return None;
    }
    let dir = [(end[0] - prev[0]) / d, (end[1] - prev[1]) / d];
    let ahead = [end[0] + dir[0], end[1] + dir[1]];
    let mut best: Option<f64> = None;
    for (c, e) in others {
        if let Some((t, u)) = intersect(end, ahead, *c, *e)
            && t > 1e-6
            && t < 1e6
            && (-1e-9..=1.0 + 1e-9).contains(&u)
            && best.is_none_or(|b| t < b)
        {
            best = Some(t);
        }
    }
    best.map(|t| [end[0] + dir[0] * t, end[1] + dir[1] * t]).filter(|p| finite(*p))
}

/// An arc from `a` through `m` to `b`, as points about 5° apart; a straight line when the three
/// are in a row.
pub fn arc_points(a: Pt, m: Pt, b: Pt) -> Vec<Pt> {
    let d = 2.0 * (a[0] * (m[1] - b[1]) + m[0] * (b[1] - a[1]) + b[0] * (a[1] - m[1]));
    if d.abs() < 1e-9 {
        return dedup(&[a, b], false);
    }
    let sq = |p: Pt| p[0] * p[0] + p[1] * p[1];
    let cx = (sq(a) * (m[1] - b[1]) + sq(m) * (b[1] - a[1]) + sq(b) * (a[1] - m[1])) / d;
    let cy = (sq(a) * (b[0] - m[0]) + sq(m) * (a[0] - b[0]) + sq(b) * (m[0] - a[0])) / d;
    let c = [cx, cy];
    let r = dist(c, a);
    if !finite(c) || !r.is_finite() || r > 1e7 {
        return dedup(&[a, b], false);
    }
    let angle = |p: Pt| (p[1] - cy).atan2(p[0] - cx);
    let tau = std::f64::consts::TAU;
    let (a0, a1, a2) = (angle(a), angle(m), angle(b));
    let ccw = (a2 - a0).rem_euclid(tau);
    let through = (a1 - a0).rem_euclid(tau);
    let sweep = if through < ccw { ccw } else { ccw - tau };
    let steps = ((sweep.abs() / 5f64.to_radians()).ceil() as usize).clamp(4, 180);
    (0..=steps)
        .map(|i| {
            let t = a0 + sweep * i as f64 / steps as f64;
            [cx + r * t.cos(), cy + r * t.sin()]
        })
        .collect()
}

/// A smooth curve through `points` (Catmull-Rom), as points about 8 per span; closed, it runs
/// back to the first point.
pub fn spline_points(points: &[Pt], closed: bool) -> Vec<Pt> {
    let pts = dedup(points, closed);
    let n = pts.len();
    if n < 3 {
        return pts;
    }
    let get = |i: isize| -> Pt {
        if closed {
            pts.get(i.rem_euclid(n as isize) as usize).copied().unwrap_or_default()
        } else {
            pts.get(i.clamp(0, n as isize - 1) as usize).copied().unwrap_or_default()
        }
    };
    let spans = if closed { n } else { n - 1 };
    let steps = 8;
    let mut out = Vec::with_capacity(spans * steps + 1);
    for i in 0..spans as isize {
        let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
        for k in 0..steps {
            let t = k as f64 / steps as f64;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    if !closed && let Some(last) = pts.last() {
        out.push(*last);
    }
    out.truncate(geometry::MAX_VERTICES);
    out
}

/// A regular polygon with `sides` corners (3–64), centred on `c`, one corner at `corner`.
pub fn regular_polygon(c: Pt, corner: Pt, sides: u32) -> Vec<Pt> {
    let n = sides.clamp(3, 64);
    let r = dist(c, corner);
    if r < 0.5 || !r.is_finite() {
        return Vec::new();
    }
    let a0 = (corner[1] - c[1]).atan2(corner[0] - c[0]);
    (0..n)
        .map(|i| {
            let a = a0 + std::f64::consts::TAU * f64::from(i) / f64::from(n);
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// An ellipse centred on `c` fitting the box with a corner at `corner`, as points (for previews).
pub fn ellipse_points(c: Pt, corner: Pt) -> Vec<Pt> {
    let (rx, ry) = ((corner[0] - c[0]).abs(), (corner[1] - c[1]).abs());
    (0..48)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 48.0;
            [c[0] + rx * a.cos(), c[1] + ry * a.sin()]
        })
        .collect()
}

/// The matrix that mirrors points across the line through `a` and `b`.
pub fn mirror(a: Pt, b: Pt) -> Option<[f64; 6]> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return None;
    }
    let (c, s) = ((dx * dx - dy * dy) / len2, 2.0 * dx * dy / len2);
    // Reflection about a line through the origin, moved to pass through `a`.
    Some([c, s, s, -c, a[0] - c * a[0] - s * a[1], a[1] - s * a[0] + c * a[1]])
}

/// The matrix that scales points by `f` about `base`.
pub fn scaling(base: Pt, f: f64) -> [f64; 6] {
    [f, 0.0, 0.0, f, base[0] - f * base[0], base[1] - f * base[1]]
}

/// A corner made where two open paths meet: their clicked segments are extended or cut to the
/// corner, keeping the side each was clicked on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Corner {
    /// Rounded with this radius (0: a sharp corner).
    Fillet(f64),
    /// Cut this far back from the corner on each side.
    Chamfer(f64),
}

/// Which end of an open path a corner moves: the end of its clicked segment past the corner, and
/// the direction from the corner back toward the click.
fn corner_side(points: &[Pt], click: Pt, x: Pt) -> Option<(bool, Pt)> {
    let lengths = running_lengths(points);
    let (s_click, _) = project(points, click)?;
    let n = points.len();
    // The segment clicked.
    let i = lengths.windows(2).position(|w| w.get(1).is_some_and(|l| s_click <= *l + 1e-9)).unwrap_or(0);
    let (a, b) = (*points.get(i)?, *points.get(i + 1)?);
    let along = |p: Pt| (p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1]);
    let at_end = along(x) > along(point_along(points, &lengths, s_click)?);
    // Only a path's own end can move.
    if (at_end && i + 2 != n) || (!at_end && i != 0) {
        return None;
    }
    let keep = point_along(points, &lengths, s_click)?;
    let d = dist(keep, x);
    if d < 1e-9 {
        return None;
    }
    Some((at_end, [(keep[0] - x[0]) / d, (keep[1] - x[1]) / d]))
}

fn set_end(points: &[Pt], at_end: bool, p: Pt) -> Vec<Pt> {
    let mut out = points.to_vec();
    if let Some(slot) = if at_end { out.last_mut() } else { out.first_mut() } {
        *slot = p;
    }
    out
}

/// The two paths with their ends meeting at a corner, and the arc or chamfer between them.
pub fn corner(p1: &[Pt], c1: Pt, p2: &[Pt], c2: Pt, kind: Corner) -> Option<(Vec<Pt>, Vec<Pt>, Option<Vec<Pt>>)> {
    let seg = |p: &[Pt], c: Pt| -> Option<(Pt, Pt)> {
        let lengths = running_lengths(p);
        let (s, _) = project(p, c)?;
        let i = lengths.windows(2).position(|w| w.get(1).is_some_and(|l| s <= *l + 1e-9)).unwrap_or(0);
        Some((*p.get(i)?, *p.get(i + 1)?))
    };
    let (a0, a1) = seg(p1, c1)?;
    let (b0, b1) = seg(p2, c2)?;
    let (t, _) = intersect(a0, a1, b0, b1)?;
    let x = lerp(a0, a1, t);
    if !finite(x) {
        return None;
    }
    let (end1, u1) = corner_side(p1, c1, x)?;
    let (end2, u2) = corner_side(p2, c2, x)?;
    let cos = (u1[0] * u2[0] + u1[1] * u2[1]).clamp(-1.0, 1.0);
    let theta = cos.acos();
    match kind {
        Corner::Fillet(r) if r <= 1e-9 => Some((set_end(p1, end1, x), set_end(p2, end2, x), None)),
        Corner::Fillet(r) => {
            if !(1e-3..std::f64::consts::PI - 1e-3).contains(&theta) {
                return None;
            }
            let d = r / (theta / 2.0).tan();
            let t1 = [x[0] + u1[0] * d, x[1] + u1[1] * d];
            let t2 = [x[0] + u2[0] * d, x[1] + u2[1] * d];
            let bis = [u1[0] + u2[0], u1[1] + u2[1]];
            let bl = bis[0].hypot(bis[1]);
            if bl < 1e-9 {
                return None;
            }
            let h = r / (theta / 2.0).sin();
            let c = [x[0] + bis[0] / bl * h, x[1] + bis[1] / bl * h];
            let a_start = (t1[1] - c[1]).atan2(t1[0] - c[0]);
            let a_end = (t2[1] - c[1]).atan2(t2[0] - c[0]);
            let mut sweep = (a_end - a_start).rem_euclid(std::f64::consts::TAU);
            if sweep > std::f64::consts::PI {
                sweep -= std::f64::consts::TAU;
            }
            let steps = ((sweep.abs() / 5f64.to_radians()).ceil() as usize).clamp(2, 72);
            let arc: Vec<Pt> = (0..=steps)
                .map(|k| {
                    let a = a_start + sweep * k as f64 / steps as f64;
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                })
                .collect();
            let all = [t1, t2, c];
            all.iter().all(|p| finite(*p)).then(|| (set_end(p1, end1, t1), set_end(p2, end2, t2), Some(arc)))
        }
        Corner::Chamfer(d) => {
            let t1 = [x[0] + u1[0] * d, x[1] + u1[1] * d];
            let t2 = [x[0] + u2[0] * d, x[1] + u2[1] * d];
            (finite(t1) && finite(t2) && d > 0.0).then(|| (set_end(p1, end1, t1), set_end(p2, end2, t2), Some(vec![t1, t2])))
        }
    }
}

/// Two open paths that share an end, as one (`None` when no ends meet within `tolerance`).
pub fn join(p1: &[Pt], p2: &[Pt], tolerance: f64) -> Option<Vec<Pt>> {
    let (s1, e1) = (*p1.first()?, *p1.last()?);
    let (s2, e2) = (*p2.first()?, *p2.last()?);
    let rev = |p: &[Pt]| p.iter().rev().copied().collect::<Vec<_>>();
    let joined = if dist(e1, s2) <= tolerance {
        [p1.to_vec(), p2.get(1..)?.to_vec()].concat()
    } else if dist(e1, e2) <= tolerance {
        [p1.to_vec(), rev(p2).get(1..)?.to_vec()].concat()
    } else if dist(s1, e2) <= tolerance {
        [p2.to_vec(), p1.get(1..)?.to_vec()].concat()
    } else if dist(s1, s2) <= tolerance {
        [rev(p1), p2.get(1..)?.to_vec()].concat()
    } else {
        return None;
    };
    (joined.len() <= geometry::MAX_VERTICES).then_some(joined)
}

/// An open path split at `at` along it (`None` when that is at an end).
pub fn split(points: &[Pt], at: f64) -> Option<(Vec<Pt>, Vec<Pt>)> {
    let lengths = running_lengths(points);
    let total = lengths.last().copied().unwrap_or(0.0);
    if at < 1.0 || at > total - 1.0 {
        return None;
    }
    Some((piece(points, &lengths, 0.0, at), piece(points, &lengths, at, total)))
}

/// Which side of an open or closed path `p` is on: 1 for the left, −1 for the right.
fn side(points: &[Pt], closed: bool, p: Pt) -> f64 {
    let mut best: Option<(f64, f64)> = None;
    let n = points.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segments {
        let (Some(a), Some(b)) = (points.get(i), points.get((i + 1) % n.max(1))) else { continue };
        let (d, _) = to_segment(p, *a, *b);
        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, cross));
        }
    }
    if best.is_some_and(|(_, c)| c < 0.0) { -1.0 } else { 1.0 }
}

// ---- the page ----------------------------------------------------------------------------------

/// The lines, polylines and polygons on `page`, read once per edit.
fn shapes(view: &mut DocView, doc: &Document, page: usize) -> Vec<Geometry> {
    let generation = doc.edit_generation();
    if view.draft.geometry.as_ref().is_none_or(|(p, g, _)| *p != page || *g != generation) {
        view.draft.geometry = Some((page, generation, doc.markup_geometry(page)));
    }
    view.draft.geometry.as_ref().map(|(_, _, g)| g.clone()).unwrap_or_default()
}

/// Every segment of the shapes except `skip`.
fn segments_except(shapes: &[Geometry], skip: Option<usize>) -> Vec<(Pt, Pt)> {
    shapes.iter().filter(|g| Some(g.index) != skip).flat_map(|g| g.segments().collect::<Vec<_>>()).collect()
}

/// The shape nearest `p` within `tolerance`.
fn nearest(shapes: &[Geometry], p: Pt, tolerance: f64) -> Option<&Geometry> {
    let mut best: Option<(&Geometry, f64)> = None;
    for g in shapes {
        for (a, b) in g.segments() {
            let (d, _) = to_segment(p, a, b);
            if d <= tolerance && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((g, d));
            }
        }
    }
    best.map(|(g, _)| g)
}

/// The markup to act on under the pointer: a line or shape near it, else any comment there.
fn pick(cx: &PageCx<'_>, shapes: &[Geometry], pos: Pos2, p: Pt, tolerance: f64) -> Option<usize> {
    nearest(shapes, p, tolerance).map(|g| g.index).or_else(|| cx.hit(pos).filter(|a| a.subtype != "Popup").map(|a| a.index))
}

/// A markup's outline to show while moving it: its points, or its rectangle's corners.
fn outline(cx: &PageCx<'_>, shapes: &[Geometry], index: usize) -> Option<(Vec<Pt>, bool)> {
    if let Some(g) = shapes.iter().find(|g| g.index == index) {
        return Some((g.points.clone(), g.kind == Kind::Polygon));
    }
    let a = cx.get(index)?;
    let [x0, y0, x1, y1] = a.rect.map(f64::from);
    Some((vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], true))
}

fn draw_path(painter: &egui::Painter, cx: &PageCx<'_>, points: &[Pt], closed: bool, stroke: Stroke) {
    let screen: Vec<Pos2> = points.iter().map(|p| cx.to_screen(*p)).collect();
    if closed && screen.len() > 2 {
        painter.add(egui::Shape::closed_line(screen, stroke));
    } else {
        painter.add(egui::Shape::line(screen, stroke));
    }
}

/// A small dark label near the pointer.
pub(crate) fn chip(painter: &egui::Painter, at: Pos2, text: &str) {
    let galley = painter.layout_no_wrap(text.to_string(), theme::semibold(12.0), Color32::WHITE);
    let r = egui::Rect::from_min_size(at, galley.size()).expand(4.0);
    painter.rect_filled(r, egui::CornerRadius::same(4), Color32::from_black_alpha(190));
    painter.galley(at, galley, Color32::WHITE);
}

fn translation(d: Pt) -> [f64; 6] {
    [1.0, 0.0, 0.0, 1.0, d[0], d[1]]
}

/// The edit for a typed or clicked second point of Move, Copy or Rotate.
fn place(tool: DraftTool, page: usize, index: usize, base: Pt, to: Pt) -> Option<Edit> {
    match tool {
        DraftTool::Move | DraftTool::Copy => {
            let d = sub(to, base);
            (d[0].hypot(d[1]) > 1e-9).then(|| Edit::TransformAnnotation { page, index, matrix: translation(d), copy: tool == DraftTool::Copy })
        }
        DraftTool::Rotate => {
            let d = sub(to, base);
            (d[0].hypot(d[1]) > 1e-9).then(|| rotate(page, index, base, d[1].atan2(d[0]).to_degrees()))
        }
        _ => None,
    }
}

/// Mirror across the line `a`–`b`, keeping the original (a copy) or flipping it.
fn mirrored(page: usize, index: usize, a: Pt, b: Pt, keep: bool) -> Option<Edit> {
    mirror(a, b).map(|matrix| Edit::TransformAnnotation { page, index, matrix, copy: keep })
}

/// Copies of the markup at `index` for a rectangular array (the original is the first cell).
fn rect_array(page: usize, index: usize, settings: &DraftSettings, k: f64) -> Vec<Edit> {
    let (rows, cols) = (settings.array_rows.clamp(1, 50), settings.array_columns.clamp(1, 50));
    let mut edits = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if (r, c) == (0, 0) || edits.len() >= 400 {
                continue;
            }
            let d = [f64::from(c) * settings.array_column_inches * k, f64::from(r) * settings.array_row_inches * k];
            edits.push(Edit::TransformAnnotation { page, index, matrix: translation(d), copy: true });
        }
    }
    edits
}

/// Copies of the markup at `index` turned about `center` for a polar array.
fn polar_array(page: usize, index: usize, center: Pt, settings: &DraftSettings) -> Vec<Edit> {
    let n = settings.polar_count.clamp(2, 360);
    let angle = settings.polar_angle.clamp(-360.0, 360.0);
    let full = (angle.abs() - 360.0).abs() < 1e-9;
    let step = if full { angle / f64::from(n) } else { angle / f64::from(n - 1) };
    (1..n).map(|k| Edit::TransformAnnotation { page, index, matrix: geometry::rotation(center, step * f64::from(k)), copy: true }).collect()
}

/// A dimension across a circle or oval at `p`: its radius or diameter toward `p`.
fn circle_dimension(cx: &PageCx<'_>, p: Pt, tolerance: f64, diameter: bool) -> Option<(usize, Pt, Pt)> {
    let mut best: Option<(usize, Pt, Pt, f64)> = None;
    for a in cx.info.annotations.iter().filter(|a| a.page == cx.page && a.subtype == "Circle" && a.in_reply_to.is_none()) {
        let [x0, y0, x1, y1] = a.rect.map(f64::from);
        let c = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let (rx, ry) = ((x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
        if rx < 0.5 || ry < 0.5 {
            continue;
        }
        let d = dist(p, c);
        let dir = if d < 1e-9 { [1.0, 0.0] } else { [(p[0] - c[0]) / d, (p[1] - c[1]) / d] };
        // The radius toward `p`.
        let r = rx * ry / ((ry * dir[0]).hypot(rx * dir[1])).max(1e-9);
        let off = (d - r).abs();
        if (off <= tolerance * 2.0 || d < r) && best.as_ref().is_none_or(|b| off < b.3) {
            let edge = [c[0] + dir[0] * r, c[1] + dir[1] * r];
            let start = if diameter { [c[0] - dir[0] * r, c[1] - dir[1] * r] } else { c };
            best = Some((a.index, start, edge, off));
        }
    }
    best.map(|(i, a, b, _)| (i, a, b))
}

fn rotate(page: usize, index: usize, center: Pt, degrees: f64) -> Edit {
    Edit::TransformAnnotation { page, index, matrix: geometry::rotation(center, degrees), copy: false }
}

/// After Move or Rotate the next one starts with a new base point; Copy keeps its base for more
/// copies.
fn after_place(view: &mut DocView, tool: DraftTool) {
    if tool != DraftTool::Copy {
        view.draft.base = None;
    }
}

/// The selected markup on `page`, the base point set on it, and the page they are on.
fn target(view: &DocView, page: usize) -> (Option<usize>, Option<Pt>) {
    let index = view.comments.selected.filter(|(p, _)| *p == page).map(|(_, i)| i);
    let base = view.draft.base.filter(|_| view.draft.page == Some(page) && index.is_some());
    (index, base)
}

/// The modify tools on one page. `point` is the snapped pointer, `pos` the pointer on screen.
#[allow(clippy::too_many_arguments)]
pub(crate) fn modify_input(
    ui: &egui::Ui,
    resp: &egui::Response,
    doc: &Document,
    view: &mut DocView,
    cx: &PageCx<'_>,
    tool: DraftTool,
    settings: &DraftSettings,
    pos: Pos2,
    point: Pt,
    tolerance: f64,
) {
    let page = cx.page;
    let accent = Color32::from_rgb(242, 163, 58);
    let stroke = Stroke::new(1.5, accent);
    let ghost = Stroke::new(1.0, accent.gamma_multiply(0.55));
    let all = shapes(view, doc, page);
    let raw = cx.to_user(pos);
    let painter = ui.painter();
    let hint_at = pos + vec2(14.0, 14.0);
    let clicked = resp.clicked();
    match tool {
        DraftTool::Move | DraftTool::Copy | DraftTool::Rotate | DraftTool::Mirror | DraftTool::Scale | DraftTool::ArrayPolar => {
            let (index, base) = target(view, page);
            let shift = ui.input(|i| i.modifiers.shift);
            let point = match base {
                Some(b) if settings.ortho != shift && matches!(tool, DraftTool::Move | DraftTool::Copy | DraftTool::Mirror) => {
                    crate::draft::orthogonal(b, point)
                }
                _ => point,
            };
            match (index, base) {
                (None, _) => {
                    if let Some(i) = pick(cx, &all, pos, raw, tolerance)
                        && let Some((pts, closed)) = outline(cx, &all, i)
                    {
                        draw_path(painter, cx, &pts, closed, ghost);
                        if clicked {
                            view.comments.selected = Some((page, i));
                            view.draft.base = None;
                        }
                    }
                    chip(painter, hint_at, tl!("Pick the markup"));
                }
                (Some(i), None) if tool == DraftTool::ArrayPolar => {
                    if let Some((pts, closed)) = outline(cx, &all, i) {
                        draw_path(painter, cx, &pts, closed, stroke);
                        for e in polar_array(page, i, point, settings) {
                            if let Edit::TransformAnnotation { matrix, .. } = e {
                                let moved: Vec<Pt> = pts.iter().map(|p| geometry::apply(&matrix, *p)).collect();
                                draw_path(painter, cx, &moved, closed, ghost);
                            }
                        }
                    }
                    painter.circle_stroke(cx.to_screen(point), 4.0, stroke);
                    chip(painter, hint_at, tl!("Click the centre of the array"));
                    if clicked {
                        let edits = polar_array(page, i, point, settings);
                        if !edits.is_empty() {
                            view.pending_edit = Some(Edit::Batch { label: "Polar array".into(), edits });
                        }
                    }
                }
                (Some(i), None) => {
                    if let Some((pts, closed)) = outline(cx, &all, i) {
                        draw_path(painter, cx, &pts, closed, stroke);
                    }
                    painter.circle_stroke(cx.to_screen(point), 4.0, stroke);
                    let hint = match tool {
                        DraftTool::Rotate => tl!("Click the centre to turn about"),
                        DraftTool::Mirror => tl!("Click the first point of the mirror line"),
                        _ => tl!("Click the base point"),
                    };
                    chip(painter, hint_at, hint);
                    if clicked {
                        view.draft.base = Some(point);
                        view.draft.page = Some(page);
                    }
                }
                (Some(i), Some(b)) if tool == DraftTool::Scale => {
                    if let Some((pts, closed)) = outline(cx, &all, i) {
                        draw_path(painter, cx, &pts, closed, stroke);
                    }
                    painter.circle_filled(cx.to_screen(b), 3.0, accent);
                    chip(painter, hint_at, tl!("Type the scale factor, then Enter (2 doubles, 0.5 halves)"));
                }
                (Some(i), Some(b)) if tool == DraftTool::Mirror => {
                    if let (Some((pts, closed)), Some(m)) = (outline(cx, &all, i), mirror(b, point)) {
                        if settings.mirror_keep {
                            draw_path(painter, cx, &pts, closed, stroke);
                        } else {
                            draw_path(painter, cx, &pts, closed, ghost);
                        }
                        let moved: Vec<Pt> = pts.iter().map(|p| geometry::apply(&m, *p)).collect();
                        draw_path(painter, cx, &moved, closed, stroke);
                    }
                    painter.extend(egui::Shape::dashed_line(&[cx.to_screen(b), cx.to_screen(point)], Stroke::new(1.0, accent), 5.0, 4.0));
                    chip(painter, hint_at, tl!("Click the second point of the mirror line"));
                    if clicked && let Some(edit) = mirrored(page, i, b, point, settings.mirror_keep) {
                        view.pending_edit = Some(edit);
                        view.draft.base = None;
                    }
                }
                (Some(i), Some(b)) => {
                    let matrix = if tool == DraftTool::Rotate {
                        let d = sub(point, b);
                        geometry::rotation(b, d[1].atan2(d[0]).to_degrees())
                    } else {
                        translation(sub(point, b))
                    };
                    if let Some((pts, closed)) = outline(cx, &all, i) {
                        draw_path(painter, cx, &pts, closed, ghost);
                        let moved: Vec<Pt> = pts.iter().map(|p| geometry::apply(&matrix, *p)).collect();
                        draw_path(painter, cx, &moved, closed, stroke);
                    }
                    painter.line_segment([cx.to_screen(b), cx.to_screen(point)], Stroke::new(1.0, accent.gamma_multiply(0.7)));
                    painter.circle_filled(cx.to_screen(b), 3.0, accent);
                    let d = sub(point, b);
                    let label = if tool == DraftTool::Rotate {
                        format!("{:.1}°", d[1].atan2(d[0]).to_degrees())
                    } else {
                        doc.measurement_scale(page, b)
                            .ok()
                            .and_then(|s| s.format_length(d[0].hypot(d[1]) * s.x * s.distance_factor).ok())
                            .unwrap_or_default()
                    };
                    chip(painter, hint_at, &label);
                    if clicked && let Some(edit) = place(tool, page, i, b, point) {
                        view.pending_edit = Some(edit);
                        after_place(view, tool);
                    }
                }
            }
        }
        DraftTool::Offset => {
            let index = view.comments.selected.filter(|(p, _)| *p == page).map(|(_, i)| i);
            let selected = index.and_then(|i| all.iter().find(|g| g.index == i));
            match selected {
                None => {
                    if let Some(g) = nearest(&all, raw, tolerance) {
                        draw_path(painter, cx, &g.points, g.kind == Kind::Polygon, ghost);
                        if clicked {
                            view.comments.selected = Some((page, g.index));
                        }
                    }
                    chip(painter, hint_at, tl!("Pick the line or shape to offset"));
                }
                Some(g) => {
                    let closed = g.kind == Kind::Polygon;
                    let k = crate::draft::units_per_inch(doc, page, g.points.first().copied().unwrap_or(raw));
                    let d = settings.offset_inches * k * side(&g.points, closed, raw);
                    let copy = offset_path(&g.points, d, closed);
                    draw_path(painter, cx, &g.points, closed, stroke);
                    draw_path(painter, cx, &copy, closed, stroke);
                    chip(painter, hint_at, tl!("Click the side to offset to"));
                    if clicked && copy.len() >= 2 && copy.iter().all(|p| finite(*p)) {
                        let copy = if g.kind == Kind::Line { copy.into_iter().take(2).collect() } else { copy };
                        view.pending_edit = Some(Edit::SetVertices { page, index: g.index, vertices: copy, copy: true });
                        view.comments.selected = None;
                    }
                }
            }
        }
        DraftTool::Trim | DraftTool::Extend => {
            let open = all.iter().filter(|g| g.kind != Kind::Polygon).cloned().collect::<Vec<_>>();
            let Some(g) = nearest(&open, raw, tolerance) else {
                let hint =
                    if tool == DraftTool::Trim { tl!("Click the part of a line to cut away") } else { tl!("Click near the end of a line to extend") };
                chip(painter, hint_at, hint);
                return;
            };
            let others = segments_except(&all, Some(g.index));
            let Some((at, _)) = project(&g.points, raw) else { return };
            let red = Stroke::new(2.5, Color32::from_rgb(220, 60, 60));
            if tool == DraftTool::Trim {
                match trim(&g.points, &others, at) {
                    Some(pieces) => {
                        draw_path(painter, cx, &g.points, false, red);
                        for p in &pieces {
                            draw_path(painter, cx, p, false, stroke);
                        }
                        if clicked && !g.locked {
                            view.pending_edit = Some(trim_edit(page, g.index, pieces));
                        }
                    }
                    None => chip(painter, hint_at, tl!("Nothing crosses this line to trim to")),
                }
            } else {
                let total = running_lengths(&g.points).last().copied().unwrap_or(0.0);
                let at_end = at > total / 2.0;
                match extend(&g.points, at_end, &others) {
                    Some(to) => {
                        let from = if at_end { g.points.last() } else { g.points.first() };
                        if let Some(from) = from {
                            painter.line_segment([cx.to_screen(*from), cx.to_screen(to)], stroke);
                        }
                        if clicked && !g.locked {
                            let mut pts = g.points.clone();
                            let slot = if at_end { pts.last_mut() } else { pts.first_mut() };
                            if let Some(slot) = slot {
                                *slot = to;
                            }
                            view.pending_edit = Some(Edit::SetVertices { page, index: g.index, vertices: pts, copy: false });
                        }
                    }
                    None => chip(painter, hint_at, tl!("Nothing ahead of this end to extend to")),
                }
            }
        }
        DraftTool::ArrayRect => match pick(cx, &all, pos, raw, tolerance) {
            Some(i) => {
                let k = crate::draft::units_per_inch(doc, page, raw);
                let edits = rect_array(page, i, settings, k);
                if let Some((pts, closed)) = outline(cx, &all, i) {
                    draw_path(painter, cx, &pts, closed, stroke);
                    for e in &edits {
                        if let Edit::TransformAnnotation { matrix, .. } = e {
                            let moved: Vec<Pt> = pts.iter().map(|p| geometry::apply(matrix, *p)).collect();
                            draw_path(painter, cx, &moved, closed, ghost);
                        }
                    }
                }
                if clicked && !edits.is_empty() {
                    view.pending_edit = Some(Edit::Batch { label: "Rectangular array".into(), edits });
                }
            }
            None => chip(painter, hint_at, tl!("Pick the markup to repeat")),
        },
        DraftTool::Fillet | DraftTool::Chamfer | DraftTool::Join => {
            let open = all.iter().filter(|g| g.kind != Kind::Polygon).cloned().collect::<Vec<_>>();
            let first = view
                .comments
                .selected
                .filter(|(p, _)| *p == page)
                .and_then(|(_, i)| open.iter().find(|g| g.index == i))
                .zip(view.draft.base.filter(|_| view.draft.page == Some(page)));
            let hover = nearest(&open, raw, tolerance).filter(|g| first.is_none_or(|(f, _)| f.index != g.index));
            match (first, hover) {
                (None, Some(g)) => {
                    draw_path(painter, cx, &g.points, false, ghost);
                    chip(painter, hint_at, tl!("Pick the first line"));
                    if clicked {
                        view.comments.selected = Some((page, g.index));
                        view.draft.base = Some(raw);
                        view.draft.page = Some(page);
                    }
                }
                (None, None) => chip(painter, hint_at, tl!("Pick the first line")),
                (Some((f, _)), None) => {
                    draw_path(painter, cx, &f.points, false, stroke);
                    chip(painter, hint_at, tl!("Pick the second line"));
                }
                (Some((f, c1)), Some(g)) => {
                    draw_path(painter, cx, &f.points, false, stroke);
                    let k = crate::draft::units_per_inch(doc, page, raw);
                    let result = match tool {
                        DraftTool::Join => join(&f.points, &g.points, tolerance.max(1e-3)).map(|merged| {
                            draw_path(painter, cx, &merged, false, stroke);
                            Edit::Batch {
                                label: "Join".into(),
                                edits: vec![
                                    Edit::SetVertices { page, index: f.index, vertices: merged, copy: false },
                                    Edit::DeleteAnnotation { page, index: g.index },
                                ],
                            }
                        }),
                        _ => {
                            let kind = if tool == DraftTool::Fillet {
                                Corner::Fillet(settings.fillet_inches * k)
                            } else {
                                Corner::Chamfer(settings.chamfer_inches * k)
                            };
                            corner(&f.points, c1, &g.points, raw, kind).map(|(p1, p2, extra)| {
                                draw_path(painter, cx, &p1, false, stroke);
                                draw_path(painter, cx, &p2, false, stroke);
                                let mut edits = vec![
                                    Edit::SetVertices { page, index: f.index, vertices: p1, copy: false },
                                    Edit::SetVertices { page, index: g.index, vertices: p2, copy: false },
                                ];
                                if let Some(extra) = extra {
                                    draw_path(painter, cx, &extra, false, stroke);
                                    edits.push(Edit::SetVertices { page, index: f.index, vertices: extra, copy: true });
                                }
                                Edit::Batch { label: if tool == DraftTool::Fillet { "Fillet" } else { "Chamfer" }.into(), edits }
                            })
                        }
                    };
                    match result {
                        Some(edit) => {
                            if clicked {
                                view.pending_edit = Some(edit);
                                view.comments.selected = None;
                                view.draft.base = None;
                            }
                        }
                        None => {
                            let why = if tool == DraftTool::Join {
                                tl!("These lines don't meet end to end")
                            } else {
                                tl!("These lines don't make a corner at their ends")
                            };
                            chip(painter, hint_at, why);
                        }
                    }
                }
            }
        }
        DraftTool::Break => {
            let open = all.iter().filter(|g| g.kind != Kind::Polygon).cloned().collect::<Vec<_>>();
            let Some(g) = nearest(&open, raw, tolerance) else {
                chip(painter, hint_at, tl!("Click a line where it should split"));
                return;
            };
            let Some((at, _)) = project(&g.points, raw) else { return };
            match split(&g.points, at) {
                Some((a, b)) => {
                    draw_path(painter, cx, &a, false, stroke);
                    draw_path(painter, cx, &b, false, Stroke::new(1.5, Color32::from_rgb(60, 150, 230)));
                    if let Some(p) = a.last() {
                        painter.circle_filled(cx.to_screen(*p), 3.5, accent);
                    }
                    if clicked && !g.locked {
                        view.pending_edit = Some(trim_edit(page, g.index, vec![a, b]));
                    }
                }
                None => chip(painter, hint_at, tl!("Click away from the ends")),
            }
        }
        DraftTool::Explode => {
            let Some(g) = nearest(&all, raw, tolerance).filter(|g| g.kind != Kind::Line) else {
                chip(painter, hint_at, tl!("Pick a polyline or shape to explode"));
                return;
            };
            let segments: Vec<(Pt, Pt)> = g.segments().take(500).collect();
            for (a, b) in &segments {
                painter.line_segment([cx.to_screen(*a), cx.to_screen(*b)], stroke);
                painter.circle_filled(cx.to_screen(*a), 2.5, accent);
            }
            if clicked && !g.locked && segments.len() >= 2 {
                let mut edits: Vec<Edit> =
                    segments.iter().map(|(a, b)| Edit::SetVertices { page, index: g.index, vertices: vec![*a, *b], copy: true }).collect();
                edits.push(Edit::DeleteAnnotation { page, index: g.index });
                view.pending_edit = Some(Edit::Batch { label: "Explode".into(), edits });
            }
        }
        DraftTool::Erase => match pick(cx, &all, pos, raw, tolerance) {
            Some(i) => {
                if let Some((pts, closed)) = outline(cx, &all, i) {
                    draw_path(painter, cx, &pts, closed, Stroke::new(2.5, Color32::from_rgb(220, 60, 60)));
                }
                if clicked {
                    view.pending_edit = Some(Edit::DeleteAnnotation { page, index: i });
                    if view.comments.selected == Some((page, i)) {
                        view.comments.selected = None;
                    }
                }
            }
            None => chip(painter, hint_at, tl!("Click the markup to delete")),
        },
        DraftTool::DimRadius | DraftTool::DimDiameter => {
            let diameter = tool == DraftTool::DimDiameter;
            match circle_dimension(cx, raw, tolerance, diameter) {
                Some((_, a, b)) => {
                    painter.line_segment([cx.to_screen(a), cx.to_screen(b)], stroke);
                    painter.circle_filled(cx.to_screen(b), 3.0, accent);
                    if clicked && let Some(edit) = crate::draft::dimension(page, a, b, doc, crate::draft::layer_color(doc), &settings.author) {
                        view.pending_edit = Some(edit);
                    }
                }
                None => chip(painter, hint_at, tl!("Click a circle")),
            }
        }
        _ => {}
    }
}

/// What a trim leaves: the first piece in place of the line, the second as a copy; nothing at all
/// deletes it.
fn trim_edit(page: usize, index: usize, pieces: Vec<Vec<Pt>>) -> Edit {
    let mut it = pieces.into_iter();
    let Some(first) = it.next() else { return Edit::DeleteAnnotation { page, index } };
    let mut edits = vec![Edit::SetVertices { page, index, vertices: first, copy: false }];
    edits.extend(it.map(|p| Edit::SetVertices { page, index, vertices: p, copy: true }));
    match edits.len() {
        1 => edits.pop().unwrap_or(Edit::DeleteAnnotation { page, index }),
        _ => Edit::Batch { label: "Trim".into(), edits },
    }
}

/// A typed entry with a modify tool: the base point, then the second point (Move, Copy) or the
/// angle in degrees (Rotate); a length sets the Offset distance.
pub(crate) fn submit(view: &mut DocView, doc: &Document, tool: DraftTool, settings: &DraftSettings, text: &str) -> Option<Edit> {
    let page = view.cursor.map(|c| c.0).or(view.draft.page).unwrap_or(view.current);
    let (index, base) = target(view, page);
    if matches!(tool, DraftTool::Offset | DraftTool::Fillet | DraftTool::Chamfer) {
        let floor = if tool == DraftTool::Fillet { 0.0 } else { 1e-6 };
        match crate::draft::parse_length(text, settings.bare).filter(|l| *l >= floor && *l < 1e6) {
            Some(len) => view.draft.typed_setting = Some((tool, len)),
            None => view.draft.error = Some(crate::i18n::fmt(tl!("Couldn't read “{text}” as a distance."), &[("text", text)])),
        }
        return None;
    }
    let Some(index) = index else {
        view.draft.error = Some(tl!("Pick the markup first.").to_string());
        return None;
    };
    if let (DraftTool::Scale, Some(b)) = (tool, base) {
        let factor = text.trim().trim_start_matches(['x', '×']).trim().parse::<f64>().ok().filter(|f| f.is_finite() && *f > 1e-4 && *f < 1e4);
        return match factor {
            Some(f) => {
                view.draft.base = None;
                Some(Edit::TransformAnnotation { page, index, matrix: scaling(b, f), copy: false })
            }
            None => {
                view.draft.error = Some(crate::i18n::fmt(tl!("Couldn't read “{text}” as a scale factor."), &[("text", text)]));
                None
            }
        };
    }
    if !matches!(tool, DraftTool::Move | DraftTool::Copy | DraftTool::Rotate | DraftTool::Mirror | DraftTool::Scale) {
        view.draft.error = Some(tl!("Click the line on the drawing.").to_string());
        return None;
    }
    let origin = crate::draft::origin(view, doc, page);
    if let (DraftTool::Rotate, Some(center)) = (tool, base) {
        let degrees =
            text.trim().trim_start_matches('<').trim_end_matches('°').trim().parse::<f64>().ok().filter(|d| d.is_finite() && d.abs() <= 3600.0);
        return match degrees {
            Some(d) => {
                view.draft.base = None;
                Some(rotate(page, index, center, d))
            }
            None => {
                view.draft.error = Some(crate::i18n::fmt(tl!("Couldn't read “{text}” as an angle in degrees."), &[("text", text)]));
                None
            }
        };
    }
    let Some(entry) = crate::draft::parse_entry(text, settings.bare) else {
        view.draft.error = Some(crate::i18n::fmt(tl!("Couldn't read “{text}”. Try 15', 12'6\", @0,15 or @15<90."), &[("text", text)]));
        return None;
    };
    let k = crate::draft::units_per_inch(doc, page, base.unwrap_or(origin));
    let toward = view.cursor.filter(|c| c.0 == page).map(|c| c.1);
    let Some(p) = crate::draft::entry_point(entry, base, origin, toward, k, settings.ortho) else {
        view.draft.error = Some(tl!("Give coordinates for the base point.").to_string());
        return None;
    };
    match base {
        None => {
            view.draft.base = Some(p);
            view.draft.page = Some(page);
            None
        }
        Some(b) if tool == DraftTool::Mirror => {
            view.draft.base = None;
            mirrored(page, index, b, p, settings.mirror_keep)
        }
        Some(b) => {
            let edit = place(tool, page, index, b, p);
            after_place(view, tool);
            edit
        }
    }
}

/// Escape with a modify tool steps back: the base point first, then the selection.
pub(crate) fn step_back(view: &mut DocView) {
    if view.draft.base.take().is_none() {
        view.comments.selected = None;
    }
}

// ---- grips -------------------------------------------------------------------------------------

/// A vertex being dragged.
#[derive(Clone, Debug)]
pub struct Grip {
    pub page: usize,
    pub index: usize,
    pub vertex: usize,
    pub kind: Kind,
    pub start: Vec<Pt>,
    pub points: Vec<Pt>,
}

/// Grips on the selected line, polyline or polygon with the Select tool: a square on each point,
/// dragged to move it (snapping to other markup, ortho from its neighbour). Returns whether the
/// pointer is busy with a grip, so the comment tools leave it alone.
pub(crate) fn grips(ui: &egui::Ui, doc: &Document, view: &mut DocView, cx: &PageCx<'_>, ortho: bool) -> bool {
    let page = cx.page;
    if !cx.allowed || cx.hidden || !doc.allows_annotation() {
        return false;
    }
    let active = view.draft.grip.as_ref().is_some_and(|g| g.page == page);
    let selected = view.comments.selected.filter(|(p, _)| *p == page).map(|(_, i)| i);
    if selected.is_none() && !active {
        return false;
    }
    let all = shapes(view, doc, page);
    let accent = Color32::from_rgb(242, 163, 58);
    let painter = ui.painter();
    let (pointer, pressed, down) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.primary_pressed(), i.pointer.primary_down()));
    if active {
        let Some(mut grip) = view.draft.grip.take() else { return false };
        if down {
            if let Some(pos) = pointer {
                let mut p = cx.to_user(pos);
                let pixels_per_unit = f64::from((cx.xf.rect.width() / cx.xf.pw).max(1e-6));
                let tolerance = crate::draft::SNAP_PX / pixels_per_unit;
                let own = grip.start.get(grip.vertex).copied();
                let mut best: Option<(Pt, f64)> = None;
                for v in doc.markup_vertices(page).into_iter().filter(|v| own.is_none_or(|o| dist(*v, o) > 1e-6)) {
                    let d = dist(v, p);
                    if d <= tolerance && best.is_none_or(|(_, b)| d < b) {
                        best = Some((v, d));
                    }
                }
                if let Some((v, _)) = best {
                    p = v;
                } else if ortho != ui.input(|i| i.modifiers.shift) {
                    let neighbour = grip.vertex.checked_sub(1).or(Some(grip.vertex.saturating_add(1))).and_then(|j| grip.points.get(j)).copied();
                    if let Some(n) = neighbour {
                        p = crate::draft::orthogonal(n, p);
                    }
                }
                if let Some(slot) = grip.points.get_mut(grip.vertex) {
                    *slot = p;
                }
            }
            let closed = grip.kind == Kind::Polygon;
            draw_path(painter, cx, &grip.start, closed, Stroke::new(1.0, accent.gamma_multiply(0.5)));
            draw_path(painter, cx, &grip.points, closed, Stroke::new(1.5, accent));
            if let Some(v) = grip.points.get(grip.vertex) {
                painter.rect_filled(egui::Rect::from_center_size(cx.to_screen(*v), egui::Vec2::splat(GRIP_PX)), 1.0, accent);
            }
            view.draft.grip = Some(grip);
        } else if grip.points != grip.start {
            view.pending_edit = Some(Edit::SetVertices { page, index: grip.index, vertices: grip.points, copy: false });
        }
        return true;
    }
    let Some(index) = selected else { return false };
    let Some(g) = all.iter().find(|g| g.index == index) else { return false };
    if g.locked {
        return false;
    }
    let near = |pos: Pos2| g.points.iter().position(|v| cx.to_screen(*v).distance(pos) <= GRIP_PX);
    for v in &g.points {
        let r = egui::Rect::from_center_size(cx.to_screen(*v), egui::Vec2::splat(GRIP_PX));
        painter.rect_filled(r, 1.0, Color32::WHITE);
        painter.rect_stroke(r, 1.0, Stroke::new(1.0, accent), egui::StrokeKind::Middle);
    }
    let hovered = pointer.and_then(near);
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if pressed && let Some(vertex) = hovered {
        view.draft.grip = Some(Grip { page, index, vertex, kind: g.kind, start: g.points.clone(), points: g.points.clone() });
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cross() {
        let (t, u) = intersect([0.0, 0.0], [10.0, 0.0], [5.0, -5.0], [5.0, 5.0]).unwrap();
        assert!((t - 0.5).abs() < 1e-12 && (u - 0.5).abs() < 1e-12);
        assert!(intersect([0.0, 0.0], [10.0, 0.0], [0.0, 1.0], [10.0, 1.0]).is_none());
    }

    fn same(a: &[Vec<Pt>], b: &[Vec<Pt>]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| dist(*p, *q) < 1e-9))
    }

    #[test]
    fn trim_cuts_between_crossings() {
        let line = [[0.0, 0.0], [100.0, 0.0]];
        let walls = [([30.0, -10.0], [30.0, 10.0]), ([60.0, -10.0], [60.0, 10.0])];
        // Clicked between the two walls: two pieces stay.
        let pieces = trim(&line, &walls, 45.0).unwrap();
        assert!(same(&pieces, &[vec![[0.0, 0.0], [30.0, 0.0]], vec![[60.0, 0.0], [100.0, 0.0]]]), "{pieces:?}");
        // Clicked past the last wall: the end goes.
        let pieces = trim(&line, &walls, 80.0).unwrap();
        assert!(same(&pieces, &[vec![[0.0, 0.0], [60.0, 0.0]]]), "{pieces:?}");
        // Nothing crosses.
        assert!(trim(&line, &[], 50.0).is_none());
        // A polyline keeps its corner.
        let l = [[0.0, 0.0], [50.0, 0.0], [50.0, 50.0]];
        let pieces = trim(&l, &[([40.0, -5.0], [40.0, 5.0])], 70.0).unwrap();
        assert!(same(&pieces, &[vec![[0.0, 0.0], [40.0, 0.0]]]), "{pieces:?}");
        let pieces = trim(&l, &[([40.0, -5.0], [40.0, 5.0])], 20.0).unwrap();
        assert!(same(&pieces, &[vec![[40.0, 0.0], [50.0, 0.0], [50.0, 50.0]]]), "{pieces:?}");
    }

    #[test]
    fn extend_reaches_the_next_line() {
        let line = [[0.0, 0.0], [10.0, 0.0]];
        let wall = [([50.0, -10.0], [50.0, 10.0]), ([80.0, -10.0], [80.0, 10.0])];
        assert_eq!(extend(&line, true, &wall), Some([50.0, 0.0]));
        assert_eq!(extend(&line, false, &wall), None, "nothing behind the start");
    }

    #[test]
    fn corners_joins_and_splits() {
        let h = [[0.0, 0.0], [8.0, 0.0]];
        let v = [[10.0, 2.0], [10.0, 20.0]];
        // A sharp corner: both lines reach (10, 0).
        let (a, b, extra) = corner(&h, [2.0, 0.0], &v, [10.0, 15.0], Corner::Fillet(0.0)).unwrap();
        assert!(dist(*a.last().unwrap(), [10.0, 0.0]) < 1e-9 && dist(b[0], [10.0, 0.0]) < 1e-9 && extra.is_none(), "{a:?} {b:?}");
        // A rounded one of radius 2: tangent at (8, 0) and (10, 2).
        let (a, b, arc) = corner(&h, [2.0, 0.0], &v, [10.0, 15.0], Corner::Fillet(2.0)).unwrap();
        assert!(dist(*a.last().unwrap(), [8.0, 0.0]) < 1e-9 && dist(b[0], [10.0, 2.0]) < 1e-9);
        assert!(arc.unwrap().iter().all(|p| (dist(*p, [8.0, 2.0]) - 2.0).abs() < 1e-9));
        let (_, _, cut) = corner(&h, [2.0, 0.0], &v, [10.0, 15.0], Corner::Chamfer(1.0)).unwrap();
        assert_eq!(cut.map(|c| c.len()), Some(2));
        assert!(corner(&h, [2.0, 0.0], &[[0.0, 5.0], [8.0, 5.0]], [4.0, 5.0], Corner::Fillet(0.0)).is_none(), "parallel");
        assert_eq!(join(&h, &[[8.0, 0.0], [8.0, 6.0]], 0.01), Some(vec![[0.0, 0.0], [8.0, 0.0], [8.0, 6.0]]));
        assert_eq!(join(&h, &[[8.0, 6.0], [8.0, 0.0]], 0.01), Some(vec![[0.0, 0.0], [8.0, 0.0], [8.0, 6.0]]));
        assert!(join(&h, &v, 0.01).is_none());
        let (a, b) = split(&[[0.0, 0.0], [20.0, 0.0]], 5.0).unwrap();
        assert!(dist(*a.last().unwrap(), [5.0, 0.0]) < 1e-9 && b.len() == 2);
        assert!(split(&[[0.0, 0.0], [20.0, 0.0]], 0.2).is_none());
    }

    #[test]
    fn shapes_and_matrices() {
        let hex = regular_polygon([0.0, 0.0], [10.0, 0.0], 6);
        assert_eq!(hex.len(), 6);
        assert!(hex.iter().all(|p| (p[0].hypot(p[1]) - 10.0).abs() < 1e-9));
        let s = spline_points(&[[0.0, 0.0], [10.0, 10.0], [20.0, 0.0]], false);
        assert!(s.len() > 10 && dist(s[0], [0.0, 0.0]) < 1e-9 && dist(*s.last().unwrap(), [20.0, 0.0]) < 1e-9);
        assert!(s.iter().any(|p| dist(*p, [10.0, 10.0]) < 1e-9), "passes through its points");
        let m = mirror([0.0, 0.0], [0.0, 10.0]).unwrap();
        let p = geometry::apply(&m, [3.0, 4.0]);
        assert!(dist(p, [-3.0, 4.0]) < 1e-9, "{p:?}");
        let m = mirror([0.0, 1.0], [10.0, 1.0]).unwrap();
        assert!(dist(geometry::apply(&m, [3.0, 4.0]), [3.0, -2.0]) < 1e-9);
        assert!(dist(geometry::apply(&scaling([1.0, 1.0], 2.0), [2.0, 3.0]), [3.0, 5.0]) < 1e-9);
    }

    #[test]
    fn offsets_and_arcs() {
        let p = offset_path(&[[0.0, 0.0], [10.0, 0.0]], 2.0, false);
        assert_eq!(p, vec![[0.0, 2.0], [10.0, 2.0]]);
        let p = offset_path(&[[0.0, 0.0], [10.0, 0.0]], -2.0, false);
        assert_eq!(p, vec![[0.0, -2.0], [10.0, -2.0]]);
        // A square grown outward by 1 on every side (counter-clockwise: left is inside).
        let sq = offset_path(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], -1.0, true);
        assert!((sq[0][0] + 1.0).abs() < 1e-9 && (sq[0][1] + 1.0).abs() < 1e-9, "{sq:?}");
        assert_eq!(side(&[[0.0, 0.0], [10.0, 0.0]], false, [5.0, 3.0]), 1.0);
        assert_eq!(side(&[[0.0, 0.0], [10.0, 0.0]], false, [5.0, -3.0]), -1.0);
        // A half circle through the top.
        let arc = arc_points([-10.0, 0.0], [0.0, 10.0], [10.0, 0.0]);
        assert!(arc.len() > 10);
        assert!(arc.iter().all(|p| (p[0].hypot(p[1]) - 10.0).abs() < 1e-6 && p[1] >= -1e-9));
        assert_eq!(arc_points([0.0, 0.0], [1.0, 0.0], [2.0, 0.0]), vec![[0.0, 0.0], [2.0, 0.0]]);
    }
}
