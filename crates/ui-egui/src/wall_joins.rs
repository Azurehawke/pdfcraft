//! RevuCraft: how the Wall tool's new walls meet the smart walls already drawn, as BIMCraft's
//! (and Revit's) walls do. A run's corners are joined as it is drawn; a free end that stops on an
//! existing wall joins it too:
//!
//! - at that wall's end, an L: both are cut on the mitre, and neither draws a line across it;
//! - along that wall, a T: the new wall stops at the face it meets, and that face opens for it.
//!
//! Everything here is worked out from the outlines; the edits are `AddWall` and `SetWall`.

use pdfcraft_engine::walls::{self, Wall};
use pdfcraft_engine::{Edit, Style};

type Pt = [f64; 2];

/// How far past a wall (points) an end still counts as on it.
const REACH: f64 = 2.0;
/// Walls this close to parallel (the sine of the angle between them) don't join.
const PARALLEL: f64 = 0.1;
/// A mitre reaching further than this many thicknesses from the joint (a very sharp angle) is
/// not made.
const MAX_MITRE: f64 = 6.0;

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

fn mid(a: Pt, b: Pt) -> Pt {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

fn cross(a: Pt, b: Pt) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn dot(a: Pt, b: Pt) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn dist(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Where the line through `p` and `q` meets the line through `r` and `s`.
fn meet(p: Pt, q: Pt, r: Pt, s: Pt) -> Option<Pt> {
    let (d1, d2) = (sub(q, p), sub(s, r));
    let den = cross(d1, d2);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = cross(sub(r, p), d2) / den;
    let x = [p[0] + d1[0] * t, p[1] + d1[1] * t];
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Which side of the line `a`–`b` the point `p` is on.
fn side(a: Pt, b: Pt, p: Pt) -> f64 {
    cross(sub(b, a), sub(p, a))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    A,
    B,
}

/// A new wall's end `end`: its centre, its corner on face 0 and on face 1, and its other end's
/// centre.
fn end_of(q: &[Pt; 4], end: End) -> (Pt, usize, usize, Pt) {
    match end {
        End::A => (mid(q[3], q[0]), 0, 3, mid(q[1], q[2])),
        End::B => (mid(q[1], q[2]), 1, 2, mid(q[3], q[0])),
    }
}

/// How the free end `end` of the new wall `q` joins one of `existing`: the new wall's outline,
/// and the edit to the wall it joins.
fn join(q: &[Pt; 4], end: End, existing: &[Wall], page: usize) -> Option<([Pt; 4], Edit)> {
    let (p, c0, c1, far) = end_of(q, end);
    let (f0, f1) = ((q[0], q[1]), (q[3], q[2]));
    let mine = walls::axis(q)?;
    let thick = mine.thickness();
    for w in existing.iter().filter(|w| !w.locked && !w.nm.is_empty()) {
        let Some(ax) = walls::axis(&w.quad) else { continue };
        if cross(mine.along, ax.along).abs() < PARALLEL {
            continue;
        }
        let t = ax.t_of(p);
        let h = dot(ax.left, sub(p, ax.a));
        let half = ax.thickness() / 2.0;
        let centre = (ax.h0 + ax.h1) / 2.0;
        if (h - centre).abs() > half + REACH {
            continue;
        }
        let reach_t = (half + REACH) / ax.len;
        let limit = thick.max(ax.thickness()) * MAX_MITRE;
        let (g0, g1) = ((w.quad[0], w.quad[1]), (w.quad[3], w.quad[2]));
        let at_a = t.abs() <= reach_t && !w.data.ja;
        let at_b = (t - 1.0).abs() <= reach_t && !w.data.jb;
        if at_a || at_b {
            // An L: the mitre runs from the inside corner to the outside one, between the two
            // walls' far ends.
            let theirs_far = if at_a { ax.at(1.0, centre) } else { ax.at(0.0, centre) };
            let pairs = [
                (meet(f0.0, f0.1, g0.0, g0.1), meet(f1.0, f1.1, g1.0, g1.1), false),
                (meet(f0.0, f0.1, g1.0, g1.1), meet(f1.0, f1.1, g0.0, g0.1), true),
            ];
            let found = pairs.into_iter().find_map(|(x, y, crossed)| {
                let (x, y) = (x?, y?);
                let opposite = side(x, y, far) * side(x, y, theirs_far) < 0.0;
                (opposite && dist(x, p) <= limit && dist(y, p) <= limit).then_some((x, y, crossed))
            });
            let Some((x, y, crossed)) = found else { continue };
            let mut new = *q;
            *new.get_mut(c0)? = x;
            *new.get_mut(c1)? = y;
            // Their corners on the mitre: on face 0 and face 1 of theirs.
            let (on_g0, on_g1) = if crossed { (y, x) } else { (x, y) };
            let mut theirs = w.quad;
            let (k0, k1) = if at_a { (0, 3) } else { (1, 2) };
            *theirs.get_mut(k0)? = on_g0;
            *theirs.get_mut(k1)? = on_g1;
            walls::axis(&new)?;
            walls::axis(&theirs)?;
            let edit = Edit::SetWall { page, index: w.index, quad: Some(theirs), ja: at_a.then_some(true), jb: at_b.then_some(true), gap: None };
            return Some((new, edit));
        }
        if t > reach_t && t < 1.0 - reach_t {
            // A T: stop at the face on this side, which opens where the new wall meets it.
            let toward = dot(ax.left, sub(far, ax.a)) - centre;
            let near0 = (ax.h0 - centre) * toward > 0.0;
            let g = if near0 { g0 } else { g1 };
            let (Some(x), Some(y)) = (meet(f0.0, f0.1, g.0, g.1), meet(f1.0, f1.1, g.0, g.1)) else { continue };
            if dist(x, p) > limit || dist(y, p) > limit {
                continue;
            }
            let (tx, ty) = (ax.t_of(x), ax.t_of(y));
            if !(tx.is_finite() && ty.is_finite()) || tx.min(ty) < 0.0 || tx.max(ty) > 1.0 {
                continue;
            }
            let mut new = *q;
            *new.get_mut(c0)? = x;
            *new.get_mut(c1)? = y;
            walls::axis(&new)?;
            let face = if near0 { 0 } else { 1 };
            let edit = Edit::SetWall { page, index: w.index, quad: None, ja: None, jb: None, gap: Some((face, tx.min(ty), tx.max(ty))) };
            return Some((new, edit));
        }
    }
    None
}

/// The edits that draw the Wall tool's runs `quads` (in order, closed into a loop or not) as smart
/// walls, joined to each other and to the smart walls already on the page.
pub fn wall_edits(page: usize, quads: &[[Pt; 4]], closed: bool, existing: &[Wall], style: &Style, author: &str) -> Vec<Edit> {
    let count = quads.len();
    let mut quads = quads.to_vec();
    let mut changes = Vec::new();
    let mut ends: Vec<(bool, bool)> = (0..count).map(|i| (closed || i > 0, closed || i + 1 < count)).collect();
    if !closed {
        let mut used: Vec<usize> = Vec::new();
        for (i, end) in [(0, End::A), (count.saturating_sub(1), End::B)] {
            let Some(q) = quads.get(i).copied() else { continue };
            let free: Vec<Wall> = existing.iter().filter(|w| !used.contains(&w.index)).cloned().collect();
            if let Some((new, edit)) = join(&q, end, &free, page) {
                if let Edit::SetWall { index, .. } = &edit {
                    used.push(*index);
                }
                if let Some(slot) = quads.get_mut(i) {
                    *slot = new;
                }
                if let Some(e) = ends.get_mut(i) {
                    match end {
                        End::A => e.0 = true,
                        End::B => e.1 = true,
                    }
                }
                changes.push(edit);
            }
        }
    }
    changes.extend(quads.into_iter().zip(ends).map(|(quad, (ja, jb))| Edit::AddWall {
        page,
        quad,
        ja,
        jb,
        style: style.clone(),
        author: author.to_string(),
    }));
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(index: usize, quad: [Pt; 4]) -> Wall {
        Wall { index, nm: format!("w{index}"), quad, data: walls::WallData::default(), locked: false }
    }

    fn style() -> Style {
        Style { color: [0.0; 3], opacity: 1.0, width: 0.75, fill: Some([0.78; 3]) }
    }

    fn add_walls(edits: &[Edit]) -> Vec<([Pt; 4], bool, bool)> {
        edits
            .iter()
            .filter_map(|e| match e {
                Edit::AddWall { quad, ja, jb, .. } => Some((*quad, *ja, *jb)),
                _ => None,
            })
            .collect()
    }

    fn near(a: Pt, b: Pt) -> bool {
        dist(a, b) < 1e-6
    }

    #[test]
    fn a_runs_corners_are_joined_and_its_free_ends_are_not() {
        let quads = [[[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]], [[100.0, 3.0], [100.0, 100.0], [106.0, 100.0], [106.0, -3.0]]];
        let edits = wall_edits(0, &quads, false, &[], &style(), "Steve");
        let added = add_walls(&edits);
        assert_eq!(added.iter().map(|a| (a.1, a.2)).collect::<Vec<_>>(), vec![(false, true), (true, false)]);
        let closed = wall_edits(0, &quads, true, &[], &style(), "Steve");
        assert!(add_walls(&closed).iter().all(|a| a.1 && a.2));
    }

    #[test]
    fn an_end_at_another_walls_end_makes_an_l() {
        // An existing wall along x from 0 to 100, 6 thick, centred on y = 0; a new one drawn up
        // from its end B.
        let old = wall(4, [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]]);
        // Drawn from (100, 0) up to (100, 80): end A at the joint, face 0 on the left (x < 100).
        let new = [[97.0, 0.0], [97.0, 80.0], [103.0, 80.0], [103.0, 0.0]];
        let edits = wall_edits(0, &[new], false, std::slice::from_ref(&old), &style(), "Steve");
        let Some(Edit::SetWall { index: 4, quad: Some(theirs), ja: None, jb: Some(true), gap: None, .. }) = edits.first() else {
            panic!("the old wall is joined at its end B: {edits:?}")
        };
        let [(mine, ja, jb)] = add_walls(&edits)[..] else { panic!("one new wall") };
        assert!(ja && !jb, "joined at the joint only");
        // The outside corner (103, -3) and the inside corner (97, 3) are shared.
        assert!(near(mine[3], [103.0, -3.0]) && near(mine[0], [97.0, 3.0]), "{mine:?}");
        assert!(near(theirs[2], [103.0, -3.0]) && near(theirs[1], [97.0, 3.0]), "{theirs:?}");
    }

    #[test]
    fn an_end_along_another_wall_makes_a_t() {
        let old = wall(1, [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]]);
        // Drawn from (50, 0), in the old wall, up to (50, 80).
        let new = [[47.0, 0.0], [47.0, 80.0], [53.0, 80.0], [53.0, 0.0]];
        let edits = wall_edits(0, &[new], false, std::slice::from_ref(&old), &style(), "Steve");
        let Some(Edit::SetWall { index: 1, quad: None, gap: Some((face, t0, t1)), .. }) = edits.first() else {
            panic!("the old wall opens: {edits:?}")
        };
        assert_eq!(*face, 0, "the face toward the new wall (y = 3)");
        assert!((t0 - 0.47).abs() < 1e-9 && (t1 - 0.53).abs() < 1e-9);
        let [(mine, ja, _)] = add_walls(&edits)[..] else { panic!("one new wall") };
        assert!(ja);
        assert!(near(mine[0], [47.0, 3.0]) && near(mine[3], [53.0, 3.0]), "stops at the face: {mine:?}");
        // Far from any wall, or along it, nothing joins.
        let away = [[47.0, 20.0], [47.0, 80.0], [53.0, 80.0], [53.0, 20.0]];
        assert_eq!(wall_edits(0, &[away], false, std::slice::from_ref(&old), &style(), "Steve").len(), 1);
        let along = [[100.0, 3.0], [200.0, 3.0], [200.0, -3.0], [100.0, -3.0]];
        assert_eq!(wall_edits(0, &[along], false, &[old], &style(), "Steve").len(), 1);
    }
}
