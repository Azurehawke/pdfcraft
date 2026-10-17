//! Measure objects: page-space clicks, live readings and persistent drawing scales.
use crate::{LeftPanel, PdfCraftApp, QuickTool, canvas::DocView, theme::Tokens, widgets};
use egui::{Color32, Pos2, Stroke};
use pdfcraft_engine::{
    Document, Edit, Style,
    measure::{self, Kind, NewMeasurement, Point, Reading, Scale},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Distance,
    Perimeter,
    Area,
    Calibrate,
    /// The area of a rectangle from two opposite corners.
    Rectangle,
    /// The angle at a vertex: an arm's end, the vertex, the other arm's end.
    Angle,
    /// The radius of a circle or arc through three points on it.
    Radius,
    /// A cable run: along its route, plus the allowance for the drop and the service loop.
    CableRun,
}
impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Distance => "distance",
            Self::Perimeter => "perimeter",
            Self::Area => "area",
            Self::Calibrate => "calibrate",
            Self::Rectangle => "rectangle",
            Self::Angle => "angle",
            Self::Radius => "radius",
            Self::CableRun => "cable",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "distance" => Some(Self::Distance),
            "perimeter" => Some(Self::Perimeter),
            "area" => Some(Self::Area),
            "calibrate" => Some(Self::Calibrate),
            "rectangle" => Some(Self::Rectangle),
            "angle" => Some(Self::Angle),
            "radius" => Some(Self::Radius),
            "cable" => Some(Self::CableRun),
            _ => None,
        }
    }
    fn kind(self) -> Kind {
        match self {
            Self::Area | Self::Rectangle => Kind::Area,
            Self::Perimeter | Self::CableRun => Kind::Perimeter,
            _ => Kind::Distance,
        }
    }
}
/// How many clicks finish a fixed-count tool.
fn clicks(tool: Tool) -> Option<usize> {
    match tool {
        Tool::Distance | Tool::Calibrate | Tool::Rectangle => Some(2),
        Tool::Angle | Tool::Radius => Some(3),
        Tool::Perimeter | Tool::Area | Tool::CableRun => None,
    }
}

/// The points a tool measures from the clicked ones: a rectangle's four corners, a circle's centre
/// and the first point (its radius); the others as clicked.
fn measured(tool: Tool, points: &[Point]) -> Vec<Point> {
    match (tool, points) {
        (Tool::Rectangle, [a, b, ..]) => vec![*a, [b[0], a[1]], *b, [a[0], b[1]]],
        (Tool::Radius, [a, m, b, ..]) => match circle_through(*a, *m, *b) {
            Some((c, _)) => vec![c, *a],
            None => vec![*a, *b],
        },
        _ => points.to_vec(),
    }
}

/// The circle through three points (centre and radius), `None` when they are in a row.
fn circle_through(a: Point, m: Point, b: Point) -> Option<(Point, f64)> {
    let d = 2.0 * (a[0] * (m[1] - b[1]) + m[0] * (b[1] - a[1]) + b[0] * (a[1] - m[1]));
    if d.abs() < 1e-9 {
        return None;
    }
    let sq = |p: Point| p[0] * p[0] + p[1] * p[1];
    let c = [
        (sq(a) * (m[1] - b[1]) + sq(m) * (b[1] - a[1]) + sq(b) * (a[1] - m[1])) / d,
        (sq(a) * (b[0] - m[0]) + sq(m) * (a[0] - b[0]) + sq(b) * (m[0] - a[0])) / d,
    ];
    let r = measure::distance(c, a);
    (c.iter().all(|v| v.is_finite()) && r.is_finite() && r < 1e7).then_some((c, r))
}

/// The angle at `vertex` between the arms to `a` and `b`, in degrees (0–180), as drawn on the
/// page (the scale's axes can differ).
fn angle_at(a: Point, vertex: Point, b: Point, scale: &Scale) -> Option<f64> {
    let u = [(a[0] - vertex[0]) * scale.x, (a[1] - vertex[1]) * scale.y];
    let v = [(b[0] - vertex[0]) * scale.x, (b[1] - vertex[1]) * scale.y];
    let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
    if lu < 1e-12 || lv < 1e-12 {
        return None;
    }
    let cos = ((u[0] * v[0] + u[1] * v[1]) / (lu * lv)).clamp(-1.0, 1.0);
    Some(cos.acos().to_degrees())
}

/// A common drawing scale: `paper_points` on the sheet stand for `real` `unit`s. `fractions`
/// is the inch denominator for feet-and-inch scales, 0 for decimal ones. The names are the
/// trade's own notation and aren't translated.
pub struct Preset {
    pub name: &'static str,
    pub paper_points: f64,
    pub real: f64,
    pub unit: &'static str,
    pub fractions: u16,
}
const fn arch(name: &'static str, paper_inches: f64) -> Preset {
    // paper_inches on the sheet = 1 foot.
    Preset { name, paper_points: paper_inches * 72.0, real: 1.0, unit: "ft", fractions: 16 }
}
const fn eng(name: &'static str, feet: f64) -> Preset {
    // 1 inch on the sheet = `feet` feet.
    Preset { name, paper_points: 72.0, real: feet, unit: "ft", fractions: 0 }
}
const fn metric(name: &'static str, ratio: f64) -> Preset {
    // 1 mm on the sheet = `ratio` mm.
    Preset { name, paper_points: 72.0 / 25.4, real: ratio / 1000.0, unit: "m", fractions: 0 }
}
pub const PRESETS: [Preset; 26] = [
    arch("1/16\" = 1'-0\"", 1.0 / 16.0),
    arch("3/32\" = 1'-0\"", 3.0 / 32.0),
    arch("1/8\" = 1'-0\"", 1.0 / 8.0),
    arch("3/16\" = 1'-0\"", 3.0 / 16.0),
    arch("1/4\" = 1'-0\"", 1.0 / 4.0),
    arch("3/8\" = 1'-0\"", 3.0 / 8.0),
    arch("1/2\" = 1'-0\"", 1.0 / 2.0),
    arch("3/4\" = 1'-0\"", 3.0 / 4.0),
    arch("1\" = 1'-0\"", 1.0),
    arch("1 1/2\" = 1'-0\"", 1.5),
    arch("3\" = 1'-0\"", 3.0),
    eng("1\" = 10'", 10.0),
    eng("1\" = 20'", 20.0),
    eng("1\" = 30'", 30.0),
    eng("1\" = 40'", 40.0),
    eng("1\" = 50'", 50.0),
    eng("1\" = 60'", 60.0),
    eng("1\" = 100'", 100.0),
    metric("1:5", 5.0),
    metric("1:10", 10.0),
    metric("1:20", 20.0),
    metric("1:50", 50.0),
    metric("1:100", 100.0),
    metric("1:200", 200.0),
    metric("1:500", 500.0),
    metric("1:1000", 1000.0),
];
/// The scale a preset sets: its real length per point of paper, in feet-and-inches when the
/// preset is architectural.
pub fn preset_scale(p: &Preset) -> Result<Scale, String> {
    let scale = Scale::new(p.real / p.paper_points, p.unit, 2);
    match p.fractions {
        0 => scale,
        d => scale.and_then(|s| s.with_feet_inches(d)),
    }
    .map_err(|e| e.to_string())
}
pub struct MeasureView {
    pub points: Vec<Point>,
    pub page: Option<usize>,
    pub tool: Option<Tool>,
    pub reading: Option<Reading>,
    pub scale: Option<Scale>,
    pub error: Option<String>,
    pub snaps: measure::snap::SnapOptions,
    pub snap_enabled: bool,
    pub sensitivity: f64,
    pub drawing_points: f64,
    pub real_distance: f64,
    pub unit: String,
    pub precision: u8,
    /// Inch denominator for feet-and-inch lengths; 0 writes decimal lengths.
    pub fractions: u16,
    pub name: String,
    pub whole_page: bool,
    pub rect: [f64; 4],
    pub label: String,
    seeded_page: Option<usize>,
    /// Calibrating: the two points picked, waiting for their real length in the value box.
    pub calibration: Option<Calibration>,
    /// What a typed number without a unit means (Draft's setting), for the real length.
    pub bare: crate::draft::BareUnit,
    /// A scale was just set by calibrating (the canvas puts the tool down).
    pub calibrated: bool,
    /// The allowance a cable run adds (inches; Draft's setting).
    pub cable_inches: f64,
    /// Snapping geometry (or why it failed) per (page, edit generation): extraction is not
    /// repeated on every hover frame.
    paths: Option<(usize, u64, Result<measure::snap::Geometry, String>)>,
    /// Saved measurements per edit generation, for the panel.
    listing: Option<(u64, Result<measure::Listing, String>)>,
    /// The Scale panel: how the scale is being set, and where it goes.
    pub set_by: SetBy,
    pub use_on: UseOn,
    /// Waiting for a box dragged on the page (the area a scale covers).
    pub area_pick: bool,
    /// Custom scales give the paper length in millimetres.
    pub paper_mm: bool,
}

/// How the Scale panel sets the scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SetBy {
    #[default]
    Preset,
    Custom,
    Calibrate,
}

/// Where the Scale panel puts the scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UseOn {
    #[default]
    Page,
    All,
    /// A box on the current page (a detail or viewport drawn at its own scale).
    Area,
}

/// Inches in one of each length unit the panel offers.
fn inches_per(unit: &str) -> Option<f64> {
    match unit {
        "in" => Some(1.0),
        "ft" => Some(12.0),
        "yd" => Some(36.0),
        "mm" => Some(1.0 / 25.4),
        "cm" => Some(10.0 / 25.4),
        "m" => Some(1000.0 / 25.4),
        _ => None,
    }
}

/// "Show lengths as": the panel's choices (label, unit, inch fraction (0: decimal), decimals).
fn length_formats() -> Vec<(String, &'static str, u16, u8)> {
    let mut out: Vec<(String, &'static str, u16, u8)> =
        [16u16, 8, 4, 2, 1].iter().map(|d| (format!("{} · 1/{d}\"", tl!("Feet and inches")), "ft", *d, 0)).collect();
    out.push((tl!("Decimal feet").to_string(), "ft", 0, 2));
    out.push((tl!("Inches").to_string(), "in", 0, 2));
    out.push((tl!("Metres").to_string(), "m", 0, 3));
    out.push((tl!("Millimetres").to_string(), "mm", 0, 0));
    out
}
/// A calibration waiting for the real length of the line between `a` and `b` (user space).
#[derive(Clone, Debug, Default)]
pub struct Calibration {
    pub page: usize,
    pub a: Point,
    pub b: Point,
    pub text: String,
    pub error: Option<String>,
    /// Set the scale on every page, not only this one.
    pub all_pages: bool,
}

impl Default for MeasureView {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            page: None,
            tool: None,
            reading: None,
            scale: None,
            error: None,
            snaps: Default::default(),
            snap_enabled: true,
            sensitivity: 6.0,
            drawing_points: 72.0,
            real_distance: 1.0,
            unit: "in".into(),
            precision: 2,
            fractions: 0,
            name: "Drawing scale".into(),
            whole_page: true,
            rect: [0.0, 0.0, 100.0, 100.0],
            label: String::new(),
            seeded_page: None,
            calibration: None,
            bare: crate::draft::BareUnit::Feet,
            calibrated: false,
            cable_inches: 0.0,
            paths: None,
            listing: None,
            set_by: SetBy::Preset,
            use_on: UseOn::Page,
            area_pick: false,
            paper_mm: false,
        }
    }
}
impl MeasureView {
    pub fn cancel(&mut self) {
        self.calibration = None;
        self.points.clear();
        self.page = None;
        self.reading = None;
        self.scale = None;
    }
}
pub(crate) fn page_input(ui: &egui::Ui, resp: &egui::Response, doc: &Document, view: &mut DocView, cx: &crate::comments::PageCx<'_>, tool: Tool) {
    let page = cx.page;
    let xf = cx.xf;
    let author = &cx.prefs.author;
    if !doc.allows_annotation() {
        return;
    }
    let Some(info) = doc.info.pages.get(page) else { return };
    let state = &mut view.measure;
    if state.tool != Some(tool) {
        state.cancel();
        state.tool = Some(tool);
    }
    // Calibrating: the two points are picked; the value box asks for their real length.
    let bare_feet = state.bare == crate::draft::BareUnit::Feet;
    if tool == Tool::Calibrate
        && let Some(cal) = state.calibration.as_mut()
    {
        if cal.page != page {
            return;
        }
        let screen = |p: Point| {
            let v = info.user_to_view(p[0] as f32, p[1] as f32);
            xf.norm_to_screen(v[0] / xf.pw, v[1] / xf.ph)
        };
        let stroke = Stroke::new(1.5, Color32::from_rgb(0, 120, 214));
        let (a, b) = (screen(cal.a), screen(cal.b));
        ui.painter().line_segment([a, b], stroke);
        ui.painter().circle_filled(a, 3.0, stroke.color);
        ui.painter().circle_filled(b, 3.0, stroke.color);
        let several = doc.info.pages.len() > 1;
        let all_pages = &mut cal.all_pages;
        let action = crate::value_prompt::show(
            ui.ctx(),
            ("calibrate", page),
            b,
            ui.clip_rect(),
            crate::value_prompt::Prompt {
                error: cal.error.as_deref(),
                ..crate::value_prompt::Prompt::new(tl!("How long is this line really?"), "12'6\"  150  3.5m")
            },
            &mut cal.text,
            |ui| {
                let unit = if bare_feet { tl!("Plain numbers are feet") } else { tl!("Plain numbers are inches") };
                ui.label(egui::RichText::new(unit).small().weak());
                if several {
                    ui.checkbox(all_pages, tl!("Use this scale on every page"));
                }
            },
        );
        let escape = action.is_none() && !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Escape));
        match action {
            Some(crate::value_prompt::PromptAction::Cancel) => state.cancel(),
            None if escape => state.cancel(),
            Some(crate::value_prompt::PromptAction::Apply) => {
                let bare = state.bare;
                let fractions = state.fractions;
                let precision = state.precision;
                let name = state.name.clone();
                if let Some(cal) = state.calibration.as_mut() {
                    match calibration_edit(doc, cal, bare, fractions, precision, &name) {
                        Ok(edit) => {
                            view.pending_edit = Some(edit);
                            state.cancel();
                            state.error = None;
                            state.calibrated = true;
                        }
                        Err(e) => cal.error = Some(e),
                    }
                }
            }
            Some(crate::value_prompt::PromptAction::Leave) | None => {}
        }
        return;
    }
    // Keys belong to a focused text field (the label, unit or viewport name) when there is one.
    let typing = ui.ctx().egui_wants_keyboard_input();
    // Escape keeps a perimeter or area that has enough points (it used to throw it away); with
    // too few points it cancels.
    if !typing && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if state.page == Some(page) && enough_points(tool, state.points.len()) {
            finish(state, &mut view.pending_edit, doc, page, tool, author);
        } else if state.page.is_none_or(|p| p == page) {
            state.cancel();
        }
    }
    let screen = |p: Point| {
        let v = info.user_to_view(p[0] as f32, p[1] as f32);
        xf.norm_to_screen(v[0] / xf.pw, v[1] / xf.ph)
    };
    let pos = ui.input(|i| i.pointer.hover_pos()).filter(|p| xf.rect.contains(*p));
    let mut hover = None;
    let mut snap = None;
    if let Some(pos) = pos {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        let (vx, vy) = xf.screen_to_view(pos);
        let [x, y] = info.view_to_user(vx, vy);
        let mut point = [f64::from(x), f64::from(y)];
        if state.snap_enabled {
            if state.paths.as_ref().is_none_or(|(p, g, _)| *p != page || *g != doc.edit_generation()) {
                state.paths = Some((page, doc.edit_generation(), doc.measurement_paths(page)));
            }
            let pixels_per_unit = (xf.rect.width() / xf.pw).max(1e-6);
            if let Some((_, _, Err(e))) = &state.paths {
                state.error = Some(e.clone());
            }
            if let Some((_, _, Ok(paths))) = &state.paths {
                if paths.truncated {
                    state.error = Some(tl!("This drawing exceeds the snapping limit; only the first paths are used.").into());
                } else if paths.unreadable > 0 {
                    state.error = Some(tl!("Some page content couldn't be read; snapping uses the rest.").into());
                }
                let tolerance = doc
                    .measurement_to_user(page, [f64::from(vx) + state.sensitivity / f64::from(pixels_per_unit), f64::from(vy)])
                    .map(|p| measure::distance(point, p))
                    .unwrap_or(state.sensitivity / f64::from(pixels_per_unit));
                if state.snaps.intersections && paths.intersection_limited(point, tolerance) {
                    state.error = Some(tl!("This area has too many overlapping paths; intersection snapping is limited.").into());
                }
                let raw = point;
                if let Ok(Some(hit)) = paths.snap(point, tolerance, state.snaps) {
                    point = hit.point;
                    snap = Some(hit.kind);
                }
                // Square off the previous point to a line, unless a corner or middle is closer.
                if snap.is_none_or(|k| k == measure::snap::SnapKind::Path)
                    && let Some(last) = state.points.last().copied()
                    && let Some(p) = measure::snap::perpendicular(last, raw, paths.segments.iter().copied(), tolerance)
                {
                    point = p;
                    snap = Some(measure::snap::SnapKind::Perpendicular);
                }
            }
        }
        if ui.input(|i| i.modifiers.shift)
            && let Some(last) = state.points.last()
        {
            point = measure::snap::constrain(*last, point);
        }
        // Near the first point of an area or perimeter, the pointer snaps to it, so the preview
        // shows the closed shape and a tap closes it.
        if matches!(tool, Tool::Area | Tool::Perimeter)
            && state.points.len() >= 3
            && let Some(first) = state.points.first().copied()
            && screen(first).distance(pos) <= crate::point_bar::CLOSE_RADIUS
        {
            point = first;
            snap = Some(measure::snap::SnapKind::Endpoint);
        }
        hover = Some(point);
        if resp.clicked() {
            if state.page != Some(page) {
                state.cancel();
                state.page = Some(page);
            }
            if state.points.is_empty() {
                match doc.measurement_scale(page, point) {
                    Ok(scale) => {
                        state.scale = Some(scale);
                        state.error = None;
                    }
                    Err(e) => {
                        state.error = Some(e);
                        return;
                    }
                }
            }
            // Tapping the first point again closes an area or a perimeter.
            let closes = matches!(tool, Tool::Area | Tool::Perimeter)
                && state.points.len() >= 3
                && state.points.first().is_some_and(|first| screen(*first).distance(pos) <= crate::point_bar::CLOSE_RADIUS);
            if closes
                && tool == Tool::Perimeter
                && let Some(first) = state.points.first().copied()
            {
                state.points.push(first);
            }
            let repeats = state.points.last().is_some_and(|last| measure::distance(*last, point) < 1e-6);
            if !closes && !repeats && state.points.len() < measure::MAX_POINTS {
                state.points.push(point);
            }
            let finishes = closes || clicks(tool).is_some_and(|n| state.points.len() >= n);
            if finishes {
                finish(state, &mut view.pending_edit, doc, page, tool, author);
            }
        }
    }
    if state.page == Some(page) && !state.points.is_empty() {
        if (resp.double_clicked() || !typing && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            && matches!(tool, Tool::Perimeter | Tool::Area | Tool::CableRun)
        {
            finish(state, &mut view.pending_edit, doc, page, tool, author);
        }
        if !typing && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
            state.points.pop();
        }
        let mut points = state.points.clone();
        if let Some(p) = hover
            && points.last().is_none_or(|last| measure::distance(*last, p) > 1e-6)
        {
            points.push(p);
        }
        if let Some(scale) = &state.scale {
            match measure::reading(tool.kind(), &measured(tool, &points), scale) {
                Ok(mut reading) => {
                    if tool == Tool::Angle {
                        reading.label = match points.as_slice() {
                            [a, v, b, ..] => angle_at(*a, *v, *b, scale).map(|d| format!("{d:.1}°")).unwrap_or_default(),
                            _ => String::new(),
                        };
                    }
                    if tool == Tool::Radius && points.len() < 3 {
                        reading.label = String::new();
                    }
                    // A cable run reads with its allowance added.
                    if tool == Tool::CableRun
                        && let Some(k) = measure::per_inch(&scale.unit)
                        && let Ok(text) = scale.format_length(reading.value + state.cable_inches * k)
                    {
                        reading.label = text;
                    }
                    state.reading = Some(reading)
                }
                Err(e) => state.error = Some(e.to_string()),
            }
        }
        // A rectangle shows its four sides; a radius its circle and the radius line.
        let shown: Vec<Point> = match (tool, points.as_slice()) {
            (Tool::Rectangle, [_, _, ..]) => {
                let mut c = measured(tool, &points);
                if let Some(first) = c.first().copied() {
                    c.push(first);
                }
                c
            }
            _ => points.clone(),
        };
        if tool == Tool::Radius
            && let [a, m, b, ..] = points.as_slice()
            && let Some((c, r)) = circle_through(*a, *m, *b)
        {
            let ring: Vec<Pos2> = (0..=72)
                .map(|k| {
                    let t = std::f64::consts::TAU * f64::from(k) / 72.0;
                    screen([c[0] + r * t.cos(), c[1] + r * t.sin()])
                })
                .collect();
            ui.painter().add(egui::Shape::line(ring, Stroke::new(1.0, Color32::from_rgb(0, 120, 214).gamma_multiply(0.6))));
            ui.painter().line_segment([screen(c), screen(*a)], Stroke::new(1.5, Color32::from_rgb(0, 120, 214)));
        }
        let positions: Vec<Pos2> = shown.iter().map(|&p| screen(p)).collect();
        let stroke = Stroke::new(1.5, Color32::from_rgb(0, 120, 214));
        for pair in positions.windows(2) {
            if let [a, b] = pair {
                ui.painter().line_segment([*a, *b], stroke);
            }
        }
        if tool == Tool::Area
            && let (Some(a), Some(b)) = (positions.first(), positions.last())
        {
            ui.painter().line_segment([*a, *b], stroke);
        }
        for &p in &positions {
            ui.painter().circle_filled(p, 3.0, stroke.color);
        }
        // The first point is a target: tapping it closes the shape.
        if matches!(tool, Tool::Area | Tool::Perimeter)
            && state.points.len() >= 3
            && let Some(first) = state.points.first()
        {
            let at = screen(*first);
            let near = pos.is_some_and(|p| p.distance(at) <= crate::point_bar::CLOSE_RADIUS);
            ui.painter().circle_stroke(at, if near { crate::point_bar::CLOSE_RADIUS } else { 7.0 }, Stroke::new(1.5, stroke.color));
        }
        // Finish, close, undo and cancel as buttons: the way to end the shape on a tablet.
        if matches!(tool, Tool::Perimeter | Tool::Area | Tool::CableRun)
            && let Some(last) = state.points.last().copied()
        {
            let n = state.points.len();
            let closes = tool == Tool::Perimeter && n >= 3;
            match crate::point_bar::show(ui.ctx(), ("measure", page), screen(last), ui.clip_rect(), enough_points(tool, n), closes) {
                Some(crate::point_bar::PointAction::Finish) => finish(state, &mut view.pending_edit, doc, page, tool, author),
                Some(crate::point_bar::PointAction::Close) => {
                    if let Some(first) = state.points.first().copied() {
                        state.points.push(first);
                    }
                    finish(state, &mut view.pending_edit, doc, page, tool, author);
                }
                Some(crate::point_bar::PointAction::Undo) => {
                    state.points.pop();
                }
                Some(crate::point_bar::PointAction::Cancel) => state.cancel(),
                None => {}
            }
        }
        if let (Some(p), Some(kind)) = (hover, snap) {
            ui.painter().circle_stroke(screen(p), 6.0, Stroke::new(1.0, Color32::from_rgb(46, 158, 92)));
            ui.painter().text(
                screen(p) + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                tl!(match kind {
                    measure::snap::SnapKind::Endpoint => "Endpoint",
                    measure::snap::SnapKind::Midpoint => "Midpoint",
                    measure::snap::SnapKind::Intersection => "Intersection",
                    measure::snap::SnapKind::Path => "Path",
                    measure::snap::SnapKind::Perpendicular => "Perpendicular",
                }),
                crate::theme::regular(11.0),
                stroke.color,
            );
        }
        if let (Some(p), Some(reading)) = (hover, &state.reading) {
            ui.painter().text(screen(p) + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, &reading.label, crate::theme::semibold(12.0), stroke.color);
        }
    }
    // Calibrating: say what to click.
    if tool == Tool::Calibrate
        && let Some(p) = pos
    {
        let hint = if state.points.is_empty() { tl!("Click one end of a length you know") } else { tl!("Click the other end") };
        crate::draft_edit::chip(ui.painter(), p + egui::vec2(14.0, 30.0), hint);
    }
}

/// The edit that sets the scale from a calibration: the real length typed for the line between
/// the two points, on this page or every page. Feet and inches read as feet and inches; a metric
/// length makes a metric scale.
fn calibration_edit(
    doc: &Document,
    cal: &Calibration,
    bare: crate::draft::BareUnit,
    fractions: u16,
    precision: u8,
    name: &str,
) -> Result<Edit, String> {
    let text = cal.text.trim().to_ascii_lowercase();
    let inches = crate::draft::parse_length(&text, bare)
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| tl!("Type the real length, like 12'6\", 150 or 3.5m").to_string())?;
    let metric = text.ends_with('m');
    let (real, unit) = if metric { (inches * 0.0254, "m") } else { (inches / 12.0, "ft") };
    let a = doc.measurement_to_view(cal.page, cal.a)?;
    let b = doc.measurement_to_view(cal.page, cal.b)?;
    let drawn = measure::distance(a, b);
    if !drawn.is_finite() || drawn < 1e-6 {
        return Err(tl!("The two points are in the same place; pick two ends of a length.").to_string());
    }
    let ratio = real / drawn;
    let pages: Vec<usize> = if cal.all_pages { (0..doc.info.pages.len()).collect() } else { vec![cal.page] };
    let mut edits = Vec::new();
    for page in pages {
        let Some(info) = doc.info.pages.get(page) else { continue };
        let ua = doc.measurement_to_user(page, [0.0, 0.0])?;
        let ub = doc.measurement_to_user(page, [1.0, 0.0])?;
        let scale = Scale::calibrate(ua, ub, ratio, unit, if metric { precision.max(2) } else { precision });
        let scale = match (metric, fractions) {
            (true, _) => scale,
            (false, 0) => scale.and_then(|s| s.with_feet_inches(16)),
            (false, d) => scale.and_then(|s| s.with_feet_inches(d)),
        }
        .map_err(|e| e.to_string())?;
        let a = info.view_to_user(0.0, 0.0);
        let b = info.view_to_user(info.width, info.height);
        edits.push(Edit::SetMeasurementScale {
            page,
            bbox: [f64::from(a[0].min(b[0])), f64::from(a[1].min(b[1])), f64::from(a[0].max(b[0])), f64::from(a[1].max(b[1]))],
            name: name.to_string(),
            scale,
        });
    }
    match edits.len() {
        0 => Err(tl!("The page can't be measured.").to_string()),
        1 => edits.pop().ok_or_else(|| tl!("The page can't be measured.").to_string()),
        _ => Ok(Edit::Batch { label: "Set measurement scale".into(), edits }),
    }
}
/// Whether a perimeter or area has enough points to keep.
fn enough_points(tool: Tool, n: usize) -> bool {
    match tool {
        Tool::Area => n >= 3,
        Tool::Perimeter | Tool::CableRun => n >= 2,
        _ => false,
    }
}

fn finish(state: &mut MeasureView, pending: &mut Option<Edit>, doc: &Document, page: usize, tool: Tool, author: &str) {
    if tool == Tool::Calibrate {
        let picked = match state.points.as_slice() {
            [a, b] => Some((*a, *b)),
            _ => None,
        };
        // The options box shows the picked length too.
        if let Some((a, b)) = picked
            && let (Ok(a), Ok(b)) = (doc.measurement_to_view(page, a), doc.measurement_to_view(page, b))
        {
            state.drawing_points = measure::distance(a, b);
        }
        state.cancel();
        // The value box asks for the line's real length next.
        if let Some((a, b)) = picked {
            state.calibration = Some(Calibration { page, a, b, ..Default::default() });
        }
        return;
    }
    // An angle is drawn as its two arms with the reading on them.
    if tool == Tool::Angle {
        if let ([a, v, b, ..], Some(scale)) = (state.points.as_slice(), state.scale.as_ref())
            && let Some(deg) = angle_at(*a, *v, *b, scale)
        {
            *pending = Some(Edit::AddDraftShape {
                new: pdfcraft_engine::NewAnnotation {
                    page,
                    shape: pdfcraft_engine::Shape::PolyLine {
                        vertices: vec![*a, *v, *b],
                        start: pdfcraft_engine::LineEnding::None,
                        end: pdfcraft_engine::LineEnding::None,
                    },
                    style: Style { color: [0.0, 0.47, 0.84], ..Style::default() },
                    contents: String::new(),
                    author: author.into(),
                },
                real_width: None,
                caption: Some((format!("{deg:.1}°"), "Angle".into())),
            });
        }
        state.cancel();
        return;
    }
    let minimum = match tool {
        Tool::Area | Tool::Angle | Tool::Radius => 3,
        _ => 2,
    };
    if state.points.len() < minimum {
        state.error = Some(crate::i18n::fmt(tl!("This measurement needs at least {n} points."), &[("n", &minimum.to_string())]));
        return;
    }
    if let Some(scale) = state.scale.clone() {
        let points = measured(tool, &state.points);
        if tool == Tool::Radius && matches!(state.points.as_slice(), [a, m, b, ..] if circle_through(*a, *m, *b).is_none()) {
            state.error = Some(tl!("These three points are in a line; pick three points on the circle or arc.").into());
            state.cancel();
            return;
        }
        let label = if tool == Tool::Radius && state.label.is_empty() { "R".to_string() } else { state.label.clone() };
        let new = NewMeasurement {
            page,
            kind: tool.kind(),
            points,
            scale,
            style: Style { color: [0.0, 0.47, 0.84], ..Style::default() },
            label,
            author: author.into(),
            dimension: None,
        };
        *pending = Some(if tool == Tool::CableRun {
            Edit::AddCableRun { new, extra_inches: state.cable_inches.clamp(0.0, 1e6) }
        } else {
            Edit::AddMeasurement(new)
        });
        state.cancel();
    }
}
pub(crate) fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::ScrollArea::vertical().id_salt("measure-panel").show(ui, |ui| panel_body(app, ui, t));
}
fn panel_body(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some((index, id)) = app.active_ids() else {
        ui.label(tl!("Open a PDF to measure it."));
        return;
    };
    let mut tool = None;
    let mut pick_area = false;
    let picking_tool = app.quick_tool == QuickTool::Snapshot;
    let Some(doc) = app.session.get(id) else { return };
    let page = app.views[index].current;
    let page_count = doc.info.pages.len();
    let info = doc.info.pages.get(page).cloned();
    let state = &mut app.views[index].measure;
    // A box pick ends when another tool is taken up.
    if !picking_tool {
        state.area_pick = false;
    }
    if state.listing.as_ref().is_none_or(|(g, _)| *g != doc.edit_generation()) {
        state.listing = Some((doc.edit_generation(), doc.measurements()));
    }
    if state.seeded_page != Some(page) {
        state.seeded_page = Some(page);
        if let (Ok(a), Ok(b)) = (doc.measurement_to_user(page, [0.0, 0.0]), doc.measurement_to_user(page, [72.0, 0.0]))
            && let Ok(scale) = doc.measurement_scale(page, a)
            && let Ok(reading) = measure::reading(Kind::Distance, &[a, b], &scale)
        {
            state.drawing_points = 72.0;
            state.real_distance = reading.value;
            state.unit = scale.unit;
            state.precision = scale.precision;
            state.fractions = match scale.format {
                measure::LengthFormat::FeetInches { denominator } => denominator,
                measure::LengthFormat::Decimal => 0,
            };
        }
    }
    let muted = |text: &str| egui::RichText::new(text).size(12.0).color(t.text_muted);
    // Header.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Scale")).font(crate::theme::semibold(15.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let of = crate::i18n::fmt(tl!("Page {n} of {total}"), &[("n", &(page + 1).to_string()), ("total", &page_count.to_string())]);
            ui.label(muted(&of));
        });
    });
    ui.add_space(6.0);
    // The page's scale.
    let current = doc.measurement_to_user(page, [0.0, 0.0]).ok().and_then(|a| {
        let s = doc.measurement_scale(page, a).ok()?;
        Some((s.ratio.clone(), doc.real_size_points(page, a, 1.0).is_some()))
    });
    let formats = length_formats();
    let shown_as = formats
        .iter()
        .find(|(_, unit, d, _)| *unit == state.unit && *d == state.fractions)
        .map_or_else(|| format!("{} ({})", tl!("Custom"), state.unit), |f| f.0.clone());
    egui::Frame::NONE
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(tl!("THIS PAGE")).size(11.0).strong().color(t.text_faint));
            match &current {
                Some((ratio, true)) => {
                    ui.label(egui::RichText::new(ratio).font(crate::theme::semibold(19.0)));
                    ui.label(muted(&shown_as));
                }
                _ => {
                    ui.label(egui::RichText::new(tl!("Not calibrated (paper size)")).font(crate::theme::semibold(16.0)));
                    ui.label(muted(tl!("Choose a scale below, or calibrate from the drawing.")));
                }
            }
        });
    // A measurement being drawn.
    if let Some(reading) = &state.reading {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&reading.label).strong());
            ui.label(muted(&format!("{:.1}°", reading.angle)));
        });
    }
    ui.add_space(10.0);
    // How to set it.
    ui.columns(3, |cols| {
        for (col, (label, value)) in
            cols.iter_mut().zip([(tl!("Preset"), SetBy::Preset), (tl!("Custom"), SetBy::Custom), (tl!("Calibrate"), SetBy::Calibrate)])
        {
            col.vertical_centered_justified(|ui| {
                if ui.selectable_label(state.set_by == value, label).clicked() {
                    state.set_by = value;
                }
            });
        }
    });
    ui.add_space(6.0);
    match state.set_by {
        SetBy::Preset => {
            ui.label(muted(tl!("Drawing scale")));
            let preset = PRESETS.iter().find(|p| {
                (p.paper_points - state.drawing_points).abs() < 1e-6
                    && (p.real - state.real_distance).abs() < 1e-9
                    && p.unit == state.unit
                    && p.fractions == state.fractions
            });
            egui::ComboBox::from_id_salt("measure-scale-preset")
                .selected_text(preset.map_or_else(|| tl!("Custom").to_string(), |p| p.name.to_string()))
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for p in &PRESETS {
                        if ui.selectable_label(preset.is_some_and(|c| c.name == p.name), p.name).clicked() {
                            state.drawing_points = p.paper_points;
                            state.real_distance = p.real;
                            state.unit = p.unit.into();
                            state.fractions = p.fractions;
                        }
                    }
                });
        }
        SetBy::Custom => {
            ui.label(muted(tl!("On paper = in real life")));
            ui.horizontal(|ui| {
                let k = if state.paper_mm { 72.0 / 25.4 } else { 72.0 };
                let mut paper = state.drawing_points / k;
                if ui.add(egui::DragValue::new(&mut paper).range(0.001..=1e6).speed(0.05)).changed() {
                    state.drawing_points = (paper * k).max(1e-6);
                }
                egui::ComboBox::from_id_salt("measure-paper-unit").selected_text(if state.paper_mm { "mm" } else { "in" }).width(52.0).show_ui(
                    ui,
                    |ui| {
                        ui.selectable_value(&mut state.paper_mm, false, "in");
                        ui.selectable_value(&mut state.paper_mm, true, "mm");
                    },
                );
                ui.label("=");
                ui.add(egui::DragValue::new(&mut state.real_distance).range(0.000001..=1e12).speed(0.1));
                ui.label(state.unit.as_str());
            });
        }
        SetBy::Calibrate => {
            for (n, step) in [
                ("1", tl!("Click two points a known distance apart, like the ends of a scale bar or a door.")),
                ("2", tl!("Type the real distance, like 3' or 36\".")),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(n).strong().color(t.accent));
                    ui.label(step);
                });
            }
            if widgets::pill_button(ui, tl!("Pick two points"), false).clicked() {
                tool = Some(Tool::Calibrate);
            }
        }
    }
    ui.add_space(8.0);
    // How lengths read.
    ui.label(muted(tl!("Show lengths as")));
    egui::ComboBox::from_id_salt("measure-length-format").selected_text(&shown_as).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
        for (label, unit, fractions, precision) in &formats {
            let on = state.unit == *unit && state.fractions == *fractions;
            if ui.selectable_label(on, label.as_str()).clicked() && !on {
                // Keep the scale: the real length is restated in the new unit.
                if let (Some(old), Some(new)) = (inches_per(&state.unit), inches_per(unit)) {
                    let v = state.real_distance * old / new;
                    if v.is_finite() && v > 0.0 {
                        state.real_distance = v;
                    }
                }
                state.unit = (*unit).to_string();
                state.fractions = *fractions;
                state.precision = *precision;
            }
        }
    });
    ui.add_space(8.0);
    // Where it goes.
    ui.label(muted(tl!("Use on")));
    ui.columns(3, |cols| {
        for (col, (label, value)) in
            cols.iter_mut().zip([(tl!("This page"), UseOn::Page), (tl!("All pages"), UseOn::All), (tl!("An area"), UseOn::Area)])
        {
            col.vertical_centered_justified(|ui| {
                if ui.selectable_label(state.use_on == value, label).clicked() {
                    state.use_on = value;
                    if value == UseOn::Area && state.whole_page {
                        pick_area = true;
                    }
                }
            });
        }
    });
    state.whole_page = state.use_on != UseOn::Area;
    if state.use_on == UseOn::Area {
        if state.area_pick {
            ui.label(muted(tl!("Drag a box around the detail or viewport that has this scale.")));
        } else {
            let [_, _, w, h] = state.rect;
            ui.horizontal(|ui| {
                ui.label(muted(&format!("{} {:.0} × {:.0} pt", tl!("Area:"), w, h)));
                if ui.link(tl!("Draw again")).clicked() {
                    pick_area = true;
                }
            });
        }
    }
    ui.add_space(8.0);
    let apply = ui
        .add_enabled_ui(doc.allows_annotation() && !(state.use_on == UseOn::Area && state.area_pick), |ui| {
            widgets::pill_button(ui, tl!("Apply scale"), true)
        })
        .inner
        .clicked();
    if let Some(error) = &state.error {
        ui.colored_label(Color32::from_rgb(190, 50, 50), error);
    }
    egui::CollapsingHeader::new(tl!("More options")).id_salt("measure-more").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(tl!("Label"));
            ui.add(egui::TextEdit::singleline(&mut state.label).desired_width(140.0).char_limit(128).hint_text(tl!("Next measurement")));
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Viewport"));
            ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(140.0).char_limit(128));
        });
        if state.fractions == 0 {
            ui.add(egui::DragValue::new(&mut state.precision).range(0..=6).prefix(format!("{} ", tl!("Decimals"))));
        }
    });
    ui.add_space(4.0);
    ui.separator();
    // Measurements.
    let mut select = None;
    let mut export = false;
    match state.listing.as_ref().map(|(_, l)| l) {
        None => {}
        Some(Ok(listing)) => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tl!("Measurements")).strong());
                ui.label(muted(&listing.measurements.len().to_string()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    export = ui.link(tl!("Export CSV")).on_hover_text(tl!("Export measurements as CSV")).clicked();
                });
            });
            if !listing.unsupported.is_empty() {
                let reasons: Vec<String> =
                    listing.unsupported.iter().map(|u| format!("{} {}: {}", tl!("Page"), u.page.saturating_add(1), u.reason)).collect();
                ui.colored_label(Color32::from_rgb(190, 120, 30), format!("{}: {}", tl!("Unsupported measurements"), listing.unsupported.len()))
                    .on_hover_text(reasons.join("\n"));
            }
            if listing.measurements.is_empty() {
                ui.label(muted(tl!("Measurements you make appear here.")));
            }
            egui::ScrollArea::vertical().id_salt("measurement-list").max_height(220.0).show(ui, |ui| {
                for m in &listing.measurements {
                    let kind = match m.reading.kind {
                        Kind::Distance => tl!("Distance"),
                        Kind::Perimeter => tl!("Perimeter"),
                        Kind::Area => tl!("Area"),
                    };
                    let name = if m.label.is_empty() { kind.to_string() } else { format!("{kind} · {}", m.label) };
                    let row = ui.horizontal(|ui| {
                        let r = ui.selectable_label(false, name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(&m.reading.label).strong());
                            if page_count > 1 {
                                ui.label(muted(&format!("p. {}", m.page.saturating_add(1))));
                            }
                        });
                        r
                    });
                    if row.inner.clicked() {
                        select = Some((m.page, m.index, m.reading.clone(), m.scale.clone()));
                    }
                }
            });
        }
        Some(Err(e)) => {
            ui.colored_label(Color32::from_rgb(190, 50, 50), e);
        }
    }
    if apply && let Some(info) = info {
        let pages: Vec<usize> = if state.use_on == UseOn::All { (0..page_count).collect() } else { vec![page] };
        let mut edits = Vec::new();
        let mut failed = None;
        for p in pages {
            let Some(pinfo) = (if p == page { Some(info.clone()) } else { doc.info.pages.get(p).cloned() }) else { continue };
            let result =
                doc.measurement_to_user(p, [0.0, 0.0]).and_then(|a| doc.measurement_to_user(p, [1.0, 0.0]).map(|b| (a, b))).and_then(|(a, b)| {
                    let scale = Scale::calibrate(a, b, state.real_distance / state.drawing_points, &state.unit, state.precision);
                    match state.fractions {
                        0 => scale,
                        d => scale.and_then(|s| s.with_feet_inches(d)),
                    }
                    .map_err(|e| e.to_string())
                });
            match result {
                Ok(scale) => {
                    let [x, y, w, h] =
                        if state.whole_page || p != page { [0.0, 0.0, f64::from(pinfo.width), f64::from(pinfo.height)] } else { state.rect };
                    let a = pinfo.view_to_user(x as f32, y as f32);
                    let b = pinfo.view_to_user((x + w) as f32, (y + h) as f32);
                    edits.push(Edit::SetMeasurementScale {
                        page: p,
                        bbox: [f64::from(a[0].min(b[0])), f64::from(a[1].min(b[1])), f64::from(a[0].max(b[0])), f64::from(a[1].max(b[1]))],
                        name: state.name.clone(),
                        scale,
                    });
                }
                Err(e) => {
                    failed = Some(e.to_string());
                    break;
                }
            }
        }
        match failed {
            Some(e) => state.error = Some(e),
            None => {
                state.error = None;
                app.views[index].pending_edit = match edits.len() {
                    0 => None,
                    1 => edits.pop(),
                    _ => Some(Edit::Batch { label: "Set measurement scale".into(), edits }),
                };
            }
        }
    }
    if let Some((page, annotation, reading, scale)) = select {
        app.views[index].current = page;
        app.views[index].comments.selected = Some((page, annotation));
        app.views[index].measure.reading = Some(reading);
        app.views[index].measure.scale = Some(scale);
        app.quick_tool = QuickTool::Select;
    }
    if pick_area {
        app.views[index].measure.area_pick = true;
        app.quick_tool = QuickTool::Snapshot;
    }
    if let Some(tool) = tool {
        app.views[index].measure.cancel();
        app.quick_tool = QuickTool::Measure(tool);
    }
    if export {
        export_csv(app);
    }
}
pub(crate) fn command(app: &mut PdfCraftApp, id: &str) {
    app.left = LeftPanel::Tool("measure");
    app.left_open = true;
    if let Some((index, _)) = app.active_ids()
        && let Some(tool) = id.strip_prefix("measure.").and_then(Tool::from_name)
    {
        app.views[index].measure.cancel();
        app.quick_tool = QuickTool::Measure(tool);
    }
    if id == "measure.export" {
        export_csv(app);
    }
}
fn export_csv(app: &mut PdfCraftApp) {
    let Some((_, id)) = app.active_ids() else { return };
    let result = app.session.get(id).ok_or_else(|| "no such document".to_string()).and_then(|d| d.measurements());
    match result {
        Err(e) => app.notify(&e),
        Ok(listing) => {
            let csv = measure::csv(&listing.measurements);
            if !listing.unsupported.is_empty() {
                app.notify(crate::i18n::fmt(
                    tl!("{n} measurements use an unsupported format and are left out."),
                    &[("n", &listing.unsupported.len().to_string())],
                ));
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let write = move |app: &mut PdfCraftApp, path: std::path::PathBuf| match crate::editing::write_atomically(
                    &path.to_string_lossy(),
                    csv.as_bytes(),
                ) {
                    Ok(()) => app.notify(tl!("Saved measurements.")),
                    Err(e) => app.notify(e.to_string()),
                };
                match app.export_dir_override.as_ref().map(|d| std::path::PathBuf::from(d).join("measurements.csv")) {
                    Some(path) => write(app, path),
                    None => {
                        let dialog = crate::dialog::AsyncFileDialog::new().set_file_name("measurements.csv").add_filter("CSV", &["csv"]);
                        app.ask_one(crate::pickers::Ask::Save(dialog), None, write);
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            if let Err(e) = crate::editing::download("measurements.csv", csv.as_bytes()) {
                app.notify(&e);
            }
        }
    }
}
