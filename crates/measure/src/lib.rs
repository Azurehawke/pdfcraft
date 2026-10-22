//! Calibrated 2D measurements in PDF user space (ISO 32000-2 §12.9, measurement properties).
//! Viewports and annotations carry standard rectilinear /Measure dictionaries; the
//! annotations use the measurement intents of line, polygon and polyline annotations (§12.5.6).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod names;
pub mod snap;
use pdfcraft_annot::{Meta, NewAnnotation, Shape, Style};
use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString};
use serde::Serialize;

pub type Point = [f64; 2];
pub const MAX_POINTS: usize = 2048;
const MAX_COORD: f64 = 1e9;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MeasureError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Annotation(#[from] pdfcraft_annot::AnnotError),
}
type Result<T> = std::result::Result<T, MeasureError>;
fn invalid(s: &str) -> MeasureError {
    MeasureError::Invalid(s.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Distance,
    Perimeter,
    Area,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Distance => "distance",
            Self::Perimeter => "perimeter",
            Self::Area => "area",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "distance" => Some(Self::Distance),
            "perimeter" => Some(Self::Perimeter),
            "area" => Some(Self::Area),
            _ => None,
        }
    }
    fn intent(self) -> &'static str {
        match self {
            Self::Distance => "LineDimension",
            Self::Perimeter => "PolyLineDimension",
            Self::Area => "PolygonDimension",
        }
    }
}

/// How lengths are written. Areas are always decimal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "style", rename_all = "snake_case")]
pub enum LengthFormat {
    /// `12.50 ft`, with `Scale::precision` decimal places.
    #[default]
    Decimal,
    /// Architectural feet and inches with fractional inches, `12'-6 1/2"`. The scale's
    /// unit is feet; `denominator` (1 to 64, a power of two) is the finest fraction of
    /// an inch. Stored as a two-element compound number format (ISO 32000-2 §12.9.2):
    /// feet, then the remainder in inches as a fraction.
    FeetInches { denominator: u16 },
}
const FEET_INCH_DENOMINATORS: [u16; 7] = [1, 2, 4, 8, 16, 32, 64];
const FOOT_MARK: &str = "'";
const INCH_MARK: &str = "\"";

/// Conversion from PDF user units to real-world units. Y can use a different scale.
/// Distance and area conversion factors are relative to X (the rectilinear measure
/// dictionary, ISO 32000-2 §12.9).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Scale {
    pub x: f64,
    pub y: f64,
    pub unit: String,
    pub distance_factor: f64,
    pub area_factor: f64,
    pub area_unit: String,
    /// Decimal places of decimal lengths and of areas.
    pub precision: u8,
    pub ratio: String,
    pub format: LengthFormat,
}
impl Default for Scale {
    fn default() -> Self {
        Self {
            x: 1.0 / 72.0,
            y: 1.0 / 72.0,
            unit: "in".into(),
            distance_factor: 1.0,
            area_factor: 1.0,
            area_unit: "in^2".into(),
            precision: 2,
            ratio: "1 in = 1 in".into(),
            format: LengthFormat::Decimal,
        }
    }
}
/// `12'-6 1/2"`: feet and inches, rounded to the nearest `1/denominator` inch.
/// Fractions are reduced, so 8/16 is written 1/2.
pub fn format_feet_inches(feet: f64, denominator: u16) -> Result<String> {
    if !FEET_INCH_DENOMINATORS.contains(&denominator) {
        return Err(invalid("inch fractions must be 1, 1/2, 1/4, 1/8, 1/16, 1/32 or 1/64"));
    }
    let d = i64::from(denominator);
    let per_foot = 12 * d;
    let ticks = (feet.abs() * per_foot as f64).round();
    // 2^53 keeps every tick exact; real drawings are many orders of magnitude smaller.
    if !ticks.is_finite() || ticks > 9.0e15 {
        return Err(invalid("measurement is too large to write in feet and inches"));
    }
    let ticks = ticks as i64;
    let sign = if feet < 0.0 && ticks > 0 { "-" } else { "" };
    let (whole_feet, rest) = (ticks / per_foot, ticks % per_foot);
    let (inches, mut num) = (rest / d, rest % d);
    let mut den = d;
    while num > 0 && num % 2 == 0 && den % 2 == 0 {
        num /= 2;
        den /= 2;
    }
    Ok(if num == 0 {
        format!("{sign}{whole_feet}{FOOT_MARK}-{inches}{INCH_MARK}")
    } else {
        format!("{sign}{whole_feet}{FOOT_MARK}-{inches} {num}/{den}{INCH_MARK}")
    })
}
impl Scale {
    pub fn new(units_per_user_unit: f64, unit: &str, precision: u8) -> Result<Self> {
        let out = Self {
            x: units_per_user_unit,
            y: units_per_user_unit,
            unit: unit.into(),
            distance_factor: 1.0,
            area_factor: 1.0,
            area_unit: format!("{unit}^2"),
            precision,
            ratio: format!("1 pt = {units_per_user_unit} {unit}"),
            format: LengthFormat::Decimal,
        };
        out.validate()?;
        Ok(out)
    }
    /// The same scale written in feet and fractional inches. A scale in inches is
    /// converted to feet; any other unit is refused.
    pub fn with_feet_inches(mut self, denominator: u16) -> Result<Self> {
        let to_feet = match self.unit.trim() {
            "ft" | "'" | "feet" | "foot" => 1.0,
            "in" | "\"" | "inch" | "inches" => 1.0 / 12.0,
            _ => return Err(invalid("feet-and-inches needs a scale in feet or inches")),
        };
        self.x *= to_feet;
        self.y *= to_feet;
        self.unit = "ft".into();
        self.area_unit = "sq ft".into();
        self.format = LengthFormat::FeetInches { denominator };
        self.validate()?;
        Ok(self)
    }
    /// A length (already converted to the scale's unit) as the scale writes it.
    pub fn format_length(&self, value: f64) -> Result<String> {
        match self.format {
            LengthFormat::Decimal => Ok(format!("{value:.precision$} {}", self.unit, precision = usize::from(self.precision))),
            LengthFormat::FeetInches { denominator } => format_feet_inches(value, denominator),
        }
    }
    pub fn calibrate(a: Point, b: Point, real_distance: f64, unit: &str, precision: u8) -> Result<Self> {
        check_points(&[a, b])?;
        if !real_distance.is_finite() || real_distance <= 0.0 {
            return Err(invalid("calibration distance must be finite and positive"));
        }
        let length = distance(a, b);
        if length < 1e-6 {
            return Err(invalid("calibration needs two distinct points"));
        }
        Self::new(real_distance / length, unit, precision)
    }
    pub fn validate(&self) -> Result<()> {
        for f in [self.x, self.y, self.distance_factor, self.area_factor] {
            if !f.is_finite() || !(1e-12..=1e12).contains(&f) {
                return Err(invalid("scale factors must be finite and between 1e-12 and 1e12"));
            }
        }
        if self.precision > 6 {
            return Err(invalid("precision must be between 0 and 6 decimal places"));
        }
        if let LengthFormat::FeetInches { denominator } = self.format {
            let fraction_ok = FEET_INCH_DENOMINATORS.contains(&denominator);
            match (fraction_ok, self.unit == "ft") {
                (false, _) => return Err(invalid("inch fractions must be 1, 1/2, 1/4, 1/8, 1/16, 1/32 or 1/64")),
                (true, false) => return Err(invalid("feet-and-inches scales must use feet")),
                (true, true) => {}
            }
        }
        // Units are text strings, so non-ASCII labels such as "m²" are allowed.
        for s in [&self.unit, &self.area_unit] {
            let n = s.chars().count();
            if s.trim().is_empty() || n > 24 || s.chars().any(char::is_control) {
                return Err(invalid("unit labels must be 1 to 24 printable characters"));
            }
        }
        if self.ratio.len() > 256 || self.ratio.chars().any(char::is_control) {
            return Err(invalid("invalid scale ratio"));
        }
        Ok(())
    }
    pub fn dictionary(&self) -> Result<Dict> {
        self.validate()?;
        let format = |unit: &str, factor: f64| {
            let mut d = Dict::new();
            d.set(b"Type".to_vec(), Object::name("NumberFormat"));
            d.set(b"U".to_vec(), PdfString::text(unit));
            d.set(b"C".to_vec(), Object::Real(factor));
            d.set(b"F".to_vec(), Object::name(if self.precision == 0 { "R" } else { "D" }));
            if self.precision > 0 {
                d.set(b"D".to_vec(), Object::Int(10_i64.pow(u32::from(self.precision))));
            }
            d.set(b"FD".to_vec(), Object::Bool(true));
            d.set(b"SS".to_vec(), PdfString::text(""));
            Object::Array(vec![Object::Dict(d)])
        };
        // Compound feet-and-inches (§12.9.2): the first format converts to feet and
        // shows the integer part; the second converts the remainder to inches (C 12)
        // and shows it as a fraction with denominator D.
        let length = |factor: f64| match self.format {
            LengthFormat::Decimal => format(&self.unit, factor),
            LengthFormat::FeetInches { denominator } => {
                let mut feet = Dict::new();
                feet.set(b"Type".to_vec(), Object::name("NumberFormat"));
                feet.set(b"U".to_vec(), PdfString::text(FOOT_MARK));
                feet.set(b"C".to_vec(), Object::Real(factor));
                feet.set(b"F".to_vec(), Object::name("D"));
                feet.set(b"D".to_vec(), Object::Int(1));
                // PS sits between number and label, SS after the label: 12'-
                feet.set(b"PS".to_vec(), PdfString::text(""));
                feet.set(b"SS".to_vec(), PdfString::text("-"));
                let mut inches = Dict::new();
                inches.set(b"Type".to_vec(), Object::name("NumberFormat"));
                inches.set(b"U".to_vec(), PdfString::text(INCH_MARK));
                inches.set(b"C".to_vec(), Object::Real(12.0));
                inches.set(b"F".to_vec(), Object::name("F"));
                inches.set(b"D".to_vec(), Object::Int(i64::from(denominator)));
                inches.set(b"FD".to_vec(), Object::Bool(false));
                inches.set(b"PS".to_vec(), PdfString::text(""));
                inches.set(b"SS".to_vec(), PdfString::text(""));
                Object::Array(vec![Object::Dict(feet), Object::Dict(inches)])
            }
        };
        let mut d = Dict::new();
        d.set(b"Type".to_vec(), Object::name("Measure"));
        d.set(b"Subtype".to_vec(), Object::name("RL"));
        d.set(b"R".to_vec(), PdfString::text(&self.ratio));
        d.set(b"X".to_vec(), length(self.x));
        if self.x != self.y {
            d.set(b"Y".to_vec(), length(self.y));
            d.set(b"CYX".to_vec(), Object::Real(1.0));
        }
        d.set(b"D".to_vec(), length(self.distance_factor));
        d.set(b"A".to_vec(), format(&self.area_unit, self.area_factor));
        Ok(d)
    }
    /// Single-unit decimal scales and feet-and-fractional-inches compound scales are
    /// editable. Other compound, fractional and non-rectilinear imports are refused
    /// explicitly; their dictionaries and appearance remain untouched.
    pub fn read(doc: &Document, object: &Object) -> Result<Self> {
        let o = doc.resolve(object);
        let d = o.as_dict().ok_or_else(|| invalid("invalid measure dictionary"))?;
        if d.name(b"Subtype").is_some_and(|n| n != b"RL") {
            return Err(invalid("only rectilinear measurement scales are supported"));
        }
        let element = |o: &Object| -> Result<Dict> { doc.resolve(o).as_dict().cloned().ok_or_else(|| invalid("invalid number format dictionary")) };
        let factor = |f: &Dict| f.get(b"C").and_then(Object::as_f64).ok_or_else(|| invalid("missing conversion factor"));
        let format = |key: &[u8]| -> Result<(String, f64, u8, LengthFormat)> {
            let a = doc.resolve(d.get(key).ok_or_else(|| invalid("measurement scale lacks a number format"))?);
            let a = a.as_array().ok_or_else(|| invalid("invalid measurement number format"))?;
            match a.as_slice() {
                [single] => {
                    let f = element(single)?;
                    let precision = match f.name(b"F") {
                        Some(b"R") => 0,
                        None | Some(b"D") => {
                            let denom = f.int(b"D").unwrap_or(100);
                            (0..=6).find(|p| 10_i64.pow(*p) == denom).ok_or_else(|| invalid("unsupported measurement precision"))?
                        }
                        _ => return Err(invalid("fractional measurement formats are not supported yet")),
                    };
                    Ok((text(&f, b"U"), factor(&f)?, precision as u8, LengthFormat::Decimal))
                }
                // Feet, then the remainder in inches (×12) as a binary fraction.
                [major, minor] => {
                    let (major, minor) = (element(major)?, element(minor)?);
                    let inches = factor(&minor)?;
                    let denominator = minor.int(b"D").and_then(|v| u16::try_from(v).ok()).unwrap_or(16);
                    if (inches - 12.0).abs() > 1e-9
                        || minor.name(b"F") != Some(&b"F"[..])
                        || !FEET_INCH_DENOMINATORS.contains(&denominator)
                        || matches!(major.name(b"F"), Some(f) if f != b"D" && f != b"R" && f != b"T")
                    {
                        return Err(invalid("only feet-and-fractional-inch compound units are supported"));
                    }
                    Ok(("ft".into(), factor(&major)?, 2, LengthFormat::FeetInches { denominator }))
                }
                _ => Err(invalid("compound measurement units with more than two parts are not supported yet")),
            }
        };
        let (unit, x, _, _) = format(b"X")?;
        let y = if d.contains(b"Y") {
            let (_, y, _, _) = format(b"Y")?;
            y * d.get(b"CYX").and_then(Object::as_f64).ok_or_else(|| invalid("Y scale requires CYX for lengths and areas"))?
        } else {
            x
        };
        let (distance_unit, distance_factor, decimals, length_format) = format(b"D")?;
        if unit != distance_unit {
            return Err(invalid("different coordinate and distance units are not supported yet"));
        }
        let (area_unit, area_factor, area_decimals, area_format) = format(b"A")?;
        if area_format != LengthFormat::Decimal {
            return Err(invalid("compound area units are not supported yet"));
        }
        // A feet-and-inches scale has no decimal lengths; its decimals are the area's.
        let precision = if length_format == LengthFormat::Decimal { decimals } else { area_decimals };
        let out = Self { x, y, unit, distance_factor, area_factor, area_unit, precision, ratio: text(d, b"R"), format: length_format };
        out.validate()?;
        Ok(out)
    }
}
pub(crate) fn check_points(points: &[Point]) -> Result<()> {
    if points.len() > MAX_POINTS || points.iter().flatten().any(|v| !v.is_finite() || v.abs() > MAX_COORD) {
        return Err(invalid("measurement geometry must be finite, within 1e9 user units, and have at most 2048 vertices"));
    }
    Ok(())
}
pub fn distance(a: Point, b: Point) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Reading {
    pub kind: Kind,
    pub value: f64,
    pub unit: String,
    pub label: String,
    pub delta_x: f64,
    pub delta_y: f64,
    pub angle: f64,
    pub length: f64,
    pub area: f64,
}
/// Live reading allows incomplete paths; adding a saved annotation validates its vertex count.
pub fn reading(kind: Kind, points: &[Point], scale: &Scale) -> Result<Reading> {
    check_points(points)?;
    scale.validate()?;
    let mut length = 0.0;
    for w in points.windows(2) {
        if let [a, b] = w {
            length += ((b[0] - a[0]) * scale.x).hypot((b[1] - a[1]) * scale.y);
        }
    }
    let area = if points.len() >= 3 {
        let origin = points.first().copied().unwrap_or_default();
        points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
            .map(|(a, b)| ((a[0] - origin[0]) * (b[1] - origin[1]) - (b[0] - origin[0]) * (a[1] - origin[1])) * 0.5)
            .sum::<f64>()
            .abs()
            * scale.x
            * scale.y
            * scale.area_factor
    } else {
        0.0
    };
    let delta = match (points.first(), points.last()) {
        (Some(a), Some(b)) => [(b[0] - a[0]) * scale.x, (b[1] - a[1]) * scale.y],
        _ => [0.0; 2],
    };
    length *= scale.distance_factor;
    let value = if kind == Kind::Area { area } else { length };
    let unit = if kind == Kind::Area { scale.area_unit.clone() } else { scale.unit.clone() };
    if !value.is_finite() {
        return Err(invalid("measurement overflows its scale"));
    }
    let label = if kind == Kind::Area {
        format!("{value:.precision$} {unit}", precision = usize::from(scale.precision))
    } else {
        scale.format_length(value)?
    };
    Ok(Reading { kind, value, unit, label, delta_x: delta[0], delta_y: delta[1], angle: delta[1].atan2(delta[0]).to_degrees(), length, area })
}
#[derive(Clone, Debug, PartialEq)]
pub struct NewMeasurement {
    pub page: usize,
    pub kind: Kind,
    pub points: Vec<Point>,
    pub scale: Scale,
    pub style: Style,
    pub label: String,
    pub author: String,
    /// A distance drawn as a drafting dimension (extension lines and an offset dimension line)
    /// rather than as a plain measuring line.
    pub dimension: Option<Dimension>,
}

/// How a distance is drawn as a dimension (ISO 32000-2 §12.5.6.7): the dimension line sits
/// `leader` points from the measured line (positive: clockwise of its direction), with extension
/// lines from the measured points (`from`, when they are not the line's own ends, as in a
/// horizontal or vertical dimension) that start `gap` from them and run `extension` past it.
#[derive(Clone, Debug, PartialEq)]
pub struct Dimension {
    pub leader: f64,
    pub extension: f64,
    pub gap: f64,
    pub ends: pdfcraft_annot::LineEnding,
    pub from: Option<[Point; 2]>,
}
/// An area's vertices without a repeated closing vertex (last == first), which some
/// writers include; the closing edge is implicit.
pub fn open_polygon(points: &[Point]) -> &[Point] {
    match points {
        [first, rest @ .., last] if rest.len() >= 2 && distance(*first, *last) < 1e-6 => points.get(..points.len() - 1).unwrap_or(points),
        _ => points,
    }
}
fn validate_geometry(kind: Kind, points: &[Point]) -> Result<()> {
    check_points(points)?;
    let points = if kind == Kind::Area { open_polygon(points) } else { points };
    match kind {
        Kind::Distance if points.len() != 2 => return Err(invalid("distance needs exactly two points")),
        Kind::Perimeter if points.len() < 2 => return Err(invalid("perimeter needs at least two points")),
        Kind::Area if points.len() < 3 => return Err(invalid("area needs at least three points")),
        _ => {}
    }
    if points.windows(2).any(|p| matches!(p,[a,b] if distance(*a,*b)<1e-6)) {
        return Err(invalid("adjacent measurement vertices must be distinct"));
    }
    if kind == Kind::Area {
        for (i, (a, b)) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()).enumerate() {
            for (j, (c, d)) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()).enumerate().skip(i + 2) {
                if i == 0 && j + 1 == points.len() {
                    continue;
                }
                if snap::intersection(*a, *b, *c, *d).is_some() {
                    return Err(invalid("area boundary must not intersect itself"));
                }
            }
        }
    }
    Ok(())
}
fn text(d: &Dict, k: &[u8]) -> String {
    d.get(k).and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default()
}
fn nums(v: impl IntoIterator<Item = f64>) -> Object {
    Object::Array(v.into_iter().map(Object::Real).collect())
}
fn page(doc: &Document, index: usize) -> Result<pdfcraft_model::Page> {
    pdfcraft_model::pages(doc).get(index).cloned().ok_or_else(|| invalid("measurement page does not exist"))
}
fn annot(doc: &Document, page_index: usize, index: usize) -> Result<(ObjRef, Dict)> {
    let p = page(doc, page_index)?;
    let list = doc.resolve(p.dict.get(b"Annots").ok_or_else(|| invalid("page has no annotations"))?);
    let o = list.as_array().and_then(|a| a.get(index)).ok_or_else(|| invalid("measurement annotation does not exist"))?;
    let r = o.as_ref().ok_or_else(|| invalid("inline measurement annotations cannot be edited yet"))?;
    let o = doc.get(r);
    Ok((r, o.as_dict().cloned().ok_or_else(|| invalid("invalid annotation dictionary"))?))
}
pub fn add(doc: &mut Document, new: &NewMeasurement, meta: &Meta) -> Result<usize> {
    validate_geometry(new.kind, &new.points)?;
    let info = reading(new.kind, &new.points, &new.scale)?;
    if info.value <= 0.0 {
        return Err(invalid("measurement must have a positive length or area"));
    }
    if new.label.len() > 512 || new.author.len() > 512 {
        return Err(invalid("measurement label and author must be at most 512 bytes"));
    }
    let shape = match new.kind {
        Kind::Distance => Shape::Line {
            from: new.points.first().copied().ok_or_else(|| invalid("missing start"))?,
            to: new.points.last().copied().ok_or_else(|| invalid("missing end"))?,
            start: pdfcraft_annot::LineEnding::None,
            end: pdfcraft_annot::LineEnding::OpenArrow,
        },
        Kind::Perimeter => {
            Shape::PolyLine { vertices: new.points.clone(), start: pdfcraft_annot::LineEnding::None, end: pdfcraft_annot::LineEnding::None }
        }
        Kind::Area => Shape::Polygon { vertices: new.points.clone(), cloud: false },
    };
    let index = pdfcraft_annot::add_annotation(
        doc,
        &NewAnnotation { page: new.page, shape, style: new.style.clone(), contents: info.label.clone(), author: new.author.clone() },
        meta,
    )?;
    // Named before it is added to the list of names in use.
    let name = names::new_name(doc, new.kind, &new.label);
    let (r, mut d) = annot(doc, new.page, index)?;
    d.set(b"IT".to_vec(), Object::name(new.kind.intent()));
    d.set(names::KEY.to_vec(), PdfString::text(&name));
    d.set(b"Measure".to_vec(), Object::Dict(new.scale.dictionary()?));
    d.set(b"Subj".to_vec(), PdfString::text(new.kind.name()));
    d.set(b"PCMeasureLabel".to_vec(), PdfString::text(&new.label));
    d.set(b"PCMeasureValue".to_vec(), PdfString::text(&info.label));
    if new.kind == Kind::Distance {
        d.set(b"LE".to_vec(), Object::Array(vec![Object::name("OpenArrow"), Object::name("OpenArrow")]));
        d.set(b"Cap".to_vec(), Object::Bool(true));
        d.set(b"CP".to_vec(), Object::name("Top"));
    }
    let dimension = new.dimension.as_ref().filter(|_| new.kind == Kind::Distance);
    if let Some(dim) = dimension {
        let values = [dim.leader, dim.extension, dim.gap];
        if values.iter().any(|v| !v.is_finite() || v.abs() > MAX_COORD) || dim.extension < 0.0 || dim.gap < 0.0 {
            return Err(invalid("invalid dimension layout"));
        }
        let ending = Object::name(dim.ends.name());
        d.set(b"LE".to_vec(), Object::Array(vec![ending.clone(), ending]));
        d.set(b"LL".to_vec(), Object::Real(dim.leader));
        d.set(b"LLE".to_vec(), Object::Real(dim.extension));
        d.set(b"LLO".to_vec(), Object::Real(dim.gap));
        d.set(b"CP".to_vec(), Object::name("Inline"));
        // The page's turn, so the text reads upright as the page is shown.
        let turn = page(doc, new.page)?.rotation(doc);
        if turn != 0 {
            d.set(b"PCRotate".to_vec(), Object::Int(turn));
        }
        if let Some(from) = dim.from {
            check_points(&from)?;
            d.set(b"PCDimPoints".to_vec(), nums(from.concat()));
        }
        // Room for the extension lines, the dimension line and its text.
        let (a, b) = (new.points.first().copied().unwrap_or_default(), new.points.last().copied().unwrap_or_default());
        let len = distance(a, b).max(1e-9);
        let n = [(b[1] - a[1]) / len, -(b[0] - a[0]) / len];
        let reach = dim.leader + dim.leader.signum() * dim.extension;
        let mut pts: Vec<Point> = vec![a, b, [a[0] + n[0] * reach, a[1] + n[1] * reach], [b[0] + n[0] * reach, b[1] + n[1] * reach]];
        if let Some(from) = dim.from {
            pts.extend(from);
        }
        let pad = (info.label.chars().count() as f64 * 3.2 + 6.0).max(16.0);
        let (x0, y0, x1, y1) =
            pts.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |(x0, y0, x1, y1), p| (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1])));
        d.set(b"Rect".to_vec(), nums([x0 - pad, y0 - pad, x1 + pad, y1 + pad]));
    } else if let Some(rect) = d.get(b"Rect").and_then(Object::as_array) {
        // Leave room for the readable numeric caption above the geometry.
        let values: Option<Vec<f64>> = rect.iter().map(Object::as_f64).collect();
        if let Some(v) = values.filter(|v| v.len() == 4) {
            let width = (info.label.len() as f64 * 6.0).max(40.0);
            let cx = (v[0] + v[2]) * 0.5;
            d.set(b"Rect".to_vec(), nums([v[0].min(cx - width / 2.0), v[1], v[2].max(cx + width / 2.0), v[3] + 16.0]));
        }
    }
    doc.update_dict(r, |target| *target = d).map_err(|e| invalid(&e.to_string()))?;
    pdfcraft_annot::set_appearance(doc, r)?;
    Ok(index)
}
/// Recompute the reading of the measurement at `(page_index, index)` after its points changed
/// (moved grips, a trim): the value and, when it still shows the old value, the comment text.
/// Returns whether the annotation is a measurement. Other annotations are left alone.
/// The largest cable allowance (in the scale's unit).
const MAX_EXTRA: f64 = 1e7;

/// How many of `unit` make an inch, for lengths typed in feet and inches.
pub fn per_inch(unit: &str) -> Option<f64> {
    Some(match unit.trim() {
        "in" | "\"" | "inch" | "inches" => 1.0,
        "ft" | "'" | "feet" | "foot" => 1.0 / 12.0,
        "yd" => 1.0 / 36.0,
        "mi" => 1.0 / 63_360.0,
        "mm" => 25.4,
        "cm" => 2.54,
        "m" => 0.0254,
        "km" => 0.000_025_4,
        _ => return None,
    })
}

/// RevuCraft: make the run measured at `(page, index)` a cable run with `extra_inches` added for
/// the drop down the wall and the service loop; its reading and label include it.
pub fn set_cable(doc: &mut Document, page_index: usize, index: usize, extra_inches: f64) -> Result<()> {
    let (r, mut d) = annot(doc, page_index, index)?;
    if d.name(b"IT") != Some(b"PolyLineDimension") {
        return Err(invalid("a cable run is measured along its route"));
    }
    if !extra_inches.is_finite() || !(0.0..=1e7).contains(&extra_inches) {
        return Err(invalid("invalid cable allowance"));
    }
    let scale = Scale::read(doc, d.get(b"Measure").ok_or_else(|| invalid("measurement has no scale"))?)?;
    let k = per_inch(&scale.unit).ok_or_else(|| invalid("the page's scale isn't in a length unit the allowance can be added in"))?;
    d.set(b"PCExtra".to_vec(), Object::Real(extra_inches * k));
    d.set(b"PCCableRun".to_vec(), Object::Bool(true));
    d.set(b"Subj".to_vec(), PdfString::text("Cable run"));
    doc.update_dict(r, |target| *target = d).map_err(|e| invalid(&e.to_string()))?;
    names::rename_cable(doc, page_index, index)?;
    refresh(doc, page_index, index).map(|_| ())
}

pub fn refresh(doc: &mut Document, page_index: usize, index: usize) -> Result<bool> {
    let (r, mut d) = annot(doc, page_index, index)?;
    let kind = match d.name(b"IT") {
        Some(b"LineDimension") => Kind::Distance,
        Some(b"PolyLineDimension") => Kind::Perimeter,
        Some(b"PolygonDimension") => Kind::Area,
        _ => return Ok(false),
    };
    let m = read_measurement(doc, &d, kind, page_index, index)?;
    let old = text(&d, b"PCMeasureValue");
    if text(&d, b"Contents") == old || old.is_empty() {
        d.set(b"Contents".to_vec(), PdfString::text(&m.reading.label));
    }
    d.set(b"PCMeasureValue".to_vec(), PdfString::text(&m.reading.label));
    doc.update_dict(r, |target| *target = d).map_err(|e| invalid(&e.to_string()))?;
    pdfcraft_annot::set_appearance(doc, r)?;
    Ok(true)
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Measurement {
    pub page: usize,
    pub index: usize,
    pub id: String,
    pub author: String,
    pub label: String,
    pub points: Vec<Point>,
    pub scale: Scale,
    pub reading: Reading,
    /// RevuCraft: a cable run (its allowance, in the scale's unit, is in the reading).
    pub cable: bool,
    pub extra: f64,
    /// RevuCraft: its name in a workbook's Takeoff sheet (`Kitchen_floor`), empty when none was
    /// given ([`names`]).
    pub name: String,
}
/// A measurement annotation that couldn't be read (unsupported scale format, invalid
/// geometry). Its PDF data is left untouched.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Unsupported {
    pub page: usize,
    pub index: usize,
    pub reason: String,
}
/// Saved measurements plus the ones that were skipped.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Listing {
    pub measurements: Vec<Measurement>,
    pub unsupported: Vec<Unsupported>,
    /// The annotation limit stopped enumeration early.
    pub truncated: bool,
}
const MAX_ANNOTATIONS: usize = 100_000;
fn read_measurement(doc: &Document, d: &Dict, kind: Kind, page: usize, index: usize) -> Result<Measurement> {
    let scale = Scale::read(doc, d.get(b"Measure").ok_or_else(|| invalid("measurement has no scale"))?)?;
    let key = if kind == Kind::Distance { &b"L"[..] } else { b"Vertices" };
    let o = doc.resolve(d.get(key).ok_or_else(|| invalid("measurement has no vertices"))?);
    let a = o.as_array().ok_or_else(|| invalid("invalid measurement vertices"))?;
    if a.len() % 2 != 0 || a.len() > MAX_POINTS * 2 {
        return Err(invalid("invalid measurement vertex count"));
    }
    let v: Vec<f64> = a.iter().map(|n| doc.resolve(n).as_f64().ok_or_else(|| invalid("invalid vertex"))).collect::<Result<_>>()?;
    let points: Vec<Point> = v.as_chunks::<2>().0.to_vec();
    validate_geometry(kind, &points)?;
    let mut reading = reading(kind, &points, &scale)?;
    // A cable run's allowance (drop down the wall, service loop) is part of its length.
    let extra = if kind == Kind::Perimeter {
        d.get(b"PCExtra").and_then(Object::as_f64).filter(|v| v.is_finite() && (0.0..=MAX_EXTRA).contains(v)).unwrap_or(0.0)
    } else {
        0.0
    };
    if extra > 0.0 {
        reading.value += extra;
        reading.length += extra;
        reading.label = scale.format_length(reading.value)?;
    }
    let cable = matches!(d.get(b"PCCableRun"), Some(Object::Bool(true)));
    Ok(Measurement {
        page,
        index,
        id: text(d, b"NM"),
        author: text(d, b"T"),
        label: text(d, b"PCMeasureLabel"),
        points,
        scale,
        reading,
        cable,
        extra,
        name: text(d, names::KEY),
    })
}
/// Every measurement annotation. One that can't be read is reported in `unsupported`
/// rather than failing the whole list.
pub fn list(doc: &Document) -> Listing {
    let mut out = Listing::default();
    let mut count = 0usize;
    for (page, p) in pdfcraft_model::pages(doc).iter().enumerate() {
        let Some(o) = p.dict.get(b"Annots") else { continue };
        let o = doc.resolve(o);
        let Some(arr) = o.as_array() else { continue };
        for (index, a) in arr.iter().enumerate() {
            count = count.saturating_add(1);
            if count > MAX_ANNOTATIONS {
                out.truncated = true;
                return out;
            }
            let o = doc.resolve(a);
            let Some(d) = o.as_dict() else { continue };
            let kind = match d.name(b"IT") {
                Some(b"LineDimension") => Kind::Distance,
                Some(b"PolyLineDimension") => Kind::Perimeter,
                Some(b"PolygonDimension") => Kind::Area,
                _ => continue,
            };
            match read_measurement(doc, d, kind, page, index) {
                Ok(m) => out.measurements.push(m),
                Err(e) => out.unsupported.push(Unsupported { page, index, reason: e.to_string() }),
            }
        }
    }
    out
}
/// Last containing viewport wins, using the first point of the measurement (ISO 32000-2 §12.9).
/// Outside every viewport the page measures true size in inches.
pub fn scale_at(doc: &Document, page_index: usize, at: Point) -> Result<Scale> {
    scale_at_or(doc, page_index, at, 1.0 / 72.0, "in")
}
/// [`scale_at`], but outside every viewport the page measures true size in `unit`, of which there
/// are `per_point` in a PDF point (25.4 / 72 for millimetres).
pub fn scale_at_or(doc: &Document, page_index: usize, at: Point, per_point: f64, unit: &str) -> Result<Scale> {
    if let Some(scale) = viewport_scale_at(doc, page_index, at)? {
        return Ok(scale);
    }
    let p = page(doc, page_index)?;
    Scale::new(p.user_unit(doc) * per_point, unit, 2)
}
/// The scale of the measurement viewport containing `at`, or `None` when no viewport covers it
/// (the page is not calibrated there).
pub fn viewport_scale_at(doc: &Document, page_index: usize, at: Point) -> Result<Option<Scale>> {
    check_points(&[at])?;
    let p = page(doc, page_index)?;
    if let Some(o) = p.dict.get(b"VP") {
        let o = doc.resolve(o);
        let arr = o.as_array().ok_or_else(|| invalid("invalid page viewports"))?;
        for viewport in arr.iter().rev() {
            let o = doc.resolve(viewport);
            let Some(d) = o.as_dict() else { continue };
            if let Some(bbox) = d.get(b"BBox") {
                let o = doc.resolve(bbox);
                let b: Option<Vec<f64>> = o.as_array().and_then(|a| a.iter().map(|v| doc.resolve(v).as_f64()).collect());
                if let Some(b) = b.filter(|b| b.len() == 4 && b.iter().all(|v| v.is_finite()))
                    && at[0] >= b[0].min(b[2])
                    && at[0] <= b[0].max(b[2])
                    && at[1] >= b[1].min(b[3])
                    && at[1] <= b[1].max(b[3])
                {
                    return Scale::read(doc, d.get(b"Measure").ok_or_else(|| invalid("viewport has no measurement scale"))?).map(Some);
                }
            }
        }
    }
    Ok(None)
}
/// Inches in one of a scale's length units (`in`, `ft`, `yd`, `mi`, `mm`, `cm`, `m`, `km`).
pub fn unit_inches(unit: &str) -> Option<f64> {
    Some(match unit.trim() {
        "in" | "\"" | "inch" | "inches" => 1.0,
        "ft" | "'" | "feet" | "foot" => 12.0,
        "yd" => 36.0,
        "mi" => 63_360.0,
        "mm" => 1.0 / 25.4,
        "cm" => 1.0 / 2.54,
        "m" => 1000.0 / 25.4,
        "km" => 1_000_000.0 / 25.4,
        _ => return None,
    })
}
impl Scale {
    /// User-space units spanning `inches` of real-world length along X, when the scale's unit
    /// is a known length unit.
    pub fn points_for_inches(&self, inches: f64) -> Option<f64> {
        let per_point = self.x * self.distance_factor * unit_inches(&self.unit)?;
        let points = inches / per_point;
        (points.is_finite() && points > 0.0).then_some(points)
    }
}
/// Add a named rectangular viewport. Existing viewports are preserved in drawing order.
pub fn set_scale(doc: &mut Document, page_index: usize, bbox: [f64; 4], name: &str, scale: &Scale) -> Result<()> {
    check_points(&[[bbox[0], bbox[1]], [bbox[2], bbox[3]]])?;
    if bbox[2] <= bbox[0] || bbox[3] <= bbox[1] || name.len() > 256 {
        return Err(invalid("viewport needs a positive rectangle and a name of at most 256 bytes"));
    }
    let p = page(doc, page_index)?;
    let vp = p.dict.get(b"VP");
    let mut list = match vp {
        Some(o) => doc.resolve(o).as_array().cloned().ok_or_else(|| invalid("invalid page viewports"))?,
        None => Vec::new(),
    };
    if list.len() >= 1024 {
        return Err(invalid("page has too many measurement viewports"));
    }
    let mut d = Dict::new();
    d.set(b"Type".to_vec(), Object::name("Viewport"));
    d.set(b"BBox".to_vec(), nums(bbox));
    d.set(b"Name".to_vec(), PdfString::text(name));
    d.set(b"Measure".to_vec(), Object::Dict(scale.dictionary()?));
    list.push(Object::Dict(d));
    // An indirect /VP array stays indirect (it may be shared); otherwise the page holds it.
    match vp.and_then(Object::as_ref) {
        Some(r) => doc.set(r, Object::Array(list)),
        None => doc.update_dict(p.obj, |d| d.set(b"VP".to_vec(), Object::Array(list))).map_err(|e| invalid(&e.to_string()))?,
    }
    Ok(())
}
pub fn csv(measurements: &[Measurement]) -> String {
    // Prefix spreadsheet formula triggers; quoting alone does not prevent evaluation.
    let field = |s: &str| {
        let prefix = if s.trim_start().starts_with(['=', '+', '-', '@']) || s.starts_with(['\t', '\r']) { "'" } else { "" };
        format!("\"{prefix}{}\"", s.replace('"', "\"\""))
    };
    let mut out = "Page,Index,Type,Value,Unit,Label,Author,Scale\r\n".to_string();
    for m in measurements {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{}\r\n",
            m.page.saturating_add(1),
            m.index.saturating_add(1),
            m.reading.kind.name(),
            m.reading.value,
            field(&m.reading.unit),
            field(&m.label),
            field(&m.author),
            field(&m.scale.ratio)
        ));
    }
    out
}

#[cfg(test)]
mod tests;

/// Display points (top-left, y down, after Rotate/UserUnit) to PDF user space.
pub fn view_to_user(doc: &Document, page_index: usize, at: Point) -> Result<Point> {
    check_points(&[at])?;
    let page = page(doc, page_index)?;
    let (_, height) = page.display_size(doc);
    let m = pdfcraft_content::Matrix(page.view_matrix(doc));
    let (x, y) = m.apply(at[0], height - at[1]);
    check_points(&[[x, y]])?;
    Ok([x, y])
}
pub fn user_to_view(doc: &Document, page_index: usize, at: Point) -> Result<Point> {
    check_points(&[at])?;
    let page = page(doc, page_index)?;
    let (_, height) = page.display_size(doc);
    let m = pdfcraft_content::Matrix(page.view_matrix(doc)).invert().ok_or_else(|| invalid("invalid page transform"))?;
    let (x, y) = m.apply(at[0], at[1]);
    Ok([x, height - y])
}
