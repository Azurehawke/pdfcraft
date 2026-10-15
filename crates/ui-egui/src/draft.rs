//! RevuCraft's Draft tab: CAD-style drawing at scale on PDF sheets. New sheets in architectural
//! (ARCH A–E) and US standard (ANSI A–E) sizes with a drawing scale; lines, polylines,
//! rectangles and walls placed by clicking (with snapping and ortho) or by typing lengths and
//! coordinates as in CAD (`15'`, `12'6"`, `@0,15`, `@15<90`, `x,y`); a keypad for the tablet.
//! What is drawn is ordinary markup (lines, polylines and polygons) on the current layer, so it
//! stays editable, measurable and visible in any PDF viewer.

use egui::{Color32, Pos2, Stroke, vec2};
use pdfcraft_engine::{Document, Edit, LineEnding, NewAnnotation, Shape, Style, measure};

use crate::canvas::DocView;
use crate::comments::PageCx;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, icons};

/// Points per inch on paper.
const PT_PER_IN: f64 = 72.0;
/// Snap radius on screen, in pixels.
const SNAP_PX: f64 = 10.0;
/// Typed entries are short; anything longer is cut.
const MAX_INPUT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DraftTool {
    Line,
    Polyline,
    Rectangle,
    Wall,
}

impl DraftTool {
    pub const ALL: [DraftTool; 4] = [DraftTool::Line, DraftTool::Polyline, DraftTool::Rectangle, DraftTool::Wall];

    pub fn name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Polyline => "polyline",
            Self::Rectangle => "rectangle",
            Self::Wall => "wall",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == s)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Polyline => "Polyline",
            Self::Rectangle => "Rectangle",
            Self::Wall => "Wall",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Line => "minus",
            Self::Polyline => "spline",
            Self::Rectangle => "square",
            Self::Wall => "rectangle-horizontal",
        }
    }

    /// Placed point by point until finished (rather than ending at the second point).
    fn open_ended(self) -> bool {
        matches!(self, Self::Polyline | Self::Wall)
    }
}

/// What a typed number without a unit means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BareUnit {
    Feet,
    Inches,
}

/// A paper size, in inches (portrait).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paper {
    pub name: &'static str,
    pub width: f64,
    pub height: f64,
}

pub const PAPERS: [Paper; 10] = [
    Paper { name: "ANSI A (Letter, 8½ × 11 in)", width: 8.5, height: 11.0 },
    Paper { name: "ANSI B (Tabloid, 11 × 17 in)", width: 11.0, height: 17.0 },
    Paper { name: "ANSI C (17 × 22 in)", width: 17.0, height: 22.0 },
    Paper { name: "ANSI D (22 × 34 in)", width: 22.0, height: 34.0 },
    Paper { name: "ANSI E (34 × 44 in)", width: 34.0, height: 44.0 },
    Paper { name: "ARCH A (9 × 12 in)", width: 9.0, height: 12.0 },
    Paper { name: "ARCH B (12 × 18 in)", width: 12.0, height: 18.0 },
    Paper { name: "ARCH C (18 × 24 in)", width: 18.0, height: 24.0 },
    Paper { name: "ARCH D (24 × 36 in)", width: 24.0, height: 36.0 },
    Paper { name: "ARCH E (36 × 48 in)", width: 36.0, height: 48.0 },
];

/// The New sheet dialog's choices.
#[derive(Clone, Debug)]
pub struct SheetDraft {
    pub open: bool,
    pub paper: usize,
    pub landscape: bool,
    /// Index into the measuring presets (`measure_ui::PRESETS`).
    pub scale: usize,
    /// Add the sheet to the open document rather than making a new one.
    pub add_to_document: bool,
}

/// Drafting settings, shared by every document.
#[derive(Clone, Debug)]
pub struct DraftSettings {
    pub ortho: bool,
    /// Wall thickness, inches.
    pub wall_inches: f64,
    pub bare: BareUnit,
    /// The on-screen keypad is shown while drafting.
    pub keypad: bool,
    pub sheet: SheetDraft,
}

impl Default for DraftSettings {
    fn default() -> Self {
        Self {
            ortho: false,
            wall_inches: 4.5,
            bare: BareUnit::Feet,
            keypad: cfg!(target_os = "android"),
            // ARCH D landscape at 1/4" = 1'-0".
            sheet: SheetDraft { open: false, paper: 8, landscape: true, scale: 4, add_to_document: false },
        }
    }
}

/// Drafting state per document: the shape being placed and the typed entry.
#[derive(Clone, Debug, Default)]
pub struct DraftView {
    pub tool: Option<DraftTool>,
    pub page: Option<usize>,
    pub points: Vec<[f64; 2]>,
    /// What has been typed (a length or coordinates), applied with Enter.
    pub input: String,
    pub error: Option<String>,
    /// The drawing's origin (page, user-space point); the page's lower-left corner when unset.
    pub origin: Option<(usize, [f64; 2])>,
    /// The next click sets the origin.
    pub setting_origin: bool,
    /// Snapping geometry of the page content, per (page, edit generation).
    paths: Option<(usize, u64, Option<measure::snap::Geometry>)>,
}

impl DraftView {
    pub fn cancel(&mut self) {
        self.points.clear();
        self.page = None;
        self.input.clear();
    }
}

/// The drawing origin on `page` in user space.
pub fn origin(view: &DocView, doc: &Document, page: usize) -> [f64; 2] {
    if let Some((p, o)) = view.draft.origin
        && p == page
    {
        return o;
    }
    doc.info.pages.get(page).map_or([0.0, 0.0], |info| {
        let u = info.view_to_user(0.0, info.height);
        [f64::from(u[0]), f64::from(u[1])]
    })
}

// ---- lengths and coordinates -------------------------------------------------------------------

/// A number, a fraction (`1/2`) or a mixed number (`6 1/2`).
fn parse_number(s: &str) -> Option<f64> {
    let mut total = 0.0;
    let mut any = false;
    for part in s.split_whitespace() {
        let v = match part.split_once('/') {
            Some((a, b)) => {
                let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
                if b == 0.0 {
                    return None;
                }
                a / b
            }
            None => part.parse::<f64>().ok()?,
        };
        total += v;
        any = true;
    }
    (any && total.is_finite()).then_some(total)
}

/// A length in inches: `15'`, `15'6"`, `15'-6 1/2"`, `6"`, `6 1/2"`, `1/2"`, `15.5` (in the
/// bare unit), `3m`, `300mm`, `30cm`. A leading `-` makes it negative.
pub fn parse_length(s: &str, bare: BareUnit) -> Option<f64> {
    let s = s.trim().to_ascii_lowercase();
    let (sign, s) = match s.strip_prefix('-') {
        Some(rest) => (-1.0, rest.trim()),
        None => (1.0, s.as_str()),
    };
    if s.is_empty() {
        return None;
    }
    let metric = [("mm", 1.0 / 25.4), ("cm", 10.0 / 25.4), ("m", 1000.0 / 25.4)];
    for (suffix, inches) in metric {
        if let Some(n) = s.strip_suffix(suffix) {
            return n.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| sign * v * inches);
        }
    }
    let value = match s.split_once('\'') {
        Some((feet, rest)) => {
            let feet = if feet.trim().is_empty() { 0.0 } else { parse_number(feet)? };
            let rest = rest.trim().trim_start_matches('-').trim().trim_end_matches('"').trim();
            let inches = if rest.is_empty() { 0.0 } else { parse_number(rest)? };
            feet * 12.0 + inches
        }
        None => match s.strip_suffix('"') {
            Some(inches) => parse_number(inches)?,
            None => {
                let v = parse_number(s)?;
                match bare {
                    BareUnit::Feet => v * 12.0,
                    BareUnit::Inches => v,
                }
            }
        },
    };
    let v = sign * value;
    (v.is_finite() && v.abs() < 1e7).then_some(v)
}

/// A typed point, lengths in inches, angles in degrees counter-clockwise from east.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entry {
    /// `@dx,dy`: from the last point.
    Relative([f64; 2]),
    /// `@length<angle`: from the last point.
    RelativePolar(f64, f64),
    /// `x,y`: from the origin.
    Absolute([f64; 2]),
    /// `length<angle`: from the origin.
    AbsolutePolar(f64, f64),
    /// A length alone: toward the pointer from the last point.
    Distance(f64),
}

pub fn parse_entry(s: &str, bare: BareUnit) -> Option<Entry> {
    let s = s.trim();
    let (relative, s) = match s.strip_prefix('@') {
        Some(rest) => (true, rest.trim()),
        None => (false, s),
    };
    if let Some((a, b)) = s.split_once(',') {
        let p = [parse_length(a, bare)?, parse_length(b, bare)?];
        return Some(if relative { Entry::Relative(p) } else { Entry::Absolute(p) });
    }
    if let Some((len, angle)) = s.split_once('<') {
        let len = parse_length(len, bare)?;
        let angle = angle.trim().trim_end_matches('°').parse::<f64>().ok().filter(|a| a.is_finite())?;
        return Some(if relative { Entry::RelativePolar(len, angle) } else { Entry::AbsolutePolar(len, angle) });
    }
    let len = parse_length(s, bare)?;
    Some(if relative { Entry::Relative([len, 0.0]) } else { Entry::Distance(len) })
}

/// User-space units per real inch on `page` near `at`: from the page's scale, or paper inches
/// when the page has none.
fn units_per_inch(doc: &Document, page: usize, at: [f64; 2]) -> f64 {
    doc.real_size_points(page, at, 1.0).unwrap_or(PT_PER_IN)
}

/// Keep `to` horizontal or vertical from `from`.
fn orthogonal(from: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    if (to[0] - from[0]).abs() >= (to[1] - from[1]).abs() { [to[0], from[1]] } else { [from[0], to[1]] }
}

/// Where a typed entry puts the next point (user space).
fn entry_point(entry: Entry, last: Option<[f64; 2]>, origin: [f64; 2], toward: Option<[f64; 2]>, k: f64, ortho: bool) -> Option<[f64; 2]> {
    let polar = |from: [f64; 2], len: f64, deg: f64| {
        let r = deg.to_radians();
        [from[0] + len * k * r.cos(), from[1] + len * k * r.sin()]
    };
    let base = last.unwrap_or(origin);
    let p = match entry {
        Entry::Relative([dx, dy]) => [base[0] + dx * k, base[1] + dy * k],
        Entry::RelativePolar(len, deg) => polar(base, len, deg),
        Entry::Absolute([x, y]) => [origin[0] + x * k, origin[1] + y * k],
        Entry::AbsolutePolar(len, deg) => polar(origin, len, deg),
        Entry::Distance(len) => {
            let last = last?;
            let mut to = toward.unwrap_or([last[0] + 1.0, last[1]]);
            if ortho {
                to = orthogonal(last, to);
            }
            let (dx, dy) = (to[0] - last[0], to[1] - last[1]);
            let d = dx.hypot(dy);
            if d < 1e-9 { [last[0] + len * k, last[1]] } else { [last[0] + dx / d * len * k, last[1] + dy / d * len * k] }
        }
    };
    p.iter().all(|v| v.is_finite() && v.abs() < 1e7).then_some(p)
}

// ---- making markup -----------------------------------------------------------------------------

fn layer_color(doc: &Document) -> [f64; 3] {
    doc.current_layer.and_then(|c| doc.layer_list().into_iter().find(|l| (l.id.num, l.id.generation) == c)).map_or([0.0; 3], |l| l.color)
}

fn annotation(page: usize, shape: Shape, color: [f64; 3], width: f64, fill: Option<[f64; 3]>, author: &str) -> Edit {
    Edit::AddAnnotation(NewAnnotation {
        page,
        shape,
        style: Style { color, opacity: 1.0, width, fill },
        contents: String::new(),
        author: author.to_string(),
    })
}

/// Wall outlines: each segment of the centre line as a filled quadrilateral `thickness` wide,
/// mitred at the corners.
pub fn wall_quads(points: &[[f64; 2]], thickness: f64, closed: bool) -> Vec<[[f64; 2]; 4]> {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for p in points {
        if pts.last().is_none_or(|l| (l[0] - p[0]).hypot(l[1] - p[1]) > 1e-6) {
            pts.push(*p);
        }
    }
    if closed && pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6) {
        pts.pop();
    }
    let n = pts.len();
    if n < 2 || !thickness.is_finite() || thickness <= 0.0 {
        return Vec::new();
    }
    let half = thickness / 2.0;
    let normal = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let d = dx.hypot(dy).max(1e-12);
        [-dy / d, dx / d]
    };
    let get = |i: isize| -> [f64; 2] { pts[i.rem_euclid(n as isize) as usize] };
    // The offset of each vertex, left of the direction of travel.
    let offsets: Vec<[f64; 2]> = (0..n as isize)
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
                    if dot < 0.25 {
                        // A hairpin: no mitre spike.
                        [a[0] * half, a[1] * half]
                    } else {
                        [m[0] / len * half / dot, m[1] / len * half / dot]
                    }
                }
                (Some(a), None) | (None, Some(a)) => [a[0] * half, a[1] * half],
                (None, None) => [0.0, 0.0],
            }
        })
        .collect();
    let segments = if closed { n } else { n - 1 };
    (0..segments)
        .map(|i| {
            let j = (i + 1) % n;
            let (a, b) = (pts[i], pts[j]);
            let (oa, ob) = (offsets[i], offsets[j]);
            [[a[0] + oa[0], a[1] + oa[1]], [b[0] + ob[0], b[1] + ob[1]], [b[0] - ob[0], b[1] - ob[1]], [a[0] - oa[0], a[1] - oa[1]]]
        })
        .collect()
}

/// The markup for a finished shape (none when it has too few points).
fn finish_edit(
    tool: DraftTool,
    page: usize,
    points: &[[f64; 2]],
    closed: bool,
    doc: &Document,
    settings: &DraftSettings,
    author: &str,
) -> Option<Edit> {
    let color = layer_color(doc);
    match tool {
        DraftTool::Line => match points {
            [a, b, ..] => {
                Some(annotation(page, Shape::Line { from: *a, to: *b, start: LineEnding::None, end: LineEnding::None }, color, 1.0, None, author))
            }
            _ => None,
        },
        DraftTool::Rectangle => match points {
            [a, b, ..] if (a[0] - b[0]).abs() > 1e-6 && (a[1] - b[1]).abs() > 1e-6 => {
                let vertices = vec![*a, [b[0], a[1]], *b, [a[0], b[1]]];
                Some(annotation(page, Shape::Polygon { vertices, cloud: false }, color, 1.0, None, author))
            }
            _ => None,
        },
        DraftTool::Polyline => {
            if closed && points.len() >= 3 {
                Some(annotation(page, Shape::Polygon { vertices: points.to_vec(), cloud: false }, color, 1.0, None, author))
            } else if points.len() >= 2 {
                Some(annotation(
                    page,
                    Shape::PolyLine { vertices: points.to_vec(), start: LineEnding::None, end: LineEnding::None },
                    color,
                    1.0,
                    None,
                    author,
                ))
            } else {
                None
            }
        }
        DraftTool::Wall => {
            let at = points.first().copied().unwrap_or([0.0, 0.0]);
            let thickness = settings.wall_inches * units_per_inch(doc, page, at);
            let quads = wall_quads(points, thickness, closed);
            let edits: Vec<Edit> = quads
                .into_iter()
                .map(|q| annotation(page, Shape::Polygon { vertices: q.to_vec(), cloud: false }, color, 0.75, Some([0.78, 0.78, 0.78]), author))
                .collect();
            (!edits.is_empty()).then(|| Edit::Batch { label: "Draw wall".into(), edits })
        }
    }
}

/// Add `p` to the shape being placed; returns the markup when that completes a shape.
fn add_point(view: &mut DocView, doc: &Document, page: usize, tool: DraftTool, p: [f64; 2], settings: &DraftSettings, author: &str) -> Option<Edit> {
    let d = &mut view.draft;
    if d.page != Some(page) {
        d.points.clear();
        d.page = Some(page);
    }
    if d.points.last().is_some_and(|l| (l[0] - p[0]).hypot(l[1] - p[1]) < 1e-6) {
        return None;
    }
    if d.points.len() >= 4096 {
        return None;
    }
    d.points.push(p);
    match tool {
        DraftTool::Line if d.points.len() >= 2 => {
            let edit = finish_edit(tool, page, &d.points, false, doc, settings, author);
            // Lines chain: the next one starts where this one ended.
            d.points = vec![p];
            edit
        }
        DraftTool::Rectangle if d.points.len() >= 2 => {
            let edit = finish_edit(tool, page, &d.points, false, doc, settings, author);
            d.points.clear();
            d.page = None;
            edit
        }
        _ => None,
    }
}

/// Finish an open-ended shape (polyline, wall), closed or not.
fn finish(view: &mut DocView, doc: &Document, tool: DraftTool, closed: bool, settings: &DraftSettings, author: &str) -> Option<Edit> {
    let d = &mut view.draft;
    let page = d.page?;
    let edit = if tool.open_ended() { finish_edit(tool, page, &d.points, closed, doc, settings, author) } else { None };
    d.points.clear();
    d.page = None;
    edit
}

// ---- input -------------------------------------------------------------------------------------

/// Apply the typed entry: the next point.
pub fn submit(view: &mut DocView, doc: &Document, tool: DraftTool, settings: &DraftSettings, author: &str) -> Option<Edit> {
    let text = std::mem::take(&mut view.draft.input);
    if text.trim().is_empty() {
        // Enter with nothing typed finishes a polyline or wall.
        return finish(view, doc, tool, false, settings, author);
    }
    let page = view.draft.page.or(view.cursor.map(|c| c.0)).unwrap_or(view.current);
    let Some(entry) = parse_entry(&text, settings.bare) else {
        view.draft.error = Some(crate::i18n::fmt(tl!("Couldn't read “{text}”. Try 15', 12'6\", @0,15 or @15<90."), &[("text", &text)]));
        return None;
    };
    view.draft.error = None;
    let origin = origin(view, doc, page);
    let last = if view.draft.page == Some(page) { view.draft.points.last().copied() } else { None };
    let toward = view.cursor.filter(|c| c.0 == page).map(|c| c.1);
    let k = units_per_inch(doc, page, last.unwrap_or(origin));
    let Some(p) = entry_point(entry, last, origin, toward, k, settings.ortho) else {
        view.draft.error = Some(tl!("Type a length after placing the first point, or give coordinates.").to_string());
        return None;
    };
    add_point(view, doc, page, tool, p, settings, author)
}

/// Keys while drafting (when no text field has focus): typing goes to the entry, Enter applies
/// it, Backspace edits it or removes the last point, Escape keeps what is drawn.
pub fn keys(ctx: &egui::Context, doc: &Document, view: &mut DocView, tool: DraftTool, settings: &DraftSettings, author: &str) -> Option<Edit> {
    if view.draft.tool != Some(tool) {
        view.draft.cancel();
        view.draft.tool = Some(tool);
    }
    if ctx.egui_wants_keyboard_input() {
        return None;
    }
    let mut out = None;
    ctx.input(|i| {
        for e in &i.events {
            if let egui::Event::Text(t) = e {
                for c in t.chars().filter(|c| c.is_ascii_digit() || " .'\"@,<-/mc°".contains(*c)) {
                    if view.draft.input.chars().count() < MAX_INPUT {
                        view.draft.input.push(c);
                    }
                }
            }
        }
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        out = submit(view, doc, tool, settings, author);
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
        if view.draft.input.pop().is_none() {
            view.draft.points.pop();
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        if !view.draft.input.is_empty() {
            view.draft.input.clear();
        } else {
            out = finish(view, doc, tool, false, settings, author);
        }
    }
    out
}

// ---- the page ----------------------------------------------------------------------------------

/// Drafting on one page: snapping, clicks, the shape so far, the rubber band and its length.
pub(crate) fn page_input(
    ui: &egui::Ui,
    resp: &egui::Response,
    doc: &Document,
    view: &mut DocView,
    cx: &PageCx<'_>,
    tool: DraftTool,
    settings: &DraftSettings,
    author: &str,
) {
    let page = cx.page;
    let xf = cx.xf;
    if !doc.allows_annotation() {
        return;
    }
    let pos = ui.input(|i| i.pointer.hover_pos()).filter(|p| xf.rect.contains(*p));
    let pixels_per_unit = f64::from((xf.rect.width() / xf.pw).max(1e-6));
    let tolerance = SNAP_PX / pixels_per_unit;
    let accent = Color32::from_rgb(242, 163, 58);
    let stroke = Stroke::new(1.5, accent);
    let mut hover: Option<[f64; 2]> = None;
    if let Some(pos) = pos {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        let mut point = cx.to_user(pos);
        // Snap: the shape's own points and other markup first, then the page's drawing.
        if view.measure.snap_enabled {
            let mut best: Option<([f64; 2], f64)> = None;
            let own = view.draft.points.iter().copied().filter(|_| view.draft.page == Some(page));
            for v in own.chain(doc.markup_vertices(page)) {
                let d = (v[0] - point[0]).hypot(v[1] - point[1]);
                if d <= tolerance && best.is_none_or(|(_, b)| d < b) {
                    best = Some((v, d));
                }
            }
            match best {
                Some((v, _)) => point = v,
                None => {
                    if view.draft.paths.as_ref().is_none_or(|(p, g, _)| *p != page || *g != doc.edit_generation()) {
                        view.draft.paths = Some((page, doc.edit_generation(), doc.measurement_paths(page).ok()));
                    }
                    if let Some((_, _, Some(paths))) = &view.draft.paths
                        && let Ok(Some(hit)) = paths.snap(point, tolerance, view.measure.snaps)
                    {
                        point = hit.point;
                    }
                }
            }
        }
        let shift = ui.input(|i| i.modifiers.shift);
        if let Some(last) = view.draft.points.last().filter(|_| view.draft.page == Some(page))
            && settings.ortho != shift
        {
            point = orthogonal(*last, point);
        }
        hover = Some(point);
        if resp.clicked() {
            if view.draft.setting_origin {
                view.draft.origin = Some((page, point));
                view.draft.setting_origin = false;
            } else {
                let closes = tool.open_ended()
                    && view.draft.page == Some(page)
                    && view.draft.points.len() >= 3
                    && view.draft.points.first().is_some_and(|f| cx.to_screen(*f).distance(pos) <= crate::point_bar::CLOSE_RADIUS);
                let edit =
                    if closes { finish(view, doc, tool, true, settings, author) } else { add_point(view, doc, page, tool, point, settings, author) };
                if edit.is_some() {
                    view.pending_edit = edit;
                }
            }
        }
        if resp.double_clicked() && tool.open_ended() && view.draft.page == Some(page) {
            let edit = finish(view, doc, tool, false, settings, author);
            if edit.is_some() {
                view.pending_edit = edit;
            }
        }
    }
    let painter = ui.painter();
    // The origin marker.
    let o = origin(view, doc, page);
    let os = cx.to_screen(o);
    if xf.rect.expand(8.0).contains(os) {
        painter.line_segment([os - vec2(8.0, 0.0), os + vec2(8.0, 0.0)], Stroke::new(1.0, accent.gamma_multiply(0.6)));
        painter.line_segment([os - vec2(0.0, 8.0), os + vec2(0.0, 8.0)], Stroke::new(1.0, accent.gamma_multiply(0.6)));
    }
    if view.draft.page != Some(page) || view.draft.points.is_empty() {
        if let Some(h) = hover {
            painter.circle_stroke(cx.to_screen(h), 4.0, stroke);
        }
        return;
    }
    let mut pts = view.draft.points.clone();
    if let Some(h) = hover {
        pts.push(h);
    }
    let screen: Vec<Pos2> = pts.iter().map(|p| cx.to_screen(*p)).collect();
    match tool {
        DraftTool::Rectangle => {
            if let [a, b] = screen[..] {
                painter.rect_stroke(egui::Rect::from_two_pos(a, b), egui::CornerRadius::ZERO, stroke, egui::StrokeKind::Middle);
            }
        }
        DraftTool::Wall => {
            let at = pts.first().copied().unwrap_or([0.0, 0.0]);
            let thickness = settings.wall_inches * units_per_inch(doc, page, at);
            for q in wall_quads(&pts, thickness, false) {
                painter.add(egui::Shape::closed_line(q.iter().map(|p| cx.to_screen(*p)).collect(), stroke));
            }
            painter.extend(egui::Shape::dashed_line(&screen, Stroke::new(1.0, accent.gamma_multiply(0.6)), 6.0, 4.0));
        }
        _ => {
            painter.add(egui::Shape::line(screen.clone(), stroke));
        }
    }
    for p in &screen {
        painter.circle_filled(*p, 3.0, accent);
    }
    // The first point closes a polyline or wall.
    if tool.open_ended()
        && view.draft.points.len() >= 3
        && let Some(first) = screen.first()
    {
        let near = pos.is_some_and(|p| p.distance(*first) <= crate::point_bar::CLOSE_RADIUS);
        painter.circle_stroke(*first, if near { crate::point_bar::CLOSE_RADIUS } else { 7.0 }, stroke);
    }
    // The rubber band's length and angle, as the scale writes lengths.
    if let (Some(h), Some(last)) = (hover, view.draft.points.last())
        && let Ok(scale) = doc.measurement_scale(page, *last)
    {
        let (dx, dy) = (h[0] - last[0], h[1] - last[1]);
        let len = dx.hypot(dy) * scale.x * scale.distance_factor;
        if let Ok(text) = scale.format_length(len) {
            let angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            let at = cx.to_screen(h) + vec2(14.0, 14.0);
            let label = format!("{text}  {angle:.0}°");
            let galley = painter.layout_no_wrap(label, theme::semibold(12.0), Color32::WHITE);
            let r = egui::Rect::from_min_size(at, galley.size()).expand(4.0);
            painter.rect_filled(r, egui::CornerRadius::same(4), Color32::from_black_alpha(190));
            painter.galley(at, galley, Color32::WHITE);
        }
    }
    // Finish, close, undo and cancel for polylines and walls.
    if tool.open_ended()
        && let Some(last) = view.draft.points.last().copied()
    {
        let n = view.draft.points.len();
        match crate::point_bar::show(ui.ctx(), ("draft", page), cx.to_screen(last), ui.clip_rect(), n >= 2, n >= 3) {
            Some(crate::point_bar::PointAction::Finish) => view.pending_edit = finish(view, doc, tool, false, settings, author),
            Some(crate::point_bar::PointAction::Close) => view.pending_edit = finish(view, doc, tool, true, settings, author),
            Some(crate::point_bar::PointAction::Undo) => {
                view.draft.points.pop();
            }
            Some(crate::point_bar::PointAction::Cancel) => view.draft.cancel(),
            None => {}
        }
    }
}

// ---- the ribbon, keypad and New sheet ----------------------------------------------------------

fn group(ui: &mut egui::Ui, t: &Tokens, caption: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        body(ui);
        ui.add(egui::Label::new(egui::RichText::new(caption).font(theme::regular(11.0)).color(t.text_faint)).wrap_mode(egui::TextWrapMode::Extend));
    });
}

fn separator(ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(6.0);
    let x = ui.cursor().left();
    ui.painter().vline(x, ui.max_rect().y_range().shrink(6.0), Stroke::new(1.0, t.divider));
    ui.add_space(6.0);
}

/// The Draft ribbon tab.
pub fn ribbon(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let active = app.active_ids();
    let allowed = active.and_then(|(_, id)| app.session.get(id)).is_some_and(|d| d.allows_annotation() && d.read_only_reason.is_none());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        group(ui, &t, tl!("Sheet"), |ui| {
            if icons::button(ui, "file-plus-2", 34.0, false, tl!("New sheet at a drawing scale")).clicked() {
                app.draft.sheet.open = true;
                app.draft.sheet.add_to_document = active.is_some();
            }
        });
        separator(ui, &t);
        ui.add_enabled_ui(allowed, |ui| {
            group(ui, &t, tl!("Draw"), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for tool in DraftTool::ALL {
                        let selected = app.quick_tool == QuickTool::Draft(tool);
                        if icons::button(ui, tool.icon(), 34.0, selected, tl!(tool.label())).clicked() {
                            app.quick_tool = if selected { QuickTool::Select } else { QuickTool::Draft(tool) };
                        }
                    }
                });
            });
        });
        separator(ui, &t);
        group(ui, &t, tl!("Wall"), |ui| {
            ui.add(egui::DragValue::new(&mut app.draft.wall_inches).range(0.5..=48.0).speed(0.25).suffix(" in")).on_hover_text(tl!("Wall thickness"));
        });
        separator(ui, &t);
        group(ui, &t, tl!("Type a length or coordinates, then Enter"), |ui| {
            ui.horizontal(|ui| {
                let Some((index, id)) = active else { return };
                let tool = match app.quick_tool {
                    QuickTool::Draft(tool) => Some(tool),
                    _ => None,
                };
                let r = ui.add_enabled(
                    tool.is_some(),
                    egui::TextEdit::singleline(&mut app.views[index].draft.input).desired_width(120.0).hint_text("15'6\"  @0,15  @15<90"),
                );
                if r.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && let Some(tool) = tool
                    && let Some(doc) = app.session.get(id)
                {
                    let author = app.comment_prefs.author.clone();
                    let settings = app.draft.clone();
                    let edit = submit(&mut app.views[index], doc, tool, &settings, &author);
                    if edit.is_some() {
                        app.views[index].pending_edit = edit;
                    }
                    r.request_focus();
                }
                egui::ComboBox::from_id_salt("draft-bare-unit")
                    .selected_text(if app.draft.bare == BareUnit::Feet { tl!("ft") } else { tl!("in") })
                    .width(44.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.draft.bare, BareUnit::Feet, tl!("Plain numbers are feet"));
                        ui.selectable_value(&mut app.draft.bare, BareUnit::Inches, tl!("Plain numbers are inches"));
                    });
                if icons::button(ui, "hash", 28.0, app.draft.keypad, tl!("Keypad")).clicked() {
                    app.draft.keypad = !app.draft.keypad;
                }
            });
        });
        separator(ui, &t);
        group(ui, &t, tl!("Origin"), |ui| {
            if let Some((index, _)) = active {
                let setting = app.views[index].draft.setting_origin;
                if icons::button(ui, "plus", 34.0, setting, tl!("Set the origin: click a point on the page")).clicked() {
                    app.views[index].draft.setting_origin = !setting;
                    if !matches!(app.quick_tool, QuickTool::Draft(_)) {
                        app.quick_tool = QuickTool::Draft(DraftTool::Line);
                    }
                }
            }
        });
    });
    if let Some((index, _)) = active
        && let Some(e) = app.views[index].draft.error.clone()
    {
        app.notify(e);
        app.views[index].draft.error = None;
    }
}

/// The keypad for typing lengths and coordinates on a tablet.
pub fn keypad(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let QuickTool::Draft(tool) = app.quick_tool else { return };
    if !app.draft.keypad {
        return;
    }
    let Some((index, id)) = app.active_ids() else { return };
    let t = Tokens::get(ctx);
    let mut pressed: Option<&str> = None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("draft-keypad"))
        .order(egui::Order::Foreground)
        .default_pos(screen.right_bottom() - vec2(330.0, 320.0))
        .movable(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(t.card).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                let shown = if app.views[index].draft.input.is_empty() { "—".to_string() } else { app.views[index].draft.input.clone() };
                ui.label(egui::RichText::new(shown).font(theme::semibold(16.0)));
                let rows: [[&str; 5]; 4] =
                    [["7", "8", "9", "'", "←"], ["4", "5", "6", "\"", "@"], ["1", "2", "3", ",", "<"], ["0", ".", "/", " ", "OK"]];
                egui::Grid::new("draft-keypad-grid").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                    for row in rows {
                        for key in row {
                            let label = if key == " " { "Space" } else { key };
                            if ui.add_sized(vec2(52.0, 44.0), egui::Button::new(egui::RichText::new(label).font(theme::semibold(18.0)))).clicked() {
                                pressed = Some(key);
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        });
    match pressed {
        Some("←") => {
            let d = &mut app.views[index].draft;
            if d.input.pop().is_none() {
                d.points.pop();
            }
        }
        Some("OK") => {
            let author = app.comment_prefs.author.clone();
            let settings = app.draft.clone();
            if let Some(doc) = app.session.get(id) {
                let edit = submit(&mut app.views[index], doc, tool, &settings, &author);
                if edit.is_some() {
                    app.views[index].pending_edit = edit;
                }
            }
        }
        Some(k) => {
            let d = &mut app.views[index].draft;
            if d.input.chars().count() < MAX_INPUT {
                d.input.push_str(k);
            }
        }
        None => {}
    }
}

/// The New sheet dialog: a blank page of a paper size with a drawing scale.
pub fn sheet_dialog(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if !app.draft.sheet.open {
        return;
    }
    let mut create = false;
    let mut close = false;
    let has_doc = app.active_ids().is_some();
    let modal = egui::Modal::new(egui::Id::new("draft-new-sheet")).show(ctx, |ui| {
        ui.set_width(420.0);
        let s = &mut app.draft.sheet;
        ui.label(egui::RichText::new(tl!("New sheet")).font(theme::semibold(17.0)));
        ui.add_space(8.0);
        egui::Grid::new("draft-sheet-grid").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
            ui.label(tl!("Paper"));
            egui::ComboBox::from_id_salt("draft-paper").selected_text(PAPERS.get(s.paper).map_or("", |p| p.name)).width(240.0).show_ui(ui, |ui| {
                for (i, p) in PAPERS.iter().enumerate() {
                    ui.selectable_value(&mut s.paper, i, p.name);
                }
            });
            ui.end_row();
            ui.label(tl!("Orientation"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.landscape, true, tl!("Landscape"));
                ui.selectable_value(&mut s.landscape, false, tl!("Portrait"));
            });
            ui.end_row();
            ui.label(tl!("Scale"));
            egui::ComboBox::from_id_salt("draft-scale")
                .selected_text(crate::measure_ui::PRESETS.get(s.scale).map_or("", |p| p.name))
                .width(240.0)
                .show_ui(ui, |ui| {
                    for (i, p) in crate::measure_ui::PRESETS.iter().enumerate() {
                        ui.selectable_value(&mut s.scale, i, p.name);
                    }
                });
            ui.end_row();
            if has_doc {
                ui.label(tl!("Add to"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.add_to_document, true, tl!("This document"));
                    ui.selectable_value(&mut s.add_to_document, false, tl!("A new document"));
                });
                ui.end_row();
            }
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::widgets::pill_button(ui, tl!("Create"), true).clicked() {
                create = true;
            }
            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                close = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
    }
    if create {
        new_sheet(app);
        close = true;
    }
    if close {
        app.draft.sheet.open = false;
    }
}

/// Make the sheet chosen in the New sheet dialog.
fn new_sheet(app: &mut PdfCraftApp) {
    let s = app.draft.sheet.clone();
    let (Some(paper), Some(preset)) = (PAPERS.get(s.paper), crate::measure_ui::PRESETS.get(s.scale)) else { return };
    let (w, h) = if s.landscape { (paper.height, paper.width) } else { (paper.width, paper.height) };
    let (w, h) = (w * PT_PER_IN, h * PT_PER_IN);
    let scale = match crate::measure_ui::preset_scale(preset) {
        Ok(scale) => scale,
        Err(e) => {
            app.notify(e);
            return;
        }
    };
    let set_scale = |page: usize| Edit::SetMeasurementScale { page, bbox: [0.0, 0.0, w, h], name: preset.name.to_string(), scale: scale.clone() };
    let existing = app.active_ids().filter(|_| s.add_to_document).and_then(|(i, id)| app.session.get(id).map(|d| (i, d.info.pages.len())));
    match existing {
        Some((index, pages)) => {
            let edit =
                Edit::Batch { label: "New sheet".into(), edits: vec![Edit::InsertBlankPage { at: pages, width: w, height: h }, set_scale(pages)] };
            if app.apply_edit(edit) {
                app.views[index].go_to_page(pages);
            }
        }
        None => {
            let created = app.session.create_blank(w, h, 1).map_err(|e| e.to_string());
            match app.open_created_bytes("Drawing.pdf", created) {
                Ok(()) => {
                    app.apply_edit(set_scale(0));
                }
                Err(e) => app.notify(e),
            }
        }
    }
    app.workflow = crate::ribbon::Workflow::Draft;
    app.quick_tool = QuickTool::Draft(DraftTool::Wall);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_parse_as_cad_users_type_them() {
        let ft = BareUnit::Feet;
        let cases: [(&str, f64); 14] = [
            ("15'", 180.0),
            ("15'6\"", 186.0),
            ("15'-6 1/2\"", 186.5),
            ("15' 6", 186.0),
            ("6\"", 6.0),
            ("6 1/2\"", 6.5),
            ("1/2\"", 0.5),
            ("15", 180.0),
            ("2.5", 30.0),
            ("-3'", -36.0),
            ("1m", 1000.0 / 25.4),
            ("300mm", 300.0 / 25.4),
            ("30cm", 300.0 / 25.4),
            ("'6\"", 6.0),
        ];
        for (s, inches) in cases {
            let v = parse_length(s, ft).unwrap_or_else(|| panic!("{s} didn't parse"));
            assert!((v - inches).abs() < 1e-9, "{s}: {v} ≠ {inches}");
        }
        assert_eq!(parse_length("15", BareUnit::Inches), Some(15.0));
        for bad in ["", "abc", "1/0\"", "'", "12'x", "1e400"] {
            assert_eq!(parse_length(bad, ft), None, "{bad}");
        }
    }

    #[test]
    fn coordinates_parse_relative_absolute_and_polar() {
        let ft = BareUnit::Feet;
        assert_eq!(parse_entry("@0,15", ft), Some(Entry::Relative([0.0, 180.0])));
        assert_eq!(parse_entry("@15<90", ft), Some(Entry::RelativePolar(180.0, 90.0)));
        assert_eq!(parse_entry("10',5'", ft), Some(Entry::Absolute([120.0, 60.0])));
        assert_eq!(parse_entry("12'6\"", ft), Some(Entry::Distance(150.0)));
        assert_eq!(parse_entry("@ x,1", ft), None);
        // 1/4" = 1'-0": 18 points per real foot, so 1.5 points per inch.
        let k = 1.5;
        let p = entry_point(Entry::Relative([0.0, 180.0]), Some([100.0, 100.0]), [0.0, 0.0], None, k, false).unwrap();
        assert!((p[0] - 100.0).abs() < 1e-9 && (p[1] - 370.0).abs() < 1e-9, "{p:?}");
        let p = entry_point(Entry::RelativePolar(120.0, 90.0), Some([0.0, 0.0]), [0.0, 0.0], None, k, false).unwrap();
        assert!(p[0].abs() < 1e-9 && (p[1] - 180.0).abs() < 1e-9, "{p:?}");
        // A length alone goes toward the pointer, kept square with ortho.
        let p = entry_point(Entry::Distance(120.0), Some([0.0, 0.0]), [0.0, 0.0], Some([10.0, 3.0]), k, true).unwrap();
        assert!((p[0] - 180.0).abs() < 1e-9 && p[1].abs() < 1e-9, "{p:?}");
        assert_eq!(entry_point(Entry::Distance(1.0), None, [0.0, 0.0], None, k, false), None);
    }

    #[test]
    fn walls_are_mitred_quads_of_the_thickness() {
        let quads = wall_quads(&[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], 10.0, false);
        assert_eq!(quads.len(), 2);
        // The first segment runs along y = 0, 5 either side; the corner is mitred to (105, -5).
        let q = quads[0];
        assert!((q[0][1] - 5.0).abs() < 1e-9 && (q[3][1] + 5.0).abs() < 1e-9, "{q:?}");
        assert!((q[2][0] - 105.0).abs() < 1e-9 && (q[2][1] + 5.0).abs() < 1e-9, "{q:?}");
        let closed = wall_quads(&[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0], [0.0, 0.0]], 10.0, true);
        assert_eq!(closed.len(), 4);
        assert!(wall_quads(&[[0.0, 0.0]], 10.0, false).is_empty());
        assert!(wall_quads(&[[0.0, 0.0], [1.0, 1.0]], f64::NAN, false).is_empty());
    }
}
