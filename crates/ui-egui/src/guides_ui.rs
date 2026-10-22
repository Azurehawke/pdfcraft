//! RevuCraft: guides on screen, and the printable area. A guide is kept with its sheet in view
//! space ([`pdfcraft_engine::guides`]); here it is a line across the sheet's part of the view,
//! dragged out of a ruler, moved with the Select tool (dropped back on a ruler to remove it), its
//! position typed after a double-click, and snapped to by the drafting and markup tools.

use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2};
use pdfcraft_engine::guides::Guides;

use crate::canvas::PageXform;

/// How close (screen pixels) the pointer snaps to a guide, and picks one up.
pub(crate) const SNAP_PX: f32 = 6.0;

/// Guides' colour, as Illustrator's.
pub(crate) const COLOR: Color32 = Color32::from_rgb(0x2E, 0xB8, 0xF0);

/// One guide on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScreenGuide {
    /// Upright on screen (a vertical line).
    pub vertical: bool,
    /// Where: x for a vertical line, y for a horizontal one.
    pub at: f32,
    /// Which list it is in ([`Guides::vertical`] or [`Guides::horizontal`]), and where.
    pub view_vertical: bool,
    pub index: usize,
}

/// Whether the view shows the sheet's view-space axes turned a quarter.
fn quarter(xf: &PageXform) -> bool {
    !xf.rot.is_multiple_of(180)
}

/// A view-space guide on screen.
pub(crate) fn to_screen(xf: &PageXform, view_vertical: bool, v: f64, index: usize) -> ScreenGuide {
    let v = v as f32;
    let p = if view_vertical { xf.norm_to_screen(v / xf.pw.max(1.0), 0.0) } else { xf.norm_to_screen(0.0, v / xf.ph.max(1.0)) };
    let vertical = view_vertical != quarter(xf);
    ScreenGuide { vertical, at: if vertical { p.x } else { p.y }, view_vertical, index }
}

/// A line on screen (vertical at x = `at`, else horizontal at y = `at`) as a view-space guide.
pub(crate) fn from_screen(xf: &PageXform, vertical: bool, at: f32) -> (bool, f64) {
    let c = xf.rect.center();
    let p = if vertical { pos2(at, c.y) } else { pos2(c.x, at) };
    let (vx, vy) = xf.screen_to_view(p);
    let view_vertical = vertical != quarter(xf);
    (view_vertical, f64::from(if view_vertical { vx } else { vy }))
}

/// All of a sheet's guides on screen.
pub(crate) fn on_screen(g: &Guides, xf: &PageXform) -> Vec<ScreenGuide> {
    let v = g.vertical.iter().enumerate().map(|(i, x)| to_screen(xf, true, *x, i));
    let h = g.horizontal.iter().enumerate().map(|(i, y)| to_screen(xf, false, *y, i));
    v.chain(h).filter(|s| s.at.is_finite()).collect()
}

/// `p` moved onto the nearest guide each way within [`SNAP_PX`] (onto both where two cross);
/// `None` when no guide is that close.
pub(crate) fn snap(guides: &[ScreenGuide], p: Pos2) -> Option<Pos2> {
    let nearest = |vertical: bool, at: f32| {
        guides
            .iter()
            .filter(|g| g.vertical == vertical)
            .map(|g| (g.at, (g.at - at).abs()))
            .filter(|(_, d)| *d <= SNAP_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|g| g.0)
    };
    let (x, y) = (nearest(true, p.x), nearest(false, p.y));
    (x.is_some() || y.is_some()).then(|| pos2(x.unwrap_or(p.x), y.unwrap_or(p.y)))
}

/// A guide's line across `zone`.
pub(crate) fn line(vertical: bool, at: f32, zone: Rect) -> [Pos2; 2] {
    if vertical { [pos2(at, zone.min.y), pos2(at, zone.max.y)] } else { [pos2(zone.min.x, at), pos2(zone.max.x, at)] }
}

/// Paints the guides across their sheet's `zone`.
pub(crate) fn paint(painter: &Painter, guides: &[ScreenGuide], zone: Rect, locked: bool) {
    let stroke = Stroke::new(1.0, if locked { COLOR.gamma_multiply(0.7) } else { COLOR });
    let p = painter.with_clip_rect(zone.intersect(painter.clip_rect()));
    for g in guides {
        let [a, b] = line(g.vertical, g.at.round() + 0.5, zone);
        p.line_segment([a, b], stroke);
    }
}

/// Paints the printable area: `inches` in from each edge of the sheet, dashed.
pub(crate) fn paint_margin(painter: &Painter, xf: &PageXform, inches: f32) {
    if !(inches.is_finite() && inches > 0.0) {
        return;
    }
    let (w, h) = if quarter(xf) { (xf.ph, xf.pw) } else { (xf.pw, xf.ph) };
    let px = (xf.rect.width() / w.max(1.0) + xf.rect.height() / h.max(1.0)) / 2.0;
    let inset = inches * 72.0 * px;
    let r = xf.rect.shrink(inset);
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    let stroke = Stroke::new(1.0, Color32::from_rgb(0xE0, 0x4E, 0x8A));
    let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    painter.extend(egui::Shape::dashed_line(&corners, stroke, 6.0, 4.0));
}

/// The printable-area choices, in inches (0 is off).
pub(crate) const MARGINS: [(f32, &str); 6] = [(0.0, "Off"), (0.125, "1/8\""), (0.1875, "3/16\""), (0.25, "1/4\""), (0.375, "3/8\""), (0.5, "1/2\"")];

#[cfg(test)]
mod tests {
    use super::*;

    fn xf(rot: u16) -> PageXform {
        // A 612 × 792 sheet at 1 px a point, at (100, 50); turned sideways at 90° and 270°.
        let (w, h) = if rot.is_multiple_of(180) { (612.0, 792.0) } else { (792.0, 612.0) };
        PageXform { rect: Rect::from_min_size(pos2(100.0, 50.0), egui::vec2(w, h)), rot, pw: 612.0, ph: 792.0 }
    }

    #[test]
    fn guides_go_to_the_screen_and_back_at_every_rotation() {
        for rot in [0, 90, 180, 270] {
            let xf = xf(rot);
            for (vv, v) in [(true, 36.0), (false, 500.0), (true, -40.0)] {
                let s = to_screen(&xf, vv, v, 0);
                let (back_vv, back) = from_screen(&xf, s.vertical, s.at);
                assert_eq!(back_vv, vv, "rot {rot}");
                assert!((back - v).abs() < 1e-3, "rot {rot}: {v} came back as {back}");
            }
        }
        let s = to_screen(&xf(0), true, 36.0, 3);
        assert!(s.vertical && (s.at - 136.0).abs() < 1e-3 && s.index == 3);
        assert!(!to_screen(&xf(90), true, 36.0, 0).vertical, "a vertical guide lies down when the view turns");
    }

    #[test]
    fn points_snap_to_guides_and_crossings() {
        let g = [
            ScreenGuide { vertical: true, at: 200.0, view_vertical: true, index: 0 },
            ScreenGuide { vertical: false, at: 300.0, view_vertical: false, index: 0 },
        ];
        assert_eq!(snap(&g, pos2(204.0, 100.0)), Some(pos2(200.0, 100.0)));
        assert_eq!(snap(&g, pos2(197.0, 302.0)), Some(pos2(200.0, 300.0)));
        assert_eq!(snap(&g, pos2(250.0, 250.0)), None);
        assert_eq!(snap(&[], pos2(1.0, 1.0)), None);
        // Garbage doesn't panic.
        let _ = snap(&g, pos2(f32::NAN, f32::INFINITY));
    }
}
