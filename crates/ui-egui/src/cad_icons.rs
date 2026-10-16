//! Drafting icons for RevuCraft's Draft tools panel, drawn in code on a 24×24 grid: light strokes
//! for existing geometry, the accent colour for what the tool makes, orange dots for picked
//! points.
//!
//! Adapted from CADCraft's icon set (`cadcraft/crates/ui-egui/src/icons.rs`, original work by the
//! ArtCraft Team, MIT OR Apache-2.0, the same licence as this codebase). No file assets.

use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, Vec2, pos2};

use crate::theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CadIcon {
    Line,
    Polyline,
    Spline,
    Rectangle,
    Circle,
    Arc,
    Ellipse,
    Polygon,
    RevCloud,
    Wall,
    Move,
    Copy,
    Rotate,
    Mirror,
    Scale,
    ArrayRect,
    ArrayPolar,
    Offset,
    Trim,
    Extend,
    Fillet,
    Chamfer,
    Break,
    Join,
    Explode,
    Erase,
    DimAligned,
    DimLinear,
    DimRadius,
    DimDiameter,
    Text,
    Callout,
}

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    w: f32,
    base: Color32,
    acc: Color32,
    pt: Color32,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        let s = self.r.width() / 24.0;
        pos2(self.r.min.x + x * s, self.r.min.y + y * s)
    }
    fn sc(&self) -> f32 {
        self.r.width() / 24.0
    }
    fn st(&self, c: Color32) -> Stroke {
        Stroke::new(self.w, c)
    }
    fn l(&self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.p.line_segment([self.at(x1, y1), self.at(x2, y2)], self.st(self.base));
    }
    fn la(&self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.p.line_segment([self.at(x1, y1), self.at(x2, y2)], self.st(self.acc));
    }
    fn poly(&self, pts: &[(f32, f32)], closed: bool, c: Color32) {
        let mut v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        if closed && let Some(f) = v.first().copied() {
            v.push(f);
        }
        self.p.line(v, self.st(c));
    }
    fn c(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_stroke(self.at(x, y), r * self.sc(), self.st(c));
    }
    fn fill_c(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_filled(self.at(x, y), r * self.sc(), c);
    }
    /// An arc in degrees, counter-clockwise as seen (y up).
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32, c: Color32) {
        let n = (((a1 - a0).abs() / 10.0).ceil() as usize).clamp(2, 72);
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                self.at(x + r * a.cos(), y - r * a.sin())
            })
            .collect();
        self.p.line(pts, self.st(c));
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.poly(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true, c);
    }
    fn dot(&self, x: f32, y: f32) {
        self.fill_c(x, y, 1.6, self.pt);
    }
    fn tri(&self, pts: [(f32, f32); 3], c: Color32) {
        self.p.add(egui::Shape::convex_polygon(pts.iter().map(|(x, y)| self.at(*x, *y)).collect(), c, Stroke::NONE));
    }
    fn arrow_tip(&self, x: f32, y: f32, dx: f32, dy: f32, c: Color32) {
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (ux, uy) = (dx / l, dy / l);
        let (nx, ny) = (-uy, ux);
        self.tri([(x, y), (x - ux * 3.0 + nx * 1.4, y - uy * 3.0 + ny * 1.4), (x - ux * 3.0 - nx * 1.4, y - uy * 3.0 - ny * 1.4)], c);
    }
    fn text(&self, x: f32, y: f32, s: &str, size: f32, c: Color32) {
        self.p.text(self.at(x, y), egui::Align2::CENTER_CENTER, s, egui::FontId::proportional(size * self.sc()), c);
    }
}

/// Paint `icon` into the square `rect`. `base` is the colour of existing geometry, `accent` of
/// what the tool makes.
pub fn paint(p: &Painter, rect: Rect, icon: CadIcon, base: Color32, accent: Color32) {
    let pen = Pen { p, r: rect, w: (rect.width() / 16.0).clamp(1.0, 1.6), base, acc: accent, pt: Color32::from_rgb(242, 163, 58) };
    let (b, a) = (pen.base, pen.acc);
    use CadIcon::*;
    match icon {
        Line => {
            pen.la(4.0, 20.0, 20.0, 4.0);
            pen.dot(4.0, 20.0);
            pen.dot(20.0, 4.0);
        }
        Polyline => {
            pen.poly(&[(3.0, 19.0), (8.0, 8.0), (14.0, 15.0), (21.0, 5.0)], false, a);
            pen.dot(3.0, 19.0);
            pen.dot(8.0, 8.0);
            pen.dot(14.0, 15.0);
            pen.dot(21.0, 5.0);
        }
        Spline => {
            let pts: Vec<(f32, f32)> =
                (0..=20).map(|i| (3.0 + 18.0 * i as f32 / 20.0, 12.0 - 6.0 * (i as f32 / 20.0 * std::f32::consts::TAU).sin())).collect();
            pen.poly(&pts, false, a);
            pen.dot(3.0, 12.0);
            pen.dot(7.5, 6.0);
            pen.dot(16.5, 18.0);
            pen.dot(21.0, 12.0);
        }
        Rectangle => {
            pen.rect(3.0, 6.0, 18.0, 12.0, a);
            pen.dot(3.0, 18.0);
            pen.dot(21.0, 6.0);
        }
        Circle => {
            pen.c(12.0, 12.0, 8.0, a);
            pen.l(12.0, 12.0, 18.0, 6.5);
            pen.dot(12.0, 12.0);
        }
        Arc => {
            pen.arc(12.0, 16.0, 9.0, 10.0, 170.0, a);
            pen.dot(3.2, 14.5);
            pen.dot(12.0, 7.0);
            pen.dot(20.8, 14.5);
        }
        Ellipse => {
            let pts: Vec<(f32, f32)> = (0..36)
                .map(|i| (12.0 + 9.0 * (i as f32 * 10.0f32).to_radians().cos(), 12.0 + 5.0 * (i as f32 * 10.0f32).to_radians().sin()))
                .collect();
            pen.poly(&pts, true, a);
            pen.dot(12.0, 12.0);
            pen.dot(21.0, 17.0);
        }
        Polygon => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|i| (12.0 + 8.5 * (i as f32 * 60.0f32).to_radians().cos(), 12.0 + 8.5 * (i as f32 * 60.0f32).to_radians().sin()))
                .collect();
            pen.poly(&pts, true, a);
            pen.dot(12.0, 12.0);
        }
        RevCloud => {
            for i in 0..6 {
                let ang = i as f32 * 60.0;
                let x = 12.0 + 6.0 * ang.to_radians().cos();
                let y = 12.0 + 6.0 * ang.to_radians().sin();
                pen.arc(x, y, 3.2, -ang - 70.0, -ang + 110.0, a);
            }
        }
        Wall => {
            pen.poly(&[(3.0, 20.0), (3.0, 4.0), (21.0, 4.0)], false, a);
            pen.poly(&[(7.0, 20.0), (7.0, 8.0), (21.0, 8.0)], false, a);
            pen.l(3.0, 20.0, 7.0, 20.0);
            pen.l(21.0, 4.0, 21.0, 8.0);
        }
        Move => {
            pen.la(12.0, 3.0, 12.0, 21.0);
            pen.la(3.0, 12.0, 21.0, 12.0);
            pen.arrow_tip(12.0, 2.0, 0.0, -1.0, a);
            pen.arrow_tip(12.0, 22.0, 0.0, 1.0, a);
            pen.arrow_tip(2.0, 12.0, -1.0, 0.0, a);
            pen.arrow_tip(22.0, 12.0, 1.0, 0.0, a);
        }
        Copy => {
            pen.rect(3.0, 3.0, 11.0, 13.0, b);
            pen.rect(10.0, 8.0, 11.0, 13.0, a);
        }
        Rotate => {
            pen.arc(12.0, 12.0, 8.0, 100.0, 380.0, a);
            pen.arrow_tip(10.6, 4.1, -1.0, 0.1, a);
            pen.dot(12.0, 12.0);
        }
        Mirror => {
            pen.poly(&[(3.0, 18.0), (9.0, 6.0), (9.0, 18.0)], true, b);
            pen.poly(&[(21.0, 18.0), (15.0, 6.0), (15.0, 18.0)], true, a);
            for y in [3.0, 7.0, 11.0, 15.0, 19.0] {
                pen.l(12.0, y, 12.0, y + 2.0);
            }
        }
        Scale => {
            pen.rect(3.0, 11.0, 10.0, 10.0, b);
            pen.rect(3.0, 3.0, 18.0, 18.0, a);
            pen.la(9.0, 15.0, 18.0, 6.0);
            pen.arrow_tip(19.0, 5.0, 1.0, -1.0, a);
        }
        ArrayRect => {
            for i in 0..3 {
                for j in 0..3 {
                    pen.rect(3.0 + i as f32 * 7.0, 3.0 + j as f32 * 7.0, 4.0, 4.0, if i + j == 0 { b } else { a });
                }
            }
        }
        ArrayPolar => {
            pen.dot(12.0, 12.0);
            for i in 0..6 {
                let ang = (i as f32 * 60.0).to_radians();
                pen.c(12.0 + 7.5 * ang.cos(), 12.0 + 7.5 * ang.sin(), 2.0, if i == 0 { b } else { a });
            }
        }
        Offset => {
            pen.arc(14.0, 18.0, 6.0, 90.0, 180.0, b);
            pen.l(14.0, 12.0, 21.0, 12.0);
            pen.l(8.0, 18.0, 8.0, 22.0);
            pen.arc(14.0, 18.0, 10.0, 90.0, 180.0, a);
            pen.la(14.0, 8.0, 21.0, 8.0);
            pen.la(4.0, 18.0, 4.0, 22.0);
        }
        Trim => {
            pen.l(8.0, 2.0, 8.0, 22.0);
            pen.l(16.0, 2.0, 16.0, 22.0);
            pen.l(2.0, 12.0, 8.0, 12.0);
            pen.l(16.0, 12.0, 22.0, 12.0);
            for x in [9.5, 12.0, 14.5] {
                pen.la(x, 12.0, x + 1.0, 12.0);
            }
            pen.poly(&[(10.0, 17.0), (12.0, 15.0), (14.0, 17.0)], false, a);
        }
        Extend => {
            pen.l(20.0, 2.0, 20.0, 22.0);
            pen.l(3.0, 12.0, 11.0, 12.0);
            pen.la(11.0, 12.0, 19.0, 12.0);
            pen.arrow_tip(19.5, 12.0, 1.0, 0.0, a);
        }
        Fillet => {
            pen.l(4.0, 20.0, 4.0, 12.0);
            pen.l(12.0, 4.0, 20.0, 4.0);
            pen.arc(12.0, 12.0, 8.0, 90.0, 180.0, a);
        }
        Chamfer => {
            pen.l(4.0, 20.0, 4.0, 10.0);
            pen.l(10.0, 4.0, 20.0, 4.0);
            pen.la(4.0, 10.0, 10.0, 4.0);
        }
        Break => {
            pen.l(3.0, 12.0, 10.0, 12.0);
            pen.la(14.0, 12.0, 21.0, 12.0);
            pen.dot(12.0, 12.0);
            pen.l(10.0, 16.0, 14.0, 8.0);
        }
        Join => {
            pen.l(3.0, 16.0, 11.0, 8.0);
            pen.la(11.0, 8.0, 21.0, 16.0);
            pen.dot(11.0, 8.0);
        }
        Explode => {
            pen.poly(&[(4.0, 9.0), (9.0, 4.0)], false, a);
            pen.poly(&[(15.0, 4.0), (20.0, 9.0)], false, a);
            pen.poly(&[(20.0, 15.0), (15.0, 20.0)], false, a);
            pen.poly(&[(9.0, 20.0), (4.0, 15.0)], false, a);
            pen.fill_c(12.0, 12.0, 2.0, pen.pt);
        }
        Erase => {
            pen.poly(&[(4.0, 16.0), (13.0, 7.0), (19.0, 13.0), (12.0, 20.0), (8.0, 20.0)], true, b);
            pen.la(9.0, 11.0, 15.0, 17.0);
            pen.l(8.0, 20.0, 20.0, 20.0);
        }
        DimAligned | DimLinear => {
            let (y0, y1) = (8.0, 18.0);
            pen.l(4.0, y1, 4.0, 5.0);
            pen.l(20.0, y1, 20.0, 5.0);
            pen.la(4.0, y0, 20.0, y0);
            pen.arrow_tip(4.0, y0, -1.0, 0.0, a);
            pen.arrow_tip(20.0, y0, 1.0, 0.0, a);
            if icon == DimLinear {
                pen.text(12.0, 14.5, "1.5", 6.0, b);
            }
        }
        DimRadius => {
            pen.arc(12.0, 14.0, 8.0, 0.0, 180.0, b);
            pen.la(12.0, 14.0, 17.6, 8.4);
            pen.arrow_tip(17.6, 8.4, 1.0, -1.0, a);
            pen.dot(12.0, 14.0);
        }
        DimDiameter => {
            pen.c(12.0, 12.0, 8.0, b);
            pen.la(6.4, 17.6, 17.6, 6.4);
            pen.arrow_tip(6.4, 17.6, -1.0, 1.0, a);
            pen.arrow_tip(17.6, 6.4, 1.0, -1.0, a);
        }
        Text => pen.text(12.0, 12.5, "A", 19.0, b),
        Callout => {
            pen.la(3.0, 20.0, 10.0, 9.0);
            pen.la(10.0, 9.0, 14.0, 9.0);
            pen.arrow_tip(3.0, 20.0, -0.55, 1.0, a);
            pen.l(15.0, 6.0, 22.0, 6.0);
            pen.l(15.0, 10.0, 21.0, 10.0);
        }
    }
}

/// An icon button like [`crate::icons::button`], with a drafting icon.
pub fn button(ui: &mut egui::Ui, icon: CadIcon, box_size: f32, selected: bool, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(box_size), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, tooltip));
    if selected {
        ui.painter().rect_filled(rect, t.radius, t.accent_soft);
    } else if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, t.radius, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let dim = !ui.is_enabled();
    let fade = |c: Color32| if dim { c.gamma_multiply(0.4) } else { c };
    let inner = Rect::from_center_size(rect.center(), Vec2::splat((box_size * 0.62).round()));
    paint(ui.painter(), inner, icon, fade(if selected { t.accent_text } else { t.icon }), fade(t.accent));
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}
