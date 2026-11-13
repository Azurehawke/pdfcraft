//! RevuCraft: how the Wall tool's new walls meet the smart walls already drawn, as BIMCraft's
//! (and Revit's) walls do. A run's corners are joined as it is drawn; a free end that stops on an
//! existing wall joins it too:
//!
//! - at that wall's end, an L: both are cut on the mitre, and neither draws a line across it;
//! - along that wall, a T: the new wall stops at the face it meets, and that face opens for it.
//!
//! Everything here is worked out from the outlines; the edits are `AddWall` and `SetWall`.

use crate::{Edit, Style};
use pdfcraft_annot::walls::{self, Wall};

type Pt = [f64; 2];

/// How far past a wall (points, at the least) an end still counts as on it; a thicker wall
/// reaches as far as its thickness, so a freehand end that looks joined is joined.
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

/// A joint: the joining wall's new outline, the wall it joins (its `index` in the list given to
/// `join`), and what the joint asks of that wall.
struct Joint {
    quad: [Pt; 4],
    partner: usize,
    with: With,
}

/// What a joint asks of the wall it joins.
enum With {
    /// An L at its end (A when `at_a`): its outline is cut on the mitre too.
    Corner { quad: [Pt; 4], at_a: bool },
    /// A butt at its end, in line: its outline keeps, and the end is marked joined.
    Butt { at_a: bool },
    /// A T along it: its face opens where the joining wall meets it.
    Gap(u8, f64, f64),
}

/// How the free end `end` of the wall `q` joins one of `existing`.
fn join(q: &[Pt; 4], end: End, existing: &[Wall]) -> Option<Joint> {
    let (p, c0, c1, far) = end_of(q, end);
    let (f0, f1) = ((q[0], q[1]), (q[3], q[2]));
    let mine = walls::axis(q)?;
    let thick = mine.thickness();
    for w in existing.iter().filter(|w| !w.locked && !w.nm.is_empty()) {
        let Some(ax) = walls::axis(&w.quad) else { continue };
        let reach = REACH.max(thick.max(ax.thickness()));
        if cross(mine.along, ax.along).abs() < PARALLEL {
            // In line: an end on one of the other wall's free ends butts onto it, so a wall can
            // carry on where another stopped, without a notch at the seam.
            let ends = [(mid(w.quad[3], w.quad[0]), true, w.data.ja, [0usize, 3]), (mid(w.quad[1], w.quad[2]), false, w.data.jb, [1, 2])];
            let butt = ends.into_iter().find_map(|(e, at_a, joined, corners)| {
                if joined || dist(p, e) > reach {
                    return None;
                }
                // They carry on from one another; one drawn back over the other doesn't join.
                let theirs_far = if at_a { mid(w.quad[1], w.quad[2]) } else { mid(w.quad[3], w.quad[0]) };
                if dot(sub(p, far), sub(e, theirs_far)) >= 0.0 {
                    return None;
                }
                let (u, v) = (*w.quad.get(corners[0])?, *w.quad.get(corners[1])?);
                let straight = dist(q[c0], u) + dist(q[c1], v) <= dist(q[c0], v) + dist(q[c1], u);
                let (x, y) = if straight { (u, v) } else { (v, u) };
                let mut new = *q;
                *new.get_mut(c0)? = x;
                *new.get_mut(c1)? = y;
                walls::axis(&new)?;
                Some(Joint { quad: new, partner: w.index, with: With::Butt { at_a } })
            });
            if let Some(j) = butt {
                return Some(j);
            }
            continue;
        }
        let t = ax.t_of(p);
        let h = dot(ax.left, sub(p, ax.a));
        let half = ax.thickness() / 2.0;
        let centre = (ax.h0 + ax.h1) / 2.0;
        if (h - centre).abs() > half + reach {
            continue;
        }
        let reach_t = (half + reach) / ax.len;
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
            return Some(Joint { quad: new, partner: w.index, with: With::Corner { quad: theirs, at_a } });
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
            return Some(Joint { quad: new, partner: w.index, with: With::Gap(face, tx.min(ty), tx.max(ty)) });
        }
    }
    None
}

/// Marks new wall `i`'s end (A when `at_a`) joined to the wall named `nm`, in `found` and `with`.
fn joined_to(found: &mut [(bool, bool)], with: &mut [(String, String)], i: usize, at_a: bool, nm: &str) {
    if let Some(f) = found.get_mut(i) {
        if at_a {
            f.0 = true;
        } else {
            f.1 = true;
        }
    }
    if let Some(s) = with.get_mut(i) {
        if at_a {
            s.0 = nm.to_string();
        } else {
            s.1 = nm.to_string();
        }
    }
}

/// Names for `count` new walls, none of them the `/NM` of a wall already on the page.
pub(crate) fn fresh_names(count: usize, existing: &[Wall]) -> Vec<String> {
    let salt = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut taken: Vec<String> = existing.iter().map(|w| w.nm.clone()).collect();
    (0..count)
        .map(|k| {
            let mut n = 0u64;
            loop {
                let name = format!("wall-{salt:x}-{k}-{n}");
                if !taken.contains(&name) {
                    taken.push(name.clone());
                    return name;
                }
                n += 1;
            }
        })
        .collect()
}

/// The middles of a wall's two ends: end A (its corners 3 and 0), then end B (corners 1 and 2).
pub fn end_points(q: &[Pt; 4]) -> [Pt; 2] {
    [mid(q[3], q[0]), mid(q[1], q[2])]
}

/// The outline with end `end` (0 for A, 1 for B) dragged to `to`: its two corners move together, along
/// the wall only, so the rest of the wall stays put. A stretch that would close the wall (or isn't
/// finite) is not made.
pub fn stretch_end(q: &[Pt; 4], end: usize, to: Pt) -> [Pt; 4] {
    let ends = end_points(q);
    let (from, other) = if end == 0 { (ends[0], ends[1]) } else { (ends[1], ends[0]) };
    let along = sub(from, other);
    let len = dot(along, along).sqrt();
    if !len.is_finite() || len <= 1e-9 {
        return *q;
    }
    let dir = [along[0] / len, along[1] / len];
    let shift = dot(sub(to, from), dir);
    let moved = [dir[0] * shift, dir[1] * shift];
    let corners = if end == 0 { [0, 3] } else { [1, 2] };
    let mut out = *q;
    for k in corners {
        if let Some(c) = out.get_mut(k) {
            *c = [c[0] + moved[0], c[1] + moved[1]];
        }
    }
    let new_len = dot(sub(add(from, moved), other), dir);
    if !new_len.is_finite() || new_len <= 1e-9 || !out.iter().all(|p| p.iter().all(|v| v.is_finite())) {
        return *q;
    }
    out
}

fn add(a: Pt, b: Pt) -> Pt {
    [a[0] + b[0], a[1] + b[1]]
}

/// The `SetWall` a joint asks of the wall it joins, with `nm` the joining wall's name.
fn partner_edit(page: usize, index: usize, nm: &str, with: &With) -> Edit {
    match with {
        With::Gap(f, t0, t1) => {
            Edit::SetWall { page, index, quad: None, ja: None, jb: None, gap: Some((*f, *t0, *t1)), ja_with: None, jb_with: None }
        }
        With::Corner { quad, at_a } => Edit::SetWall {
            page,
            index,
            quad: Some(*quad),
            ja: at_a.then_some(true),
            jb: (!at_a).then_some(true),
            gap: None,
            ja_with: at_a.then(|| nm.to_string()),
            jb_with: (!at_a).then(|| nm.to_string()),
        },
        With::Butt { at_a } => Edit::SetWall {
            page,
            index,
            quad: None,
            ja: at_a.then_some(true),
            jb: (!at_a).then_some(true),
            gap: None,
            ja_with: at_a.then(|| nm.to_string()),
            jb_with: (!at_a).then(|| nm.to_string()),
        },
    }
}

/// The join for free end `end` (0 for A, 1 for B) of the wall named `nm` with outline `q`, against
/// `existing`: its new outline, the edit to the wall it joins, and that wall's name when the joint
/// is at an end (a T names nothing: the wall it meets runs on through).
pub fn join_end(page: usize, q: &[Pt; 4], end: usize, nm: &str, existing: &[Wall]) -> Option<([Pt; 4], Edit, Option<String>)> {
    let end = if end == 0 { End::A } else { End::B };
    let j = join(q, end, existing)?;
    let partner = match &j.with {
        With::Gap(..) => None,
        _ => existing.iter().find(|w| w.index == j.partner).map(|w| w.nm.clone()),
    };
    Some((j.quad, partner_edit(page, j.partner, nm, &j.with), partner))
}

/// The edits that draw the Wall tool's runs `quads` (in order, closed into a loop or not) as smart
/// walls, joined to each other and to the smart walls already on the page. Each new wall is named,
/// and each joint records the name of the wall it is joined to.
pub fn wall_edits(page: usize, quads: &[[Pt; 4]], closed: bool, existing: &[Wall], style: &Style, author: &str) -> Vec<Edit> {
    let count = quads.len();
    let names = fresh_names(count, existing);
    let mut quads = quads.to_vec();
    let mut changes = Vec::new();
    // Each run's neighbour at each end, by name: a run's own walls join one another, a loop's
    // last to its first.
    let neighbour = |i: usize, end: End| -> Option<usize> {
        match end {
            End::A if i > 0 => Some(i - 1),
            End::B if i + 1 < count => Some(i + 1),
            End::A if closed && count > 0 => Some(count - 1),
            End::B if closed && count > 0 => Some(0),
            _ => None,
        }
    };
    let mut with: Vec<(String, String)> = (0..count)
        .map(|i| {
            let name = |end| neighbour(i, end).and_then(|j| names.get(j).cloned()).unwrap_or_default();
            (name(End::A), name(End::B))
        })
        .collect();
    // The ends that joined an existing wall (an L, a butt or a T): they are joined too.
    let mut found = vec![(false, false); count];
    let mut used: Vec<usize> = Vec::new();
    if !closed {
        for (i, end) in [(0, End::A), (count.saturating_sub(1), End::B)] {
            let Some(q) = quads.get(i).copied() else { continue };
            let Some(nm) = names.get(i) else { continue };
            let free: Vec<Wall> = existing.iter().filter(|w| !used.contains(&w.index)).cloned().collect();
            if let Some(j) = join(&q, end, &free) {
                used.push(j.partner);
                // Only an end joint names its wall: a T (a gap) runs on through it.
                if !matches!(j.with, With::Gap(..)) {
                    let theirs = free.iter().find(|w| w.index == j.partner).map(|w| w.nm.clone()).unwrap_or_default();
                    if let Some(slot) = with.get_mut(i) {
                        match end {
                            End::A => slot.0 = theirs,
                            End::B => slot.1 = theirs,
                        }
                    }
                }
                if let Some(f) = found.get_mut(i) {
                    match end {
                        End::A => f.0 = true,
                        End::B => f.1 = true,
                    }
                }
                if let Some(slot) = quads.get_mut(i) {
                    *slot = j.quad;
                }
                changes.push(partner_edit(page, j.partner, nm, &j.with));
            }
        }
    }
    // The other way round: an existing wall's free end that lands on one of the new walls joins
    // it too, so which wall was drawn first doesn't matter.
    let mut gaps: Vec<Vec<(u8, f64, f64)>> = vec![Vec::new(); count];
    for w in existing.iter().filter(|w| !w.locked && !w.nm.is_empty() && !used.contains(&w.index)) {
        for (end, already) in [(End::A, w.data.ja), (End::B, w.data.jb)] {
            if already {
                continue;
            }
            // The new walls as they stand: their names, outlines and the ends joined so far.
            let news: Vec<Wall> = (0..count)
                .map(|i| {
                    let f = found.get(i).copied().unwrap_or_default();
                    let data = walls::WallData {
                        ja: f.0 || neighbour(i, End::A).is_some(),
                        jb: f.1 || neighbour(i, End::B).is_some(),
                        ..Default::default()
                    };
                    Wall {
                        index: i,
                        nm: names.get(i).cloned().unwrap_or_default(),
                        quad: quads.get(i).copied().unwrap_or_default(),
                        data,
                        locked: false,
                    }
                })
                .collect();
            let Some(j) = join(&w.quad, end, &news) else { continue };
            let i = j.partner;
            let at_end = !matches!(j.with, With::Gap(..));
            match j.with {
                With::Gap(f, t0, t1) => {
                    if let Some(g) = gaps.get_mut(i) {
                        g.push((f, t0, t1));
                    }
                }
                With::Corner { quad, at_a } => {
                    if let Some(slot) = quads.get_mut(i) {
                        *slot = quad;
                    }
                    joined_to(&mut found, &mut with, i, at_a, &w.nm);
                }
                With::Butt { at_a } => joined_to(&mut found, &mut with, i, at_a, &w.nm),
            }
            let mine_a = end == End::A;
            let name = at_end.then(|| names.get(i).cloned().unwrap_or_default());
            changes.push(Edit::SetWall {
                page,
                index: w.index,
                quad: Some(j.quad),
                ja: mine_a.then_some(true),
                jb: (!mine_a).then_some(true),
                gap: None,
                ja_with: if mine_a { name.clone() } else { None },
                jb_with: if mine_a { None } else { name },
            });
        }
    }
    let flags: Vec<(bool, bool)> = (0..count)
        .map(|i| {
            let f = found.get(i).copied().unwrap_or((false, false));
            (f.0 || neighbour(i, End::A).is_some(), f.1 || neighbour(i, End::B).is_some())
        })
        .collect();
    let adds = quads.into_iter().zip(flags).zip(with).zip(names).zip(gaps).map(|((((quad, (ja, jb)), (ja_with, jb_with)), nm), gaps)| {
        Edit::AddWall { page, quad, ja, jb, style: style.clone(), author: author.to_string(), nm, ja_with, jb_with, gaps }
    });
    changes.extend(adds);
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

    #[test]
    fn an_end_connector_sits_in_the_middle_of_its_end() {
        let q = [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]];
        assert_eq!(end_points(&q), [[0.0, 0.0], [100.0, 0.0]]);
    }

    #[test]
    fn stretching_an_end_moves_its_two_corners_along_the_wall_only() {
        let q = [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]];
        // Dragged to (130, 9): the wall grows to 130 along its length; the sideways part is ignored.
        assert_eq!(stretch_end(&q, 1, [130.0, 9.0]), [[0.0, 3.0], [130.0, 3.0], [130.0, -3.0], [0.0, -3.0]]);
        // The other end, pulled back in: the wall shortens from its start.
        assert_eq!(stretch_end(&q, 0, [20.0, 0.0]), [[20.0, 3.0], [100.0, 3.0], [100.0, -3.0], [20.0, -3.0]]);
    }

    #[test]
    fn a_stretch_that_would_close_the_wall_is_not_made() {
        let q = [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]];
        assert_eq!(stretch_end(&q, 1, [-50.0, 0.0]), q, "the wall keeps its length");
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
    fn joined_walls_name_each_other() {
        // The L of `an_end_at_another_walls_end_makes_an_l`: the new wall, named as it is added,
        // is recorded at the joint on the old wall, and the old wall's name on the new one.
        let old = wall(4, [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]]);
        let new = [[97.0, 0.0], [97.0, 80.0], [103.0, 80.0], [103.0, 0.0]];
        let edits = wall_edits(0, &[new], false, std::slice::from_ref(&old), &style(), "Steve");
        let [Edit::SetWall { index: 4, jb_with: Some(theirs), .. }, Edit::AddWall { nm, ja_with, jb_with, .. }] = edits.as_slice() else {
            panic!("the L names both walls: {edits:?}")
        };
        assert!(!nm.is_empty() && nm != "w4", "a new wall gets its own name: {nm:?}");
        assert_eq!(ja_with, "w4", "the new wall's joint is with the old one");
        assert!(jb_with.is_empty());
        assert_eq!(theirs, nm, "the old wall's joint is with the new one");
    }

    #[test]
    fn a_runs_walls_name_their_neighbours_at_each_corner() {
        let quads = [[[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]], [[100.0, 3.0], [100.0, 100.0], [106.0, 100.0], [106.0, -3.0]]];
        let edits = wall_edits(0, &quads, false, &[], &style(), "Steve");
        let [Edit::AddWall { nm: first, ja_with: a_ja, jb_with: a_jb, .. }, Edit::AddWall { nm: second, ja_with: b_ja, jb_with: b_jb, .. }] =
            edits.as_slice()
        else {
            panic!("two walls: {edits:?}")
        };
        assert!(a_ja.is_empty(), "a free end names nothing");
        assert_eq!(a_jb, second, "the first wall's end B is with the second");
        assert_eq!(b_ja, first, "and the second's end A with the first");
        assert!(b_jb.is_empty());
        assert_ne!(first, second);
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
        // Far from any wall, or parallel beside it, nothing joins.
        let away = [[47.0, 20.0], [47.0, 80.0], [53.0, 80.0], [53.0, 20.0]];
        assert_eq!(wall_edits(0, &[away], false, std::slice::from_ref(&old), &style(), "Steve").len(), 1);
        let beside = [[0.0, 23.0], [100.0, 23.0], [100.0, 17.0], [0.0, 17.0]];
        assert_eq!(wall_edits(0, &[beside], false, &[old], &style(), "Steve").len(), 1);
    }

    #[test]
    fn an_end_a_little_short_of_a_wall_still_joins_it() {
        // The join reaches as far as the walls' thickness, so a freehand end that looks joined
        // (drawn with snapping off) is joined.
        let old = wall(1, [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]]);
        // Drawn down to (50, 7): four points short of the face at y = 3.
        let new = [[47.0, 7.0], [47.0, 80.0], [53.0, 80.0], [53.0, 7.0]];
        let edits = wall_edits(0, &[new], false, std::slice::from_ref(&old), &style(), "Steve");
        let Some(Edit::SetWall { index: 1, gap: Some((0, t0, t1)), .. }) = edits.first() else { panic!("the old wall opens for the T: {edits:?}") };
        assert!((t0 - 0.47).abs() < 1e-9 && (t1 - 0.53).abs() < 1e-9);
        let [(mine, ja, _)] = add_walls(&edits)[..] else { panic!("one new wall") };
        assert!(ja);
        assert!(near(mine[0], [47.0, 3.0]) && near(mine[3], [53.0, 3.0]), "pulled onto the face: {mine:?}");
    }

    #[test]
    fn walls_in_line_butt_join_end_to_end() {
        // A wall carrying on where another stopped — even with a small jog — joins it: the seam
        // closes on the old end's corners and no line is drawn across the butt.
        let old = wall(4, [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]]);
        let new = [[102.0, 5.0], [200.0, 5.0], [200.0, -1.0], [102.0, -1.0]];
        let edits = wall_edits(0, &[new], false, std::slice::from_ref(&old), &style(), "Steve");
        let [
            Edit::SetWall { index: 4, quad: None, ja: None, jb: Some(true), gap: None, jb_with: Some(theirs), .. },
            Edit::AddWall { quad, ja: true, jb: false, nm, ja_with, .. },
        ] = edits.as_slice()
        else {
            panic!("a butt joint: {edits:?}")
        };
        assert_eq!(theirs, nm, "the old wall knows its continuation");
        assert_eq!(ja_with, "w4");
        assert!(near(quad[0], [100.0, 3.0]) && near(quad[3], [100.0, -3.0]), "the seam closes on the old end: {quad:?}");
        // One drawn back over the other doesn't join.
        let over = [[100.0, 3.0], [0.0, 3.0], [0.0, -3.0], [100.0, -3.0]];
        let edits = wall_edits(0, &[over], false, &[old], &style(), "Steve");
        assert!(edits.iter().all(|e| !matches!(e, Edit::SetWall { .. })), "{edits:?}");
    }

    #[test]
    fn drawing_across_a_free_end_heals_it() {
        // A wall drawn across an existing wall's free end joins that end too: the order the
        // walls were drawn in doesn't matter.
        let old = wall(2, [[47.0, 0.0], [47.0, 60.0], [53.0, 60.0], [53.0, 0.0]]);
        // A new wall straight across above it, centred on y = 60.
        let new = [[0.0, 63.0], [100.0, 63.0], [100.0, 57.0], [0.0, 57.0]];
        let edits = wall_edits(0, &[new], false, std::slice::from_ref(&old), &style(), "Steve");
        let [Edit::SetWall { index: 2, quad: Some(theirs), jb: Some(true), gap: None, jb_with: None, .. }, Edit::AddWall { gaps, .. }] =
            edits.as_slice()
        else {
            panic!("the old wall's end is healed: {edits:?}")
        };
        assert!(near(theirs[1], [47.0, 57.0]) && near(theirs[2], [53.0, 57.0]), "cut back to the face it meets: {theirs:?}");
        let [(face, t0, t1)] = gaps[..] else { panic!("the new wall opens for it: {gaps:?}") };
        assert_eq!(face, 1, "the face toward the old wall");
        assert!((t0 - 0.47).abs() < 1e-9 && (t1 - 0.53).abs() < 1e-9, "{t0} {t1}");
    }
}
