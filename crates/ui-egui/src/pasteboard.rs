//! RevuCraft: the pasteboard, as Illustrator's and VectorCraft's: the dark area around each
//! sheet is part of the drawing. A line or shape that runs off the sheet shows there, faded, and
//! can still be selected, moved, grip-edited, snapped to and drawn on; the sheet stays the print
//! border (what is off it doesn't print).
//!
//! Two parts: the pointer zone of each page (the sheet and the area around it, up to halfway to
//! the next sheet, within the view), which the markup, drafting and Select tools take input in;
//! and the off-sheet parts of the markups, painted under the sheet so the sheet hides the rest.

use std::collections::HashMap;

use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2};
use pdfcraft_engine::Document;
use pdfcraft_engine::geometry::{Geometry, Kind};
use pdfcraft_render::DocInfo;

use crate::canvas::PageXform;

/// Each page's lines, polylines and polygons, read once per edit.
#[derive(Default)]
pub(crate) struct Cache {
    generation: u64,
    pages: HashMap<usize, Vec<Geometry>>,
}

impl Cache {
    fn shapes(&mut self, doc: &Document, page: usize) -> &[Geometry] {
        let generation = doc.edit_generation();
        if generation != self.generation {
            self.pages.clear();
            self.generation = generation;
        }
        self.pages.entry(page).or_insert_with(|| doc.markup_geometry(page))
    }
}

/// The part of the view where the pointer works on the page at `r`: the page, and the
/// pasteboard around it up to halfway to each neighbouring page (`others`), within `bound`.
pub(crate) fn zone(r: Rect, others: impl IntoIterator<Item = Rect>, bound: Rect) -> Rect {
    let mut z = bound;
    for o in others {
        if o == r {
            continue;
        }
        if o.min.y >= r.max.y {
            z.max.y = z.max.y.min((r.max.y + o.min.y) / 2.0);
        } else if o.max.y <= r.min.y {
            z.min.y = z.min.y.max((o.max.y + r.min.y) / 2.0);
        } else if o.min.x >= r.max.x {
            z.max.x = z.max.x.min((r.max.x + o.min.x) / 2.0);
        } else if o.max.x <= r.min.x {
            z.min.x = z.min.x.max((o.max.x + r.min.x) / 2.0);
        }
    }
    // The page itself always counts (as much of it as is in view).
    z.union(r.intersect(bound))
}

/// Relative luminance (0 to 1) of an sRGB colour, roughly.
fn luminance(c: Color32) -> f32 {
    (0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b())) / 255.0
}

/// The markup's colour as drawn on the pasteboard: faded, and lifted (or darkened) where it
/// would vanish against it (black walls on a dark pasteboard).
fn shown(color: Option<[f32; 3]>, pasteboard: Color32) -> Color32 {
    let byte = |v: f32| if v.is_finite() { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 };
    let c = color.map_or(Color32::from_rgb(0xD0, 0x30, 0x30), |[r, g, b]| Color32::from_rgb(byte(r), byte(g), byte(b)));
    let back = luminance(pasteboard);
    let c = if (luminance(c) - back).abs() >= 0.3 {
        c
    } else if back < 0.5 {
        Color32::from_gray(0xC8)
    } else {
        Color32::from_gray(0x50)
    };
    c.gamma_multiply(0.75)
}

/// Subtypes with nothing of their own to show off the page.
fn skipped(subtype: &str) -> bool {
    matches!(subtype, "Popup" | "Link" | "Widget" | "Highlight" | "Underline" | "StrikeOut" | "Squiggly" | "Redact")
}

/// Paints the off-page parts of page `page`'s markups. Call before the page is painted: the
/// page then covers what is on it.
pub(crate) fn paint(painter: &Painter, cache: &mut Cache, doc: &Document, info: &DocInfo, page: usize, xf: &PageXform, back: Color32) {
    let Some(pg) = info.pages.get(page) else { return };
    let to_screen = |u: [f64; 2]| {
        let q = pg.user_to_view(u[0] as f32, u[1] as f32);
        xf.norm_to_screen(q[0] / xf.pw, q[1] / xf.ph)
    };
    let page_rect = xf.rect;
    // Comments name their layer by the editor's numbers ([`Document::layer_keys`]).
    let keys = doc.layer_keys();
    let hidden_layer = |l: Option<(u32, u16)>| {
        l.is_some_and(|l| info.layers.iter().enumerate().any(|(i, x)| keys.get(i).copied().unwrap_or(x.id) == l && !x.visible))
    };
    let off: Vec<_> = info
        .annotations
        .iter()
        .filter(|a| a.page == page && !skipped(&a.subtype) && !hidden_layer(a.layer))
        .filter(|a| {
            let r = xf.user_rect(info, page, a.rect);
            r.is_finite() && !page_rect.contains_rect(r)
        })
        .collect();
    if off.is_empty() {
        return;
    }
    let units_to_px = f64::from(page_rect.width().min(page_rect.height()) / xf.pw.min(xf.ph).max(1.0));
    for a in off {
        let color = shown(a.color, back);
        let shape = cache.shapes(doc, page).iter().find(|g| g.index == a.index).cloned();
        match shape {
            Some(g) => {
                let w = if g.width.is_finite() { (g.width * units_to_px).clamp(1.0, 200.0) as f32 } else { 1.0 };
                let mut pts: Vec<Pos2> = g.points.iter().map(|p| to_screen(*p)).collect();
                if g.kind == Kind::Polygon
                    && let Some(first) = pts.first().copied()
                {
                    pts.push(first);
                }
                painter.add(egui::Shape::line(pts, Stroke::new(w, color)));
            }
            None => {
                let r = xf.user_rect(info, page, a.rect);
                let stroke = Stroke::new(1.0, color);
                if a.subtype == "Circle" {
                    let (c, rx, ry) = (r.center(), r.width() / 2.0, r.height() / 2.0);
                    let pts = (0..=48)
                        .map(|k| {
                            let t = k as f32 / 48.0 * std::f32::consts::TAU;
                            pos2(c.x + rx * t.cos(), c.y + ry * t.sin())
                        })
                        .collect();
                    painter.add(egui::Shape::line(pts, stroke));
                } else {
                    painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    }

    #[test]
    fn a_lone_sheet_owns_the_whole_view() {
        let view = rect(0.0, 0.0, 1000.0, 800.0);
        let page = rect(100.0, 30.0, 900.0, 600.0);
        assert_eq!(zone(page, [page], view), view);
    }

    #[test]
    fn neighbouring_sheets_split_the_gap() {
        let view = rect(0.0, 0.0, 1000.0, 800.0);
        let a = rect(100.0, 30.0, 900.0, 400.0);
        let b = rect(100.0, 420.0, 900.0, 790.0);
        let za = zone(a, [a, b], view);
        let zb = zone(b, [a, b], view);
        assert_eq!(za, rect(0.0, 0.0, 1000.0, 410.0));
        assert_eq!(zb, rect(0.0, 410.0, 1000.0, 800.0));
        // Side by side (two-up).
        let l = rect(10.0, 30.0, 480.0, 600.0);
        let r = rect(500.0, 30.0, 970.0, 600.0);
        assert_eq!(zone(l, [l, r], view).max.x, 490.0);
        assert_eq!(zone(r, [l, r], view).min.x, 490.0);
    }

    #[test]
    fn the_page_counts_even_partly_out_of_view() {
        let view = rect(0.0, 0.0, 500.0, 500.0);
        let page = rect(-200.0, -200.0, 300.0, 300.0);
        assert_eq!(zone(page, [], view), view);
    }

    #[test]
    fn dark_lines_lift_on_a_dark_pasteboard() {
        let dark = Color32::from_rgb(0x2A, 0x2D, 0x32);
        let light = Color32::from_rgb(0xF1, 0xF1, 0xF3);
        assert!(luminance(shown(Some([0.0, 0.0, 0.0]), dark)) > 0.3);
        assert!(luminance(shown(Some([1.0, 1.0, 1.0]), light)) < 0.6);
        // A colour that already reads keeps its hue.
        let red = shown(Some([1.0, 0.0, 0.0]), light);
        assert!(red.r() > red.g());
        // Garbage colours don't panic.
        let _ = shown(Some([f32::NAN, f32::INFINITY, -3.0]), dark);
    }
}
