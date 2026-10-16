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
pub(crate) const SNAP_PX: f64 = 10.0;
/// Typed entries are short; anything longer is cut.
const MAX_INPUT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DraftTool {
    Line,
    Polyline,
    Rectangle,
    Wall,
    /// Centre, then a point on the circle (or a typed radius).
    Circle,
    /// Start, a point on the arc, end.
    Arc,
    /// An aligned dimension: a distance measurement at the page's scale.
    Dimension,
    /// A smooth curve through the clicked points (as connected lines).
    Spline,
    /// Centre, then a corner of the box it fits in.
    Ellipse,
    /// A regular polygon: centre, then a corner.
    Polygon,
    /// A revision cloud around the clicked points.
    Cloud,
    /// A horizontal or vertical dimension.
    DimLinear,
    /// Click a circle: its radius.
    DimRadius,
    /// Click a circle: its diameter.
    DimDiameter,
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
}

impl DraftTool {
    pub const ALL: [DraftTool; 30] = [
        DraftTool::Line,
        DraftTool::Polyline,
        DraftTool::Rectangle,
        DraftTool::Wall,
        DraftTool::Circle,
        DraftTool::Arc,
        DraftTool::Dimension,
        DraftTool::Spline,
        DraftTool::Ellipse,
        DraftTool::Polygon,
        DraftTool::Cloud,
        DraftTool::DimLinear,
        DraftTool::DimRadius,
        DraftTool::DimDiameter,
        DraftTool::Move,
        DraftTool::Copy,
        DraftTool::Rotate,
        DraftTool::Mirror,
        DraftTool::Scale,
        DraftTool::ArrayRect,
        DraftTool::ArrayPolar,
        DraftTool::Offset,
        DraftTool::Trim,
        DraftTool::Extend,
        DraftTool::Fillet,
        DraftTool::Chamfer,
        DraftTool::Break,
        DraftTool::Join,
        DraftTool::Explode,
        DraftTool::Erase,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Polyline => "polyline",
            Self::Rectangle => "rectangle",
            Self::Wall => "wall",
            Self::Circle => "circle",
            Self::Arc => "arc",
            Self::Dimension => "dimension",
            Self::Spline => "spline",
            Self::Ellipse => "ellipse",
            Self::Polygon => "polygon",
            Self::Cloud => "cloud",
            Self::DimLinear => "dim-linear",
            Self::DimRadius => "dim-radius",
            Self::DimDiameter => "dim-diameter",
            Self::Move => "move",
            Self::Copy => "copy",
            Self::Rotate => "rotate",
            Self::Mirror => "mirror",
            Self::Scale => "scale",
            Self::ArrayRect => "array",
            Self::ArrayPolar => "polar-array",
            Self::Offset => "offset",
            Self::Trim => "trim",
            Self::Extend => "extend",
            Self::Fillet => "fillet",
            Self::Chamfer => "chamfer",
            Self::Break => "break",
            Self::Join => "join",
            Self::Explode => "explode",
            Self::Erase => "erase",
        }
    }

    /// Picks markup that is already drawn (and changes or measures it) rather than drawing by
    /// points.
    pub fn modifies(self) -> bool {
        !matches!(
            self,
            Self::Line
                | Self::Polyline
                | Self::Rectangle
                | Self::Wall
                | Self::Circle
                | Self::Arc
                | Self::Dimension
                | Self::Spline
                | Self::Ellipse
                | Self::Polygon
                | Self::Cloud
                | Self::DimLinear
        )
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == s)
    }

    /// The English name with how to use it, translated where it is shown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Polyline => "Polyline",
            Self::Rectangle => "Rectangle",
            Self::Wall => "Wall",
            Self::Circle => "Circle (centre, then radius)",
            Self::Arc => "Arc (start, a point on it, end)",
            Self::Dimension => "Aligned dimension",
            Self::Spline => "Spline (a smooth curve through the points)",
            Self::Ellipse => "Ellipse (centre, then a corner)",
            Self::Polygon => "Polygon (centre, then a corner; sides in the ribbon)",
            Self::Cloud => "Revision cloud (click around the change)",
            Self::DimLinear => "Linear dimension (horizontal or vertical)",
            Self::DimRadius => "Radius dimension (click a circle)",
            Self::DimDiameter => "Diameter dimension (click a circle)",
            Self::Move => "Move (pick, base point, then where to)",
            Self::Copy => "Copy (pick, base point, then each copy)",
            Self::Rotate => "Rotate (pick, centre, then the angle)",
            Self::Mirror => "Mirror (pick, then two points on the mirror line)",
            Self::Scale => "Scale (pick, base point, then type the factor)",
            Self::ArrayRect => "Rectangular array (rows and columns in the ribbon)",
            Self::ArrayPolar => "Polar array (pick, then the centre)",
            Self::Offset => "Offset (a parallel copy at the offset distance)",
            Self::Trim => "Trim (click the part to cut away at crossing lines)",
            Self::Extend => "Extend (click near an end to reach the next line)",
            Self::Fillet => "Fillet (two lines: a clean or rounded corner)",
            Self::Chamfer => "Chamfer (two lines: a cut corner)",
            Self::Break => "Break (click a line where it splits)",
            Self::Join => "Join (two lines that meet become one)",
            Self::Explode => "Explode (a polyline or shape into separate lines)",
            Self::Erase => "Erase (click the markup to delete)",
        }
    }

    /// The drafting icon.
    pub fn icon(self) -> crate::cad_icons::CadIcon {
        use crate::cad_icons::CadIcon as I;
        match self {
            Self::Line => I::Line,
            Self::Polyline => I::Polyline,
            Self::Rectangle => I::Rectangle,
            Self::Wall => I::Wall,
            Self::Circle => I::Circle,
            Self::Arc => I::Arc,
            Self::Dimension => I::DimAligned,
            Self::Spline => I::Spline,
            Self::Ellipse => I::Ellipse,
            Self::Polygon => I::Polygon,
            Self::Cloud => I::RevCloud,
            Self::DimLinear => I::DimLinear,
            Self::DimRadius => I::DimRadius,
            Self::DimDiameter => I::DimDiameter,
            Self::Move => I::Move,
            Self::Copy => I::Copy,
            Self::Rotate => I::Rotate,
            Self::Mirror => I::Mirror,
            Self::Scale => I::Scale,
            Self::ArrayRect => I::ArrayRect,
            Self::ArrayPolar => I::ArrayPolar,
            Self::Offset => I::Offset,
            Self::Trim => I::Trim,
            Self::Extend => I::Extend,
            Self::Fillet => I::Fillet,
            Self::Chamfer => I::Chamfer,
            Self::Break => I::Break,
            Self::Join => I::Join,
            Self::Explode => I::Explode,
            Self::Erase => I::Erase,
        }
    }

    /// How many points finish the shape (open-ended shapes finish when told).
    fn points_needed(self) -> Option<usize> {
        match self {
            Self::Line | Self::Rectangle | Self::Circle | Self::Ellipse | Self::Polygon => Some(2),
            Self::Arc | Self::Dimension | Self::DimLinear => Some(3),
            _ => None,
        }
    }

    /// Placed point by point until finished (rather than ending at the second point).
    fn open_ended(self) -> bool {
        matches!(self, Self::Polyline | Self::Wall | Self::Spline | Self::Cloud)
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
    /// Who new markup is by (filled in from the comment author before drafting).
    pub author: String,
    /// Offset distance, inches.
    pub offset_inches: f64,
    /// Sides of a new polygon.
    pub polygon_sides: u32,
    /// Fillet radius, inches (0: a clean corner).
    pub fillet_inches: f64,
    /// Chamfer distance from the corner, inches.
    pub chamfer_inches: f64,
    /// Rectangular array: rows, columns and their spacing (inches).
    pub array_rows: u32,
    pub array_columns: u32,
    pub array_row_inches: f64,
    pub array_column_inches: f64,
    /// Polar array: how many in all, and the angle they fill (degrees).
    pub polar_count: u32,
    pub polar_angle: f64,
    /// Mirror keeps the original (a mirrored copy) rather than flipping it.
    pub mirror_keep: bool,
    /// Dimensions end in architectural ticks rather than arrows.
    pub dim_ticks: bool,
    /// The Draft tools panel shows only icons.
    pub panel_collapsed: bool,
    /// The tool each fly-out group shows, by group.
    pub group_tool: Vec<DraftTool>,
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
            author: String::new(),
            offset_inches: 12.0,
            polygon_sides: 6,
            fillet_inches: 0.0,
            chamfer_inches: 6.0,
            array_rows: 2,
            array_columns: 3,
            array_row_inches: 48.0,
            array_column_inches: 48.0,
            polar_count: 6,
            polar_angle: 360.0,
            mirror_keep: true,
            dim_ticks: false,
            panel_collapsed: false,
            group_tool: Vec::new(),
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
    /// Move, Copy and Rotate: the base point (on `page`).
    pub base: Option<[f64; 2]>,
    /// A grip being dragged with the Select tool.
    pub grip: Option<crate::draft_edit::Grip>,
    /// The markup's lines and shapes per (page, edit generation).
    pub(crate) geometry: Option<(usize, u64, Vec<pdfcraft_engine::geometry::Geometry>)>,
    /// A distance or factor typed with a modify tool (inches, or the scale factor), for the shared
    /// settings: Offset, Fillet and Chamfer keep theirs.
    pub typed_setting: Option<(DraftTool, f64)>,
}

impl DraftView {
    pub fn cancel(&mut self) {
        self.points.clear();
        self.page = None;
        self.input.clear();
        self.base = None;
        self.grip = None;
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
            let rest = rest.trim().trim_start_matches('-').trim().trim_end_matches('"').trim();
            if feet.trim().is_empty() && rest.is_empty() {
                return None;
            }
            let feet = if feet.trim().is_empty() { 0.0 } else { parse_number(feet)? };
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
pub(crate) fn units_per_inch(doc: &Document, page: usize, at: [f64; 2]) -> f64 {
    doc.real_size_points(page, at, 1.0).unwrap_or(PT_PER_IN)
}

/// Keep `to` horizontal or vertical from `from`.
pub(crate) fn orthogonal(from: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    if (to[0] - from[0]).abs() >= (to[1] - from[1]).abs() { [to[0], from[1]] } else { [from[0], to[1]] }
}

/// Where a typed entry puts the next point (user space).
pub(crate) fn entry_point(entry: Entry, last: Option<[f64; 2]>, origin: [f64; 2], toward: Option<[f64; 2]>, k: f64, ortho: bool) -> Option<[f64; 2]> {
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

pub(crate) fn layer_color(doc: &Document) -> [f64; 3] {
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
    let offsets = crate::draft_edit::mitre_offsets(&pts, thickness / 2.0, closed);
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

/// Extension lines run this far past the dimension line, and start this far from the measured
/// points (paper points).
const DIM_EXTENSION: f64 = 4.0;
const DIM_GAP: f64 = 2.0;

/// Where a dimension goes: the measured line (`L`), how far off its dimension line sits (`/LL`,
/// clockwise positive), and the measured points when they are not the line's ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimLayout {
    pub from: [f64; 2],
    pub to: [f64; 2],
    pub leader: f64,
    pub points: Option<[[f64; 2]; 2]>,
}

impl DimLayout {
    /// The dimension line's ends.
    pub fn line(&self) -> ([f64; 2], [f64; 2]) {
        let (a, b) = (self.from, self.to);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
        let n = [(b[1] - a[1]) / len, -(b[0] - a[0]) / len];
        ([a[0] + n[0] * self.leader, a[1] + n[1] * self.leader], [b[0] + n[0] * self.leader, b[1] + n[1] * self.leader])
    }

    /// The measured points the extension lines start from.
    pub fn origins(&self) -> [[f64; 2]; 2] {
        self.points.unwrap_or([self.from, self.to])
    }
}

/// The layout of an aligned (`linear` false) or horizontal/vertical dimension between `a` and `b`
/// whose dimension line passes through `c`. A linear dimension is horizontal when `c` is above or
/// below the points, vertical when it is beside them, as in CAD programs.
pub fn dim_layout(a: [f64; 2], b: [f64; 2], c: [f64; 2], linear: bool) -> Option<DimLayout> {
    let (from, to, points) = if linear {
        let out = |v: f64, lo: f64, hi: f64| (lo - v).max(v - hi).max(0.0);
        let dx = out(c[0], a[0].min(b[0]), a[0].max(b[0]));
        let dy = out(c[1], a[1].min(b[1]), a[1].max(b[1]));
        let horizontal = if dx == 0.0 && dy == 0.0 { (b[0] - a[0]).abs() >= (b[1] - a[1]).abs() } else { dy >= dx };
        let to = if horizontal { [b[0], a[1]] } else { [a[0], b[1]] };
        (a, to, Some([a, b]))
    } else {
        (a, b, None)
    };
    let len = (to[0] - from[0]).hypot(to[1] - from[1]);
    if len < 0.5 {
        return None;
    }
    let n = [(to[1] - from[1]) / len, -(to[0] - from[0]) / len];
    let leader = (c[0] - from[0]) * n[0] + (c[1] - from[1]) * n[1];
    leader.is_finite().then_some(DimLayout { from, to, leader, points })
}

/// A distance measurement from `a` to `b` at the page's scale, on the current layer's colour,
/// drawn as a dimension when `layout` says where its dimension line goes.
pub(crate) fn dimension(page: usize, a: [f64; 2], b: [f64; 2], doc: &Document, color: [f64; 3], author: &str) -> Option<Edit> {
    dimension_with(page, a, b, None, doc, color, author, DIM_ARROWS)
}

const DIM_ARROWS: LineEnding = LineEnding::ClosedArrow;

#[allow(clippy::too_many_arguments)]
fn dimension_with(
    page: usize,
    a: [f64; 2],
    b: [f64; 2],
    layout: Option<&DimLayout>,
    doc: &Document,
    color: [f64; 3],
    author: &str,
    ends: LineEnding,
) -> Option<Edit> {
    let scale = doc.measurement_scale(page, a).ok()?;
    let dimension = layout.filter(|l| l.leader.abs() > 0.25).map(|l| measure::Dimension {
        leader: l.leader,
        extension: DIM_EXTENSION,
        gap: DIM_GAP,
        ends,
        from: l.points,
    });
    Some(Edit::AddMeasurement(measure::NewMeasurement {
        page,
        kind: measure::Kind::Distance,
        points: vec![a, b],
        scale,
        style: Style { color, opacity: 1.0, width: 1.0, fill: None },
        label: String::new(),
        author: author.to_string(),
        dimension,
    }))
}

/// The markup for a finished shape (none when it has too few points).
fn finish_edit(tool: DraftTool, page: usize, points: &[[f64; 2]], closed: bool, doc: &Document, settings: &DraftSettings) -> Option<Edit> {
    let color = layer_color(doc);
    let author = settings.author.as_str();
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
        DraftTool::Circle => match points {
            [c, p, ..] => {
                let r = (p[0] - c[0]).hypot(p[1] - c[1]);
                (r >= 0.5).then(|| annotation(page, Shape::Oval { rect: [c[0] - r, c[1] - r, c[0] + r, c[1] + r] }, color, 1.0, None, author))
            }
            _ => None,
        },
        DraftTool::Arc => match points {
            [a, m, b, ..] => {
                let vertices = crate::draft_edit::arc_points(*a, *m, *b);
                (vertices.len() >= 2)
                    .then(|| annotation(page, Shape::PolyLine { vertices, start: LineEnding::None, end: LineEnding::None }, color, 1.0, None, author))
            }
            _ => None,
        },
        DraftTool::Dimension | DraftTool::DimLinear => match points {
            // The third point places the dimension line.
            [a, b, c, ..] => {
                let l = dim_layout(*a, *b, *c, tool == DraftTool::DimLinear)?;
                let ends = if settings.dim_ticks { LineEnding::Slash } else { DIM_ARROWS };
                dimension_with(page, l.from, l.to, Some(&l), doc, color, author, ends)
            }
            _ => None,
        },
        DraftTool::Spline => {
            let vertices = crate::draft_edit::spline_points(points, closed);
            if closed && vertices.len() >= 3 {
                Some(annotation(page, Shape::Polygon { vertices, cloud: false }, color, 1.0, None, author))
            } else if vertices.len() >= 2 {
                Some(annotation(page, Shape::PolyLine { vertices, start: LineEnding::None, end: LineEnding::None }, color, 1.0, None, author))
            } else {
                None
            }
        }
        DraftTool::Ellipse => match points {
            [c, p, ..] => {
                let (rx, ry) = ((p[0] - c[0]).abs(), (p[1] - c[1]).abs());
                (rx >= 0.5 && ry >= 0.5)
                    .then(|| annotation(page, Shape::Oval { rect: [c[0] - rx, c[1] - ry, c[0] + rx, c[1] + ry] }, color, 1.0, None, author))
            }
            _ => None,
        },
        DraftTool::Polygon => match points {
            [c, p, ..] => {
                let vertices = crate::draft_edit::regular_polygon(*c, *p, settings.polygon_sides);
                (vertices.len() >= 3).then(|| annotation(page, Shape::Polygon { vertices, cloud: false }, color, 1.0, None, author))
            }
            _ => None,
        },
        DraftTool::Cloud => {
            (points.len() >= 3).then(|| annotation(page, Shape::Polygon { vertices: points.to_vec(), cloud: true }, color, 1.0, None, author))
        }
        _ => None,
    }
}

/// Add `p` to the shape being placed; returns the markup when that completes a shape.
fn add_point(view: &mut DocView, doc: &Document, page: usize, tool: DraftTool, p: [f64; 2], settings: &DraftSettings) -> Option<Edit> {
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
    match tool.points_needed() {
        Some(n) if d.points.len() >= n => {
            let edit = finish_edit(tool, page, &d.points, false, doc, settings);
            if tool == DraftTool::Line {
                // Lines chain: the next one starts where this one ended.
                d.points = vec![p];
            } else {
                d.points.clear();
                d.page = None;
            }
            edit
        }
        _ => None,
    }
}

/// Finish an open-ended shape (polyline, wall), closed or not.
fn finish(view: &mut DocView, doc: &Document, tool: DraftTool, closed: bool, settings: &DraftSettings) -> Option<Edit> {
    let d = &mut view.draft;
    let page = d.page?;
    let edit = if tool.open_ended() { finish_edit(tool, page, &d.points, closed, doc, settings) } else { None };
    d.points.clear();
    d.page = None;
    edit
}

// ---- input -------------------------------------------------------------------------------------

/// Apply the typed entry: the next point.
pub fn submit(view: &mut DocView, doc: &Document, tool: DraftTool, settings: &DraftSettings) -> Option<Edit> {
    let text = std::mem::take(&mut view.draft.input);
    if tool.modifies() {
        return if text.trim().is_empty() { None } else { crate::draft_edit::submit(view, doc, tool, settings, &text) };
    }
    if text.trim().is_empty() {
        // Enter with nothing typed finishes a polyline or wall.
        return finish(view, doc, tool, false, settings);
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
    // A dimension's third entry is how far off its dimension line goes, on the pointer's side.
    if matches!(tool, DraftTool::Dimension | DraftTool::DimLinear)
        && view.draft.page == Some(page)
        && let ([a, b], Entry::Distance(len)) = (&view.draft.points[..], entry)
    {
        let pointer = toward.unwrap_or([a[0], a[1] + 1.0]);
        let l = dim_layout(*a, *b, pointer, tool == DraftTool::DimLinear)?;
        let side = if l.leader < 0.0 { -1.0 } else { 1.0 };
        let placed = DimLayout { leader: side * len * k, ..l };
        let (q1, _) = placed.line();
        return add_point(view, doc, page, tool, q1, settings);
    }
    let Some(p) = entry_point(entry, last, origin, toward, k, settings.ortho) else {
        view.draft.error = Some(tl!("Type a length after placing the first point, or give coordinates.").to_string());
        return None;
    };
    add_point(view, doc, page, tool, p, settings)
}

/// Keys while drafting (when no text field has focus): typing goes to the entry, Enter applies
/// it, Backspace edits it or removes the last point, Escape keeps what is drawn.
pub fn keys(ctx: &egui::Context, doc: &Document, view: &mut DocView, tool: DraftTool, settings: &DraftSettings) -> Option<Edit> {
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
        out = submit(view, doc, tool, settings);
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) && view.draft.input.pop().is_none() {
        view.draft.points.pop();
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        if !view.draft.input.is_empty() {
            view.draft.input.clear();
        } else if tool.modifies() {
            crate::draft_edit::step_back(view);
        } else {
            out = finish(view, doc, tool, false, settings);
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
        if tool.modifies() {
            if resp.clicked() && view.draft.setting_origin {
                view.draft.origin = Some((page, point));
                view.draft.setting_origin = false;
            } else {
                crate::draft_edit::modify_input(ui, resp, doc, view, cx, tool, settings, pos, point, tolerance);
            }
            return;
        }
        if resp.clicked() {
            if view.draft.setting_origin {
                view.draft.origin = Some((page, point));
                view.draft.setting_origin = false;
            } else {
                let closes = tool.open_ended()
                    && view.draft.page == Some(page)
                    && view.draft.points.len() >= 3
                    && view.draft.points.first().is_some_and(|f| cx.to_screen(*f).distance(pos) <= crate::point_bar::CLOSE_RADIUS);
                let edit = if closes { finish(view, doc, tool, true, settings) } else { add_point(view, doc, page, tool, point, settings) };
                if edit.is_some() {
                    view.pending_edit = edit;
                }
            }
        }
        if resp.double_clicked() && tool.open_ended() && view.draft.page == Some(page) {
            let edit = finish(view, doc, tool, false, settings);
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
        DraftTool::Circle => {
            if let ([c, p, ..], Some(sc)) = (&pts[..], screen.first()) {
                let r = (p[0] - c[0]).hypot(p[1] - c[1]);
                let edge = cx.to_screen([c[0] + r, c[1]]);
                painter.circle_stroke(*sc, sc.distance(edge), stroke);
                painter.add(egui::Shape::line(screen.clone(), Stroke::new(1.0, accent.gamma_multiply(0.6))));
            }
        }
        DraftTool::Arc => match &pts[..] {
            [a, m, b, ..] => {
                let arc: Vec<Pos2> = crate::draft_edit::arc_points(*a, *m, *b).iter().map(|p| cx.to_screen(*p)).collect();
                painter.add(egui::Shape::line(arc, stroke));
            }
            _ => {
                painter.add(egui::Shape::line(screen.clone(), Stroke::new(1.0, accent.gamma_multiply(0.6))));
            }
        },
        DraftTool::Ellipse | DraftTool::Polygon => {
            if let [c, p, ..] = &pts[..] {
                let outline = if tool == DraftTool::Polygon {
                    crate::draft_edit::regular_polygon(*c, *p, settings.polygon_sides)
                } else {
                    crate::draft_edit::ellipse_points(*c, *p)
                };
                painter.add(egui::Shape::closed_line(outline.iter().map(|q| cx.to_screen(*q)).collect(), stroke));
            }
        }
        DraftTool::Spline => {
            let curve: Vec<Pos2> = crate::draft_edit::spline_points(&pts, false).iter().map(|p| cx.to_screen(*p)).collect();
            painter.add(egui::Shape::line(curve, stroke));
            painter.extend(egui::Shape::dashed_line(&screen, Stroke::new(1.0, accent.gamma_multiply(0.5)), 4.0, 4.0));
        }
        DraftTool::Cloud => {
            painter.add(egui::Shape::closed_line(screen.clone(), stroke));
        }
        DraftTool::Dimension | DraftTool::DimLinear => match &pts[..] {
            [a, b, c, ..] => {
                if let Some(l) = dim_layout(*a, *b, *c, tool == DraftTool::DimLinear) {
                    let (q1, q2) = l.line();
                    for (o, q) in l.origins().iter().zip([q1, q2]) {
                        painter.line_segment([cx.to_screen(*o), cx.to_screen(q)], Stroke::new(1.0, accent.gamma_multiply(0.8)));
                    }
                    painter.line_segment([cx.to_screen(q1), cx.to_screen(q2)], stroke);
                    if let Ok(scale) = doc.measurement_scale(page, l.from) {
                        let len = (l.to[0] - l.from[0]).hypot(l.to[1] - l.from[1]) * scale.x * scale.distance_factor;
                        if let Ok(text) = scale.format_length(len) {
                            let mid = cx.to_screen([(q1[0] + q2[0]) / 2.0, (q1[1] + q2[1]) / 2.0]);
                            crate::draft_edit::chip(painter, mid + vec2(6.0, -22.0), &text);
                        }
                    }
                }
            }
            _ => {
                painter.add(egui::Shape::line(screen.clone(), stroke));
            }
        },
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
            Some(crate::point_bar::PointAction::Finish) => view.pending_edit = finish(view, doc, tool, false, settings),
            Some(crate::point_bar::PointAction::Close) => view.pending_edit = finish(view, doc, tool, true, settings),
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
    egui::ScrollArea::horizontal().id_salt("draft-ribbon").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                group(ui, &t, tl!("Sheet"), |ui| {
                    if icons::button(ui, "file-plus-2", 34.0, false, tl!("New sheet at a drawing scale")).clicked() {
                        app.draft.sheet.open = true;
                        app.draft.sheet.add_to_document = active.is_some();
                    }
                });
                separator(ui, &t);
                ui.add_enabled_ui(allowed, |ui| tool_options(app, ui, &t));
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
                            let mut settings = app.draft.clone();
                            settings.author = app.comment_prefs.author.clone();
                            let edit = submit(&mut app.views[index], doc, tool, &settings);
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
        },
    );
    if let Some((index, _)) = active
        && let Some(e) = app.views[index].draft.error.clone()
    {
        app.notify(e);
        app.views[index].draft.error = None;
    }
}

/// A labelled number in the ribbon.
fn number<N: egui::emath::Numeric>(ui: &mut egui::Ui, label: &str, value: &mut N, range: std::ops::RangeInclusive<N>, suffix: &str, tip: &str) {
    ui.label(egui::RichText::new(label).font(theme::regular(12.0)));
    ui.add(egui::DragValue::new(value).range(range).speed(0.25).suffix(suffix)).on_hover_text(tip);
}

/// The ribbon's options for the tool in use: wall thickness, offset distance, polygon sides,
/// fillet radius, array rows and columns…
fn tool_options(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let tool = match app.quick_tool {
        QuickTool::Draft(tool) => Some(tool),
        _ => None,
    };
    let d = &mut app.draft;
    group(ui, t, tl!("Tool options"), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            match tool {
                Some(DraftTool::Wall) => number(ui, tl!("Thickness"), &mut d.wall_inches, 0.5..=48.0, " in", tl!("Wall thickness")),
                Some(DraftTool::Offset) => number(
                    ui,
                    tl!("Distance"),
                    &mut d.offset_inches,
                    0.125..=1200.0,
                    " in",
                    tl!("Offset distance (or type it with the Offset tool, then Enter)"),
                ),
                Some(DraftTool::Polygon) => number(ui, tl!("Sides"), &mut d.polygon_sides, 3..=64, "", tl!("How many sides a new polygon has")),
                Some(DraftTool::Fillet) => {
                    number(ui, tl!("Radius"), &mut d.fillet_inches, 0.0..=1200.0, " in", tl!("Fillet radius (0 makes a sharp corner)"))
                }
                Some(DraftTool::Chamfer) => {
                    number(ui, tl!("Distance"), &mut d.chamfer_inches, 0.125..=1200.0, " in", tl!("How far back from the corner each side is cut"))
                }
                Some(DraftTool::ArrayRect) => {
                    number(ui, tl!("Rows"), &mut d.array_rows, 1..=50, "", tl!("Rows in the array"));
                    number(ui, tl!("Columns"), &mut d.array_columns, 1..=50, "", tl!("Columns in the array"));
                    number(
                        ui,
                        tl!("Row spacing"),
                        &mut d.array_row_inches,
                        -2400.0..=2400.0,
                        " in",
                        tl!("Distance between rows (negative goes down)"),
                    );
                    number(
                        ui,
                        tl!("Column spacing"),
                        &mut d.array_column_inches,
                        -2400.0..=2400.0,
                        " in",
                        tl!("Distance between columns (negative goes left)"),
                    );
                }
                Some(DraftTool::ArrayPolar) => {
                    number(ui, tl!("Count"), &mut d.polar_count, 2..=360, "", tl!("How many in all, the original included"));
                    number(ui, tl!("Angle"), &mut d.polar_angle, -360.0..=360.0, "°", tl!("The angle they fill (360 goes all the way round)"));
                }
                Some(DraftTool::Mirror) => {
                    ui.checkbox(&mut d.mirror_keep, tl!("Keep the original"));
                }
                Some(DraftTool::Dimension | DraftTool::DimLinear) => {
                    ui.radio_value(&mut d.dim_ticks, false, tl!("Arrows"));
                    ui.radio_value(&mut d.dim_ticks, true, tl!("Ticks"));
                    ui.label(
                        egui::RichText::new(tl!("Click both ends, then where the dimension line goes"))
                            .font(theme::regular(12.0))
                            .color(t.text_faint),
                    );
                }
                _ => {
                    ui.label(
                        egui::RichText::new(tl!("Pick a tool in the Draft tools panel on the left")).font(theme::regular(12.0)).color(t.text_faint),
                    );
                }
            }
        });
    });
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
            let mut settings = app.draft.clone();
            settings.author = app.comment_prefs.author.clone();
            if let Some(doc) = app.session.get(id) {
                let edit = submit(&mut app.views[index], doc, tool, &settings);
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
