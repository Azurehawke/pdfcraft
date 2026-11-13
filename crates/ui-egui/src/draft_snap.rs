//! RevuCraft: the drafting tools' angle guides, crossings and ruler, after BIMCraft's snaps
//! (which follow Revit's). A direction from the last point sticks to multiples of an angle step,
//! from east and from the line before it, along a dashed guide; where the guide or the pointer
//! meets a line, the point lands on the crossing. The line being drawn carries a ruler, and its
//! corner with the line before shows the angle between them. Typing SE, SM, SI, SP or SN takes
//! one snap alone for the next point (SO none, SS back to all), as in Revit.
//!
//! Everything here is plain geometry in user space; `draft::page_input` does the drawing.

use pdfcraft_engine::measure::snap::SnapKind;

type Pt = [f64; 2];

/// The angle steps a direction can stick to, in degrees (0: off).
pub(crate) const ANGLE_STEPS: [f64; 6] = [0.0, 5.0, 15.0, 30.0, 45.0, 90.0];

/// Segments looked at for crossings near the pointer, at most (a heavy drawing has thousands).
const MAX_NEAR: usize = 256;

/// A snap taken alone for the next point, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OneShot {
    Only(SnapKind),
    Off,
}

/// What typed letters ask of the next point: `Some(Some(one))` a one-off snap, `Some(None)`
/// back to the usual snaps (SS), `None` not a snap code. Lengths and coordinates never start
/// with S, so the codes can share the value box.
pub(crate) fn one_shot_code(typed: &str) -> Option<Option<OneShot>> {
    match typed.trim().to_ascii_lowercase().as_str() {
        "se" => Some(Some(OneShot::Only(SnapKind::Endpoint))),
        "sm" => Some(Some(OneShot::Only(SnapKind::Midpoint))),
        "si" => Some(Some(OneShot::Only(SnapKind::Intersection))),
        "sp" => Some(Some(OneShot::Only(SnapKind::Perpendicular))),
        "sn" => Some(Some(OneShot::Only(SnapKind::Path))),
        "so" => Some(Some(OneShot::Off)),
        "ss" => Some(None),
        _ => None,
    }
}

/// Whether snap `k` may catch the next point.
pub(crate) fn allowed(one_shot: Option<OneShot>, k: SnapKind) -> bool {
    match one_shot {
        None => true,
        Some(OneShot::Only(only)) => only == k,
        Some(OneShot::Off) => false,
    }
}

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

fn finite(p: Pt) -> bool {
    p.iter().all(|v| v.is_finite())
}

/// The guide a direction from `last` sticks to: `p` moved onto the nearest multiple of `step`
/// degrees, counted from east and from the line `prev` → `last`, when `p` is within `tolerance`
/// of it (and ahead of `last`). Returns the point and the guide's unit direction.
pub(crate) fn polar(last: Pt, prev: Option<Pt>, p: Pt, step: f64, tolerance: f64) -> Option<(Pt, Pt)> {
    if !(step.is_finite() && step > 0.0 && step <= 180.0 && tolerance.is_finite() && tolerance > 0.0) || !finite(last) || !finite(p) {
        return None;
    }
    let d = sub(p, last);
    let len = d[0].hypot(d[1]);
    if len <= tolerance {
        return None;
    }
    let mut bases = vec![0.0];
    if let Some(q) = prev.filter(|q| finite(*q)) {
        let v = sub(last, q);
        if v[0].hypot(v[1]) > 1e-9 {
            bases.push(v[1].atan2(v[0]).to_degrees());
        }
    }
    let angle = d[1].atan2(d[0]).to_degrees();
    let mut best: Option<(Pt, Pt, f64)> = None;
    for base in bases {
        let k = ((angle - base) / step).round();
        let dir_deg = base + k * step;
        let (s, c) = dir_deg.to_radians().sin_cos();
        let t = d[0] * c + d[1] * s;
        let off = (d[0] * s - d[1] * c).abs();
        if t > 0.0 && off <= tolerance && best.is_none_or(|(_, _, b)| off < b) {
            best = Some(([last[0] + c * t, last[1] + s * t], [c, s], off));
        }
    }
    best.map(|(q, dir, _)| (q, dir))
}

/// Where the lines `a`–`b` and `c`–`d` cross (as segments), if they do.
fn crossing(a: Pt, b: Pt, c: Pt, d: Pt) -> Option<Pt> {
    let r = sub(b, a);
    let s = sub(d, c);
    let den = r[0] * s[1] - r[1] * s[0];
    if !den.is_finite() || den.abs() < 1e-12 {
        return None;
    }
    let q = sub(c, a);
    let t = (q[0] * s[1] - q[1] * s[0]) / den;
    let u = (q[0] * r[1] - q[1] * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| [a[0] + r[0] * t, a[1] + r[1] * t])
}

/// How far `p` is from the segment `a`–`b`.
fn distance_to(p: Pt, a: Pt, b: Pt) -> f64 {
    let ab = sub(b, a);
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 > 1e-18 { ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2 } else { 0.0 };
    let t = t.clamp(0.0, 1.0);
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
    (p[0] - q[0]).hypot(p[1] - q[1])
}

/// The crossing of two of `segments` nearest `p`, within `tolerance` (lines drawn as markup
/// cross each other where the page's own drawing has no point).
/// How far from a tracked point the pointer may wander before the point is let go, in tolerances.
pub(crate) const TRACK_REACH: f64 = 30.0;

/// Most points tracked at once; a new one replaces the oldest.
const MAX_TRACKS: usize = 4;

fn gap(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Keeps the tracks on `page` the pointer is still near, and starts tracking the anchor nearest the
/// pointer when it is within `tol` (unless that point is tracked already). Tracks on other pages go.
pub(crate) fn track(tracks: &mut Vec<(usize, Pt)>, page: usize, anchors: &[Pt], raw: Pt, tol: f64, reach: f64) {
    tracks.retain(|(p, t)| *p == page && gap(*t, raw) <= reach);
    let nearest = anchors.iter().filter(|a| finite(**a) && gap(**a, raw) <= tol).min_by(|a, b| gap(**a, raw).total_cmp(&gap(**b, raw))).copied();
    if let Some(a) = nearest
        && !tracks.iter().any(|(_, t)| gap(*t, a) <= tol * 0.5)
    {
        tracks.push((page, a));
    }
    while tracks.len() > MAX_TRACKS {
        tracks.remove(0);
    }
}

/// The tracked point whose x is nearest the pointer's, and the one whose y is, each only when within
/// `tol`. Either can be none; one point can answer for both.
pub(crate) fn aligned(tracks: &[Pt], raw: Pt, tol: f64) -> (Option<Pt>, Option<Pt>) {
    let x = tracks.iter().filter(|t| (t[0] - raw[0]).abs() <= tol).min_by(|a, b| (a[0] - raw[0]).abs().total_cmp(&(b[0] - raw[0]).abs())).copied();
    let y = tracks.iter().filter(|t| (t[1] - raw[1]).abs() <= tol).min_by(|a, b| (a[1] - raw[1]).abs().total_cmp(&(b[1] - raw[1]).abs())).copied();
    (x, y)
}

/// The point of each segment nearest `p` that lies within `tolerance` of it: the foot of the
/// perpendicular when that falls on the segment, otherwise the nearer end. A segment with no length
/// gives its one point. At most `MAX_NEAR` are returned, so a crowded page costs the same per move.
pub(crate) fn edge_points(segments: &[(Pt, Pt)], p: Pt, tolerance: f64) -> Vec<Pt> {
    let mut out = Vec::new();
    if !finite(p) || !(tolerance.is_finite() && tolerance > 0.0) {
        return out;
    }
    for &(a, b) in segments {
        if out.len() >= MAX_NEAR {
            break;
        }
        if finite(a) && finite(b) && distance_to(p, a, b) <= tolerance {
            out.push(foot(p, a, b));
        }
    }
    out
}

/// The point on segment `a`–`b` nearest `p`, clamped to the segment's ends.
fn foot(p: Pt, a: Pt, b: Pt) -> Pt {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if !(len2.is_finite() && len2 > 0.0) {
        return a;
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
    [a[0] + t * dx, a[1] + t * dy]
}

pub(crate) fn nearest_crossing(segments: impl IntoIterator<Item = [Pt; 2]>, p: Pt, tolerance: f64) -> Option<Pt> {
    if !finite(p) || !(tolerance.is_finite() && tolerance > 0.0) {
        return None;
    }
    let near: Vec<[Pt; 2]> =
        segments.into_iter().filter(|[a, b]| finite(*a) && finite(*b) && distance_to(p, *a, *b) <= tolerance).take(MAX_NEAR).collect();
    let mut best: Option<(Pt, f64)> = None;
    for (i, [a, b]) in near.iter().enumerate() {
        for [c, d] in near.iter().skip(i + 1) {
            if let Some(x) = crossing(*a, *b, *c, *d) {
                let dist = (x[0] - p[0]).hypot(x[1] - p[1]);
                if dist <= tolerance && best.is_none_or(|(_, bd)| dist < bd) {
                    best = Some((x, dist));
                }
            }
        }
    }
    best.map(|(x, _)| x)
}

/// Where the guide from `from` along `dir` crosses one of `segments`, nearest `p` and within
/// `tolerance` of it: the point lands on the line it meets.
pub(crate) fn guide_crossing(from: Pt, dir: Pt, segments: impl IntoIterator<Item = [Pt; 2]>, p: Pt, tolerance: f64) -> Option<Pt> {
    if !finite(from) || !finite(dir) || !finite(p) || !(tolerance.is_finite() && tolerance > 0.0) {
        return None;
    }
    // The guide as a long segment through the pointer.
    let reach = (p[0] - from[0]).hypot(p[1] - from[1]) + tolerance * 4.0;
    let to = [from[0] + dir[0] * reach, from[1] + dir[1] * reach];
    let mut best: Option<(Pt, f64)> = None;
    for [a, b] in segments.into_iter().take(MAX_NEAR * 64) {
        if let Some(x) = crossing(from, to, a, b) {
            let dist = (x[0] - p[0]).hypot(x[1] - p[1]);
            if dist <= tolerance && dist > 1e-9 && best.is_none_or(|(_, bd)| dist < bd) {
                best = Some((x, dist));
            }
        }
    }
    best.map(|(x, _)| x)
}

/// Where the square run from `last` (across or down) meets one of `lines`, nearest `p` and within
/// `tolerance`: a point both square to `last` and on the line.
pub(crate) fn square_crossing(last: Pt, p: Pt, lines: &[[Pt; 2]], tolerance: f64) -> Option<Pt> {
    let across = guide_crossing(last, [1.0, 0.0], lines.iter().copied(), p, tolerance);
    let down = guide_crossing(last, [0.0, 1.0], lines.iter().copied(), p, tolerance);
    let dist = |x: Pt| (x[0] - p[0]).hypot(x[1] - p[1]);
    match (across, down) {
        (Some(a), Some(b)) => Some(if dist(a) <= dist(b) { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// A round step for the ruler's graduations along a line `length` long (in the scale's units):
/// 1, 2 or 5 times a power of ten, so about five to ten fit.
pub(crate) fn ruler_step(length: f64) -> Option<f64> {
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let raw = length / 8.0;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * mag).find(|s| *s >= raw)?;
    (step.is_finite() && step > 0.0).then_some(step)
}

/// The angle at `corner` between the line from `prev` and the line to `next`, in degrees
/// (0 to 180), when both have a length.
pub(crate) fn corner_angle(prev: Pt, corner: Pt, next: Pt) -> Option<f64> {
    let a = sub(prev, corner);
    let b = sub(next, corner);
    let (la, lb) = (a[0].hypot(a[1]), b[0].hypot(b[1]));
    if !(la > 1e-9 && lb > 1e-9) || !la.is_finite() || !lb.is_finite() {
        return None;
    }
    let cos = ((a[0] * b[0] + a[1] * b[1]) / (la * lb)).clamp(-1.0, 1.0);
    Some(cos.acos().to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Pt, b: Pt) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn directions_stick_to_angle_steps_from_east_and_the_last_line() {
        // 12° off east at 15° steps: onto east, 100 along.
        let (p, dir) = polar([0.0, 0.0], None, [100.0, 2.0], 15.0, 5.0).unwrap();
        assert!(close(p, [100.0, 0.0]), "{p:?}");
        assert!(close(dir, [1.0, 0.0]));
        // Near 30°: onto 30°.
        let (p, _) = polar([0.0, 0.0], None, [86.0, 51.0], 15.0, 5.0).unwrap();
        assert!((p[1].atan2(p[0]).to_degrees() - 30.0).abs() < 1e-9);
        // Too far off a step, or behind the point: nothing.
        assert!(polar([0.0, 0.0], None, [100.0, 13.0], 15.0, 5.0).is_none());
        assert!(polar([0.0, 0.0], None, [1.0, 0.0], 15.0, 5.0).is_none(), "within the tolerance of the point itself");
        // From the last line: after a line at 10°, 10° + 90° is a step too.
        let last_line = Some([-(10f64.to_radians().cos()) * 50.0, -(10f64.to_radians().sin()) * 50.0]);
        let (p, _) = polar([0.0, 0.0], last_line, [-17.0, 98.0], 90.0, 3.0).unwrap();
        assert!((p[1].atan2(p[0]).to_degrees() - 100.0).abs() < 1e-6, "{p:?}");
        // Nonsense in, nothing out.
        assert!(polar([0.0, 0.0], None, [f64::NAN, 1.0], 15.0, 5.0).is_none());
        assert!(polar([0.0, 0.0], None, [100.0, 0.0], 0.0, 5.0).is_none());
    }

    #[test]
    fn crossings_of_drawn_lines_and_of_the_guide() {
        let segs = [[[0.0, 0.0], [100.0, 100.0]], [[0.0, 100.0], [100.0, 0.0]], [[500.0, 0.0], [500.0, 10.0]]];
        assert!(close(nearest_crossing(segs, [52.0, 49.0], 5.0).unwrap(), [50.0, 50.0]));
        assert!(nearest_crossing(segs, [70.0, 50.0], 5.0).is_none(), "too far from the crossing");
        // The guide east from (0, 20) meets the wall at x = 80.
        let wall = [[[80.0, -50.0], [80.0, 50.0]]];
        assert!(close(guide_crossing([0.0, 20.0], [1.0, 0.0], wall, [78.0, 20.0], 5.0).unwrap(), [80.0, 20.0]));
        assert!(guide_crossing([0.0, 20.0], [1.0, 0.0], wall, [60.0, 20.0], 5.0).is_none());
    }

    #[test]
    fn rulers_graduate_in_round_steps_and_corners_read_their_angle() {
        assert_eq!(ruler_step(12.5), Some(2.0));
        assert_eq!(ruler_step(80.0), Some(10.0));
        assert!((ruler_step(0.3).unwrap() - 0.05).abs() < 1e-12);
        assert_eq!(ruler_step(0.0), None);
        assert_eq!(ruler_step(f64::INFINITY), None);
        assert!((corner_angle([0.0, 10.0], [0.0, 0.0], [10.0, 0.0]).unwrap() - 90.0).abs() < 1e-9);
        assert!((corner_angle([-10.0, 0.0], [0.0, 0.0], [10.0, 10.0]).unwrap() - 135.0).abs() < 1e-9);
        assert_eq!(corner_angle([0.0, 0.0], [0.0, 0.0], [1.0, 0.0]), None);
    }

    #[test]
    fn typed_codes_take_one_snap_for_the_next_point() {
        assert_eq!(one_shot_code("SE"), Some(Some(OneShot::Only(SnapKind::Endpoint))));
        assert_eq!(one_shot_code("si"), Some(Some(OneShot::Only(SnapKind::Intersection))));
        assert_eq!(one_shot_code("so"), Some(Some(OneShot::Off)));
        assert_eq!(one_shot_code("ss"), Some(None));
        assert_eq!(one_shot_code("s"), None);
        assert_eq!(one_shot_code("15'"), None);
        assert!(allowed(None, SnapKind::Midpoint));
        assert!(allowed(Some(OneShot::Only(SnapKind::Midpoint)), SnapKind::Midpoint));
        assert!(!allowed(Some(OneShot::Only(SnapKind::Midpoint)), SnapKind::Endpoint));
        assert!(!allowed(Some(OneShot::Off), SnapKind::Path));
    }
}

/// The centre lines of smart walls, end to end. Extra snap lines while the Wall tool is drawing,
/// so a run can end on another wall's centre instead of being pulled to its face.
pub(crate) fn wall_centres(quads: impl IntoIterator<Item = [[f64; 2]; 4]>) -> Vec<(Pt, Pt)> {
    let mid = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    quads.into_iter().map(|q| (mid(q[3], q[0]), mid(q[1], q[2]))).collect()
}

#[cfg(test)]
mod wall_centre_tests {
    #[test]
    fn a_walls_centre_line_runs_end_to_end() {
        let q = [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]];
        assert_eq!(super::wall_centres([q]), vec![([0.0, 0.0], [100.0, 0.0])]);
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    #[test]
    fn the_foot_of_a_perpendicular_is_the_edge_point() {
        let pts = edge_points(&[([0.0, 0.0], [100.0, 0.0])], [40.0, 3.0], 5.0);
        assert_eq!(pts, vec![[40.0, 0.0]]);
    }

    #[test]
    fn a_point_past_an_end_snaps_to_that_end_not_the_extension() {
        let pts = edge_points(&[([0.0, 0.0], [100.0, 0.0])], [104.0, 2.0], 5.0);
        assert_eq!(pts, vec![[100.0, 0.0]]);
    }

    #[test]
    fn only_segments_within_tolerance_count_and_a_zero_length_one_is_its_point() {
        let segs = [([0.0, 50.0], [100.0, 50.0]), ([10.0, 10.0], [10.0, 10.0])];
        assert_eq!(edge_points(&segs, [12.0, 11.0], 5.0), vec![[10.0, 10.0]]);
        assert!(edge_points(&segs, [12.0, 30.0], 5.0).is_empty());
    }

    #[test]
    fn a_bad_pointer_or_tolerance_gives_nothing() {
        let segs = [([0.0, 0.0], [10.0, 0.0])];
        assert!(edge_points(&segs, [f64::NAN, 0.0], 5.0).is_empty());
        assert!(edge_points(&segs, [1.0, 0.0], f64::NAN).is_empty());
    }
}

#[cfg(test)]
mod align_tests {
    use super::*;

    #[test]
    fn hovering_an_anchor_tracks_it_once() {
        let mut tracks = Vec::new();
        track(&mut tracks, 0, &[[100.0, 100.0]], [102.0, 101.0], 5.0, 150.0);
        track(&mut tracks, 0, &[[100.0, 100.0]], [101.0, 100.0], 5.0, 150.0);
        assert_eq!(tracks, vec![(0, [100.0, 100.0])]);
    }

    #[test]
    fn a_track_is_let_go_when_the_pointer_leaves_it_or_the_page() {
        let mut tracks = vec![(0, [100.0, 100.0])];
        track(&mut tracks, 0, &[], [400.0, 400.0], 5.0, 150.0);
        assert!(tracks.is_empty());
        let mut tracks = vec![(1, [100.0, 100.0])];
        track(&mut tracks, 0, &[], [100.0, 100.0], 5.0, 150.0);
        assert!(tracks.is_empty(), "a track on another page goes");
    }

    #[test]
    fn the_pointer_lined_up_with_a_tracked_point_gets_its_x_or_y() {
        let tracks = [[100.0, 100.0]];
        assert_eq!(aligned(&tracks, [102.0, 300.0], 5.0), (Some([100.0, 100.0]), None));
        assert_eq!(aligned(&tracks, [300.0, 103.0], 5.0), (None, Some([100.0, 100.0])));
        assert_eq!(aligned(&tracks, [300.0, 300.0], 5.0), (None, None));
    }

    #[test]
    fn two_tracks_give_a_crossing_of_their_guides() {
        let tracks = [[100.0, 40.0], [300.0, 200.0]];
        assert_eq!(aligned(&tracks, [101.0, 199.0], 5.0), (Some([100.0, 40.0]), Some([300.0, 200.0])));
    }
}
