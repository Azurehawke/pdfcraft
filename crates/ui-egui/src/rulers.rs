//! RevuCraft: rulers along the top and left of the drawing, as Illustrator's. A marker on each
//! follows the pointer. They read in the sheet's drawing scale (feet and inches at 1/8" = 1'-0"),
//! or in paper inches on a sheet without one, from zero at the sheet's top-left corner, and their
//! ticks thin out or fill in with the zoom.

use egui::{Align2, Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

use crate::theme::{self, Tokens};

/// The rulers' thickness on screen.
pub(crate) const SIZE: f32 = 18.0;

/// What the rulers count in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Unit {
    Feet,
    Inches,
    /// Any other unit, by name (`m`, `mm`, `yd`…).
    Other(String),
}

/// The rulers' units: how many of them one PDF point spans, and which.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Units {
    pub per_point: f64,
    pub unit: Unit,
}

impl Units {
    /// Paper inches: a sheet without a drawing scale.
    pub(crate) fn paper() -> Self {
        Units { per_point: 1.0 / 72.0, unit: Unit::Inches }
    }

    /// The sheet's drawing scale, else paper inches.
    pub(crate) fn of(scale: Option<&pdfcraft_engine::measure::Scale>) -> Self {
        let Some(s) = scale else { return Self::paper() };
        let per_point = s.x * s.distance_factor;
        if !per_point.is_finite() || per_point <= 0.0 {
            return Self::paper();
        }
        let unit = match s.unit.trim().to_ascii_lowercase().as_str() {
            "ft" | "feet" | "foot" | "'" => Unit::Feet,
            "in" | "inch" | "inches" | "\"" => Unit::Inches,
            other => Unit::Other(other.to_string()),
        };
        Units { per_point, unit }
    }

    /// The short name shown where the rulers meet.
    pub(crate) fn name(&self) -> &str {
        match &self.unit {
            Unit::Feet => "ft",
            Unit::Inches => "in",
            Unit::Other(u) => u,
        }
    }
}

/// The steps the labelled ticks may take, smallest first, and how a step divides into minor
/// ticks.
fn steps(unit: &Unit) -> Vec<(f64, &'static [u32])> {
    let mut out: Vec<(f64, &'static [u32])> = match unit {
        Unit::Feet => vec![(1.0 / 12.0, &[4, 2][..]), (2.0 / 12.0, &[2][..]), (3.0 / 12.0, &[3][..]), (0.5, &[6, 2][..]), (1.0, &[12, 4, 2][..])],
        Unit::Inches => vec![(1.0 / 16.0, &[][..]), (1.0 / 8.0, &[2][..]), (0.25, &[2][..]), (0.5, &[4, 2][..]), (1.0, &[16, 8, 4, 2][..])],
        Unit::Other(_) => Vec::new(),
    };
    let first = if out.is_empty() { -4 } else { 0 };
    for k in first..=7 {
        let p = 10f64.powi(k);
        let decimal_steps: [(f64, &'static [u32]); 3] = [(1.0, &[10, 5, 2]), (2.0, &[4, 2]), (5.0, &[5])];
        for (m, subs) in decimal_steps {
            let v = m * p;
            if out.last().is_none_or(|l| v > l.0 * 1.0001) {
                out.push((v, subs));
            }
        }
    }
    out
}

/// The labelled step (in units) and its number of minor divisions, for `px_per_unit` pixels a
/// unit: labels at least `MIN_LABEL` pixels apart, minor ticks at least `MIN_TICK`.
pub(crate) fn step(unit: &Unit, px_per_unit: f64) -> (f64, u32) {
    const MIN_LABEL: f64 = 64.0;
    const MIN_TICK: f64 = 5.0;
    let all = steps(unit);
    let px = if px_per_unit.is_finite() && px_per_unit > 0.0 { px_per_unit } else { 1.0 };
    let (s, subs) = all.iter().find(|(s, _)| s * px >= MIN_LABEL).or(all.last()).copied().unwrap_or((1.0, &[][..]));
    let n = subs.iter().copied().find(|n| s * px / f64::from(*n) >= MIN_TICK).unwrap_or(1);
    (s, n)
}

/// `n/d` for a fraction of an inch to sixteenths, else `None`.
fn fraction(v: f64) -> Option<String> {
    let sixteenths = (v * 16.0).round();
    if (v * 16.0 - sixteenths).abs() > 1e-6 || sixteenths <= 0.0 || sixteenths >= 16.0 {
        return None;
    }
    let mut n = sixteenths as u32;
    let mut d = 16u32;
    while n % 2 == 0 && d > 1 {
        n /= 2;
        d /= 2;
    }
    Some(format!("{n}/{d}"))
}

/// A decimal with trailing zeros dropped.
fn decimal(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// A tick's label.
pub(crate) fn label(v: f64, unit: &Unit) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let sign = if v < 0.0 { "-" } else { "" };
    let a = v.abs();
    match unit {
        Unit::Feet => {
            let inches = (a * 12.0 * 16.0).round() / 16.0;
            let feet = (inches / 12.0).floor();
            let rest = inches - feet * 12.0;
            let rest_text = match (rest.floor(), fraction(rest - rest.floor())) {
                (w, Some(f)) if w > 0.0 => format!("{w} {f}"),
                (_, Some(f)) => f,
                (w, None) => decimal(w),
            };
            if rest < 1e-9 {
                format!("{sign}{feet}'")
            } else if feet < 1.0 {
                format!("{sign}{rest_text}\"")
            } else {
                format!("{sign}{feet}'-{rest_text}\"")
            }
        }
        Unit::Inches => {
            let whole = a.floor();
            match fraction(a - whole) {
                Some(f) if whole > 0.0 => format!("{sign}{whole} {f}"),
                Some(f) => format!("{sign}{f}"),
                None => format!("{sign}{}", decimal(a)),
            }
        }
        Unit::Other(_) => format!("{sign}{}", decimal(a)),
    }
}

/// One ruler: where zero is on screen (along its axis), pixels a unit, and the sheet's extent.
pub(crate) struct Axis {
    pub zero: f32,
    pub px_per_unit: f64,
    pub sheet: (f32, f32),
}

/// Paints both rulers in `full` (the drawing area with the rulers' strips along its top and left).
/// `pointer` puts a marker on each; `x`/`y` are the sheet's axes, `None` with no sheet in view.
pub(crate) fn paint(painter: &Painter, t: &Tokens, full: Rect, units: &Units, x: Option<&Axis>, y: Option<&Axis>, pointer: Option<Pos2>) {
    let top = Rect::from_min_max(pos2(full.min.x + SIZE, full.min.y), pos2(full.max.x, full.min.y + SIZE));
    let left = Rect::from_min_max(pos2(full.min.x, full.min.y + SIZE), pos2(full.min.x + SIZE, full.max.y));
    let corner = Rect::from_min_size(full.min, vec2(SIZE, SIZE));
    let back = t.card;
    let sheet_band = if t.dark() { Color32::from_white_alpha(14) } else { Color32::from_black_alpha(10) };
    let ink = t.text_faint;
    let font = theme::regular(9.5);
    for strip in [top, left, corner] {
        painter.rect_filled(strip, 0.0, back);
    }
    painter.line_segment([pos2(top.min.x, top.max.y - 0.5), pos2(top.max.x, top.max.y - 0.5)], Stroke::new(1.0, t.divider));
    painter.line_segment([pos2(left.max.x - 0.5, left.min.y), pos2(left.max.x - 0.5, left.max.y)], Stroke::new(1.0, t.divider));
    painter.line_segment([pos2(corner.min.x, corner.max.y - 0.5), pos2(corner.max.x, corner.max.y - 0.5)], Stroke::new(1.0, t.divider));
    painter.line_segment([pos2(corner.max.x - 0.5, corner.min.y), pos2(corner.max.x - 0.5, corner.max.y)], Stroke::new(1.0, t.divider));
    painter.text(corner.center(), Align2::CENTER_CENTER, units.name(), theme::regular(8.5), ink);

    for (horizontal, strip, axis) in [(true, top, x), (false, left, y)] {
        let Some(axis) = axis else { continue };
        let p = painter.with_clip_rect(strip);
        let (lo, hi) = if horizontal { (strip.min.x, strip.max.x) } else { (strip.min.y, strip.max.y) };
        // The sheet's extent, lighter.
        let (s0, s1) = (axis.sheet.0.max(lo), axis.sheet.1.min(hi));
        if s1 > s0 {
            let band = if horizontal {
                Rect::from_min_max(pos2(s0, strip.min.y), pos2(s1, strip.max.y))
            } else {
                Rect::from_min_max(pos2(strip.min.x, s0), pos2(strip.max.x, s1))
            };
            p.rect_filled(band, 0.0, sheet_band);
        }
        let (step, subs) = step(&units.unit, axis.px_per_unit);
        let minor = step / f64::from(subs.max(1));
        let minor_px = minor * axis.px_per_unit;
        if !minor_px.is_finite() || minor_px < 1.0 {
            continue;
        }
        let first = ((f64::from(lo - axis.zero)) / minor_px).floor();
        let last = ((f64::from(hi - axis.zero)) / minor_px).ceil();
        if !first.is_finite() || !last.is_finite() || last - first > 4000.0 {
            continue;
        }
        let mut k = first as i64;
        let end = last as i64;
        while k <= end {
            let at = axis.zero + (k as f64 * minor_px) as f32;
            let i = k.rem_euclid(i64::from(subs.max(1)));
            let len = if i == 0 {
                SIZE
            } else if subs % 2 == 0 && i == i64::from(subs / 2) {
                SIZE * 0.45
            } else {
                SIZE * 0.25
            };
            let stroke = Stroke::new(1.0, ink.gamma_multiply(if i == 0 { 0.9 } else { 0.6 }));
            if horizontal {
                p.line_segment([pos2(at + 0.5, strip.max.y - len), pos2(at + 0.5, strip.max.y)], stroke);
            } else {
                p.line_segment([pos2(strip.max.x - len, at + 0.5), pos2(strip.max.x, at + 0.5)], stroke);
            }
            if i == 0 {
                let text = label(k as f64 * minor, &units.unit);
                if horizontal {
                    p.text(pos2(at + 3.0, strip.min.y + 1.0), Align2::LEFT_TOP, text, font.clone(), ink);
                } else {
                    let galley = p.layout_no_wrap(text, font.clone(), ink);
                    // Read bottom to top, as Illustrator's.
                    let shape = egui::epaint::TextShape::new(pos2(strip.min.x + 1.0, at - 3.0), galley, ink).with_angle(-std::f32::consts::FRAC_PI_2);
                    p.add(shape);
                }
            }
            k = k.saturating_add(1);
        }
    }
    // The pointer's markers.
    if let Some(ptr) = pointer {
        let accent = Stroke::new(1.0, t.accent);
        if ptr.x >= top.min.x && ptr.x <= top.max.x {
            painter.line_segment([pos2(ptr.x, top.min.y), pos2(ptr.x, top.max.y)], accent);
        }
        if ptr.y >= left.min.y && ptr.y <= left.max.y {
            painter.line_segment([pos2(left.min.x, ptr.y), pos2(left.max.x, ptr.y)], accent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_the_zoom() {
        // 1/8" = 1'-0" at 100%: a foot is 9 points, 12 pixels.
        let (s, n) = step(&Unit::Feet, 12.0);
        assert_eq!(s, 10.0);
        assert!(n >= 1);
        // Zoomed far in: inches.
        let (s, _) = step(&Unit::Feet, 2000.0);
        assert!(s < 1.0);
        // Paper inches at 100%: 96 pixels an inch.
        assert_eq!(step(&Unit::Inches, 96.0).0, 1.0);
        assert_eq!(step(&Unit::Other("mm".into()), 3.78).0, 20.0);
        // Garbage in: still a step.
        let (s, n) = step(&Unit::Feet, f64::NAN);
        assert!(s.is_finite() && s > 0.0 && n >= 1);
        let (s, _) = step(&Unit::Inches, 0.0);
        assert!(s > 0.0);
    }

    #[test]
    fn labels_read_as_on_a_scale() {
        assert_eq!(label(10.0, &Unit::Feet), "10'");
        assert_eq!(label(0.5, &Unit::Feet), "6\"");
        assert_eq!(label(1.5, &Unit::Feet), "1'-6\"");
        assert_eq!(label(-5.0, &Unit::Feet), "-5'");
        assert_eq!(label(0.0, &Unit::Feet), "0'");
        assert_eq!(label(0.25, &Unit::Inches), "1/4");
        assert_eq!(label(2.5, &Unit::Inches), "2 1/2");
        assert_eq!(label(3.0, &Unit::Inches), "3");
        assert_eq!(label(0.2, &Unit::Other("m".into())), "0.2");
        assert_eq!(label(f64::INFINITY, &Unit::Feet), "");
    }

    #[test]
    fn units_come_from_the_scale() {
        assert_eq!(Units::of(None), Units::paper());
        let s = pdfcraft_engine::measure::Scale { x: 1.0 / 9.0, unit: "ft".into(), ..Default::default() };
        let u = Units::of(Some(&s));
        assert_eq!(u.unit, Unit::Feet);
        assert!((u.per_point - 1.0 / 9.0).abs() < 1e-12);
        let bad = pdfcraft_engine::measure::Scale { x: f64::NAN, ..Default::default() };
        assert_eq!(Units::of(Some(&bad)), Units::paper());
    }
}
