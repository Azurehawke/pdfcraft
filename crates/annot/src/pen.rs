//! PdfCraft's freehand pens: drawings whose width follows the pen's pressure (and, for flat nibs,
//! the direction of the stroke), like a fountain pen, a brush or a calligraphy nib.
//!
//! A pen drawing is still a standard `/Ink` annotation: `/InkList` holds the centre lines and
//! `/BS /W` the nominal width, which is what another app falls back on if it redraws the drawing
//! itself. PdfCraft adds `/PCPen` (the pen's name) and `/PCPressure` (per stroke, one 0–1 value
//! per point of `/InkList`), and its appearance is the strokes' filled outlines, which any viewer
//! shows exactly as drawn.

use crate::appearance::smooth_segments;
use crate::n;

/// The pens of the Draw tool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PenKind {
    /// A ballpoint: nearly even, a little thicker when pressed.
    #[default]
    Pen,
    /// Thin to thick with pressure.
    Fountain,
    /// A flat nib held at 45°: thick across the nib, hairline along it.
    Calligraphy,
    /// A soft round brush: a wide range of widths, tapering ends.
    Brush,
    /// Fine and slightly translucent.
    Pencil,
    /// A wide flat chisel, translucent, multiplied over the page.
    Highlighter,
}

impl PenKind {
    pub const ALL: [PenKind; 6] = [PenKind::Pen, PenKind::Fountain, PenKind::Calligraphy, PenKind::Brush, PenKind::Pencil, PenKind::Highlighter];

    /// The name in `/PCPen` and in the settings.
    pub fn name(self) -> &'static str {
        match self {
            PenKind::Pen => "Pen",
            PenKind::Fountain => "Fountain",
            PenKind::Calligraphy => "Calligraphy",
            PenKind::Brush => "Brush",
            PenKind::Pencil => "Pencil",
            PenKind::Highlighter => "Highlighter",
        }
    }

    pub fn from_name(name: &[u8]) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name().as_bytes() == name)
    }

    /// What the pen picker calls it.
    pub fn label(self) -> &'static str {
        match self {
            PenKind::Pen => "Pen",
            PenKind::Fountain => "Fountain pen",
            PenKind::Calligraphy => "Calligraphy pen",
            PenKind::Brush => "Brush",
            PenKind::Pencil => "Pencil",
            PenKind::Highlighter => "Highlighter",
        }
    }

    /// The comment's `/Subj` (shown as its type in other viewers).
    pub fn subject(self) -> &'static str {
        match self {
            PenKind::Highlighter => "Highlighter",
            _ => "Pencil",
        }
    }

    /// The pen's opacity relative to the chosen one.
    pub fn opacity_factor(self) -> f64 {
        match self {
            PenKind::Highlighter => 0.45,
            PenKind::Pencil => 0.85,
            _ => 1.0,
        }
    }

    /// Drawn with the Multiply blend mode, so the page shows through as with a real highlighter.
    pub fn multiply(self) -> bool {
        self == PenKind::Highlighter
    }

    /// A flat nib's direction (radians from the x axis); `None` for round tips.
    fn nib(self) -> Option<f64> {
        match self {
            PenKind::Calligraphy => Some(std::f64::consts::FRAC_PI_4),
            PenKind::Highlighter => Some(std::f64::consts::FRAC_PI_2),
            _ => None,
        }
    }

    /// The full width (or nib length) at pressure `p` (0–1) for nominal width `w`.
    pub fn width_at(self, w: f64, p: f64) -> f64 {
        let p = if p.is_finite() { p.clamp(0.0, 1.0) } else { DEFAULT_PRESSURE };
        let w = if w.is_finite() { w.clamp(0.1, 50.0) } else { 1.0 };
        match self {
            PenKind::Pen => w * (0.8 + 0.3 * p),
            PenKind::Fountain => w * (0.25 + 1.35 * p),
            PenKind::Calligraphy => w * 2.6 * (0.65 + 0.35 * p),
            PenKind::Brush => w * (0.15 + 2.6 * p.powf(1.4)),
            PenKind::Pencil => w * (0.55 + 0.25 * p),
            PenKind::Highlighter => w * 5.0,
        }
    }

    /// The widest the pen draws at nominal width `w`.
    pub fn max_width(self, w: f64) -> f64 {
        self.width_at(w, 1.0)
    }
}

/// The pressure of a point that has none (a mouse, or a stroke whose pressures don't line up
/// with its points): a medium press.
pub const DEFAULT_PRESSURE: f64 = 0.55;

/// Bézier circle constant: control points of a quarter circle at this fraction of the radius.
const KAPPA: f64 = 0.552_284_75;

fn add(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 + b.0, a.1 + b.1)
}

fn sub(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 - b.0, a.1 - b.1)
}

fn scale(a: (f64, f64), s: f64) -> (f64, f64) {
    (a.0 * s, a.1 * s)
}

fn unit(a: (f64, f64)) -> Option<(f64, f64)> {
    let len = a.0.hypot(a.1);
    (len.is_finite() && len > 1e-9).then(|| (a.0 / len, a.1 / len))
}

fn pt(p: (f64, f64)) -> String {
    format!("{} {}", n(p.0), n(p.1))
}

/// Pressures smoothed a little (a pen reports small jitters), one per point; missing or invalid
/// values are a medium press.
fn smoothed(pressure: &[f64], len: usize) -> Vec<f64> {
    let raw: Vec<f64> =
        (0..len).map(|i| pressure.get(i).copied().filter(|p| p.is_finite()).map_or(DEFAULT_PRESSURE, |p| p.clamp(0.0, 1.0))).collect();
    (0..len)
        .map(|i| {
            let at = |j: usize| raw.get(j).copied().unwrap_or(DEFAULT_PRESSURE);
            let (prev, next) = (at(i.saturating_sub(1)), if i + 1 < len { at(i + 1) } else { at(i) });
            (prev + 2.0 * at(i) + next) / 4.0
        })
        .collect()
}

/// The filled outline of one stroke through `pts` drawn with `pen` at nominal width `w`, as path
/// operators ending in a fill (`f`, nonzero). Empty when there is nothing to draw.
pub fn outline(pts: &[(f64, f64)], pressure: &[f64], pen: PenKind, w: f64) -> String {
    let pts: Vec<(f64, f64)> = pts.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    let Some(&first) = pts.first() else { return String::new() };
    let press = smoothed(pressure, pts.len());
    let half: Vec<f64> = press.iter().map(|p| pen.width_at(w, *p) / 2.0).collect();
    let h0 = half.first().copied().unwrap_or(w / 2.0);
    if pts.len() == 1 {
        // A dot.
        return format!("{}f\n", circle(first, h0));
    }
    // Directions along the stroke at each point (the chord between its neighbours); points
    // where the pen hadn't moved yet take the first direction it moved in.
    let chord = |i: usize| unit(sub(pts[(i + 1).min(pts.len() - 1)], pts[i.saturating_sub(1)]));
    let mut last_dir = (0..pts.len()).find_map(chord).unwrap_or((1.0, 0.0));
    let dirs: Vec<(f64, f64)> = (0..pts.len())
        .map(|i| {
            if let Some(d) = chord(i) {
                last_dir = d;
            }
            last_dir
        })
        .collect();
    // Each point's offset to the left edge: along the normal for round tips, along the nib for
    // flat ones (so the width follows the stroke's direction).
    let nib = pen.nib().map(|a| (a.cos(), a.sin()));
    let offs: Vec<(f64, f64)> = dirs
        .iter()
        .zip(&half)
        .map(|(d, h)| match nib {
            Some(v) => scale(v, *h),
            None => scale((-d.1, d.0), *h),
        })
        .collect();
    let segs = smooth_segments(&pts);
    let mut c = String::new();
    let left = |i: usize| add(pts[i], offs[i]);
    let right = |i: usize| sub(pts[i], offs[i]);
    c.push_str(&format!("{} m\n", pt(left(0))));
    for (k, [c1, c2, _]) in segs.iter().enumerate() {
        c.push_str(&format!("{} {} {} c\n", pt(add(*c1, offs[k])), pt(add(*c2, offs[k + 1])), pt(left(k + 1))));
    }
    let end = pts.len() - 1;
    match nib {
        // A flat nib ends square.
        Some(_) => c.push_str(&format!("{} l\n", pt(right(end)))),
        None => c.push_str(&round_cap(pts[end], offs[end], dirs[end], half[end])),
    }
    for (k, [c1, c2, _]) in segs.iter().enumerate().rev() {
        c.push_str(&format!("{} {} {} c\n", pt(sub(*c2, offs[k + 1])), pt(sub(*c1, offs[k])), pt(right(k))));
    }
    match nib {
        Some(_) => c.push_str("h\n"),
        None => c.push_str(&round_cap(pts[0], scale(offs[0], -1.0), scale(dirs[0], -1.0), half[0])),
    }
    c.push_str("f\n");
    c
}

/// Two quarter circles from `p + off` around the tip ahead (`dir`) to `p - off`, radius `r`.
fn round_cap(p: (f64, f64), off: (f64, f64), dir: (f64, f64), r: f64) -> String {
    let ahead = scale(dir, r);
    let (a, b, z) = (add(p, off), add(p, ahead), sub(p, off));
    format!(
        "{} {} {} c\n{} {} {} c\n",
        pt(add(a, scale(ahead, KAPPA))),
        pt(add(b, scale(off, KAPPA))),
        pt(b),
        pt(sub(b, scale(off, KAPPA))),
        pt(add(z, scale(ahead, KAPPA))),
        pt(z)
    )
}

/// A closed circle path (no paint operator).
fn circle(c: (f64, f64), r: f64) -> String {
    let k = r * KAPPA;
    let (x, y) = c;
    format!(
        "{} {} m\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\n{} {} {} {} {} {} c\nh\n",
        n(x + r),
        n(y),
        n(x + r),
        n(y + k),
        n(x + k),
        n(y + r),
        n(x),
        n(y + r),
        n(x - k),
        n(y + r),
        n(x - r),
        n(y + k),
        n(x - r),
        n(y),
        n(x - r),
        n(y - k),
        n(x - k),
        n(y - r),
        n(x),
        n(y - r),
        n(x + k),
        n(y - r),
        n(x + r),
        n(y - k),
        n(x + r),
        n(y)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_widens_a_fountain_pen() {
        assert!(PenKind::Fountain.width_at(2.0, 1.0) > 3.0 * PenKind::Fountain.width_at(2.0, 0.0));
        assert!((PenKind::Highlighter.width_at(2.0, 0.0) - PenKind::Highlighter.width_at(2.0, 1.0)).abs() < 1e-9);
    }

    #[test]
    fn names_round_trip() {
        for p in PenKind::ALL {
            assert_eq!(PenKind::from_name(p.name().as_bytes()), Some(p));
        }
        assert_eq!(PenKind::from_name(b"Crayon"), None);
    }

    #[test]
    fn outlines_are_closed_fills_for_any_input() {
        let line = [(0.0, 0.0), (10.0, 5.0), (20.0, 0.0), (20.0, 0.0), (30.0, 10.0)];
        for pen in PenKind::ALL {
            for pts in [&line[..], &line[..1], &line[..2], &[(5.0, 5.0), (5.0, 5.0)][..]] {
                for pressure in [&[][..], &[0.1, 0.9][..], &[f64::NAN; 5][..]] {
                    let c = outline(pts, pressure, pen, 2.0);
                    assert!(c.ends_with("f\n"), "{pen:?}: {c}");
                    assert!(!c.contains("NaN") && !c.contains("inf"), "{pen:?}: {c}");
                }
            }
        }
        assert!(outline(&[], &[], PenKind::Pen, 2.0).is_empty());
    }
}
