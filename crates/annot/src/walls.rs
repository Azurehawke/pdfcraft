//! RevuCraft: smart walls, after BIMCraft's walls (and Revit's). A wall stays a PDF markup (a
//! four-sided polygon, the Wall tool's outline) that other viewers draw as it is, but it knows
//! about the walls it meets and the doors and windows in it:
//!
//! - `/PCWall` on the wall says which of its ends are joined to another wall (their end line isn't
//!   drawn, so an L or a chain's corner is clean), where a face has a gap (a T: the wall that meets
//!   it runs on through) and where the doors and windows are (the wall is open there).
//! - A door or window is one markup of its own, a polyline drawn by its `/PCOpening`: the wall it
//!   is in (by `/NM`), where along the wall's centre line (0 to 1), which side it opens to, which
//!   end it hinges at, and its mark (D1, W1). It moves, stretches and is deleted with its wall, and
//!   moving it slides it along the wall.
//!
//! The wall's faces are its outline's sides 0–1 and 3–2 (as the Wall tool draws them), its ends
//! 3–0 (A) and 1–2 (B).

use pdfcraft_cos::{Dict, Document, Object, PdfString};

use crate::{
    AnnotError, Meta, NewAnnotation, Rgb, Shape, Style, add_annotation, annot_dict, annot_ref, annots, delete_annotation, n, page_ref, set_appearance,
};

type Pt = [f64; 2];

pub const WALL: &[u8] = b"PCWall";
pub const OPENING: &[u8] = b"PCOpening";

/// Gaps and openings kept per wall, at most.
const MAX_PARTS: usize = 64;
/// Door and window lines on paper, points.
const PEN: f64 = 0.75;
/// A window is drawn at least this thick (a hairline wall still shows one).
const MIN_WINDOW: f64 = 4.0;

/// What a wall knows beyond its outline.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct WallData {
    /// End A (side 3–0) and end B (side 1–2) are joined to another wall: no line across them.
    pub ja: bool,
    pub jb: bool,
    /// Gaps in a face (0: side 0–1, 1: side 3–2) along the centre line, 0 to 1.
    pub gaps: Vec<(u8, f64, f64)>,
    /// Where doors and windows open the wall, along its centre line, 0 to 1.
    pub open: Vec<(f64, f64)>,
}

/// A smart wall on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    pub index: usize,
    /// Its `/NM`, which its doors and windows name.
    pub nm: String,
    pub quad: [Pt; 4],
    pub data: WallData,
    pub locked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningKind {
    Door,
    Window,
}

impl OpeningKind {
    fn name(self) -> &'static str {
        match self {
            Self::Door => "Door",
            Self::Window => "Window",
        }
    }
}

/// Where a door or window is in its wall and how it opens.
#[derive(Clone, Debug, PartialEq)]
pub struct OpeningSpec {
    pub kind: OpeningKind,
    /// Its ends along the wall's centre line, 0 to 1 (`t0` < `t1`).
    pub t0: f64,
    pub t1: f64,
    /// The side it opens to: 1 toward face 0 (side 0–1), -1 toward face 1.
    pub side: f64,
    /// A door hinges at `t1` rather than `t0`.
    pub flip: bool,
    /// Its mark (D1, W1), shown beside it and listed.
    pub mark: String,
}

/// A door or window on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    pub index: usize,
    /// The `/NM` of the wall it is in.
    pub host: String,
    pub spec: OpeningSpec,
}

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: Pt, b: Pt) -> Pt {
    [a[0] + b[0], a[1] + b[1]]
}

fn mul(a: Pt, k: f64) -> Pt {
    [a[0] * k, a[1] * k]
}

fn dot(a: Pt, b: Pt) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn mid(a: Pt, b: Pt) -> Pt {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

fn finite(p: Pt) -> bool {
    p.iter().all(|v| v.is_finite())
}

/// A wall's centre line and faces: from the middle of end A along `along` for `len`, the faces
/// `h0` and `h1` off it toward `left`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub a: Pt,
    pub along: Pt,
    pub left: Pt,
    pub len: f64,
    pub h0: f64,
    pub h1: f64,
}

impl Axis {
    /// The point `t` along the centre line, `h` off it.
    pub fn at(&self, t: f64, h: f64) -> Pt {
        add(add(self.a, mul(self.along, t * self.len)), mul(self.left, h))
    }

    /// How far along the centre line `p` is, 0 to 1 over the wall.
    pub fn t_of(&self, p: Pt) -> f64 {
        dot(sub(p, self.a), self.along) / self.len
    }

    /// The wall's thickness.
    pub fn thickness(&self) -> f64 {
        (self.h0 - self.h1).abs()
    }
}

/// The axis of a wall outline, when it has a length and a thickness.
pub fn axis(q: &[Pt; 4]) -> Option<Axis> {
    if !q.iter().all(|p| finite(*p)) {
        return None;
    }
    let a = mid(q[3], q[0]);
    let b = mid(q[1], q[2]);
    let d = sub(b, a);
    let len = d[0].hypot(d[1]);
    if !(len.is_finite() && len > 1e-6) {
        return None;
    }
    let along = [d[0] / len, d[1] / len];
    let left = [-along[1], along[0]];
    let (h0, h1) = (dot(left, sub(q[0], a)), dot(left, sub(q[3], a)));
    ((h0 - h1).abs() > 1e-6).then_some(Axis { a, along, left, len, h0, h1 })
}

fn numbers(d: &Dict, key: &[u8]) -> Vec<f64> {
    d.get(key).and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).filter(|v| v.is_finite()).collect()).unwrap_or_default()
}

fn flag(d: &Dict, key: &[u8]) -> bool {
    matches!(d.get(key), Some(Object::Bool(true)))
}

fn num_array(v: &[f64]) -> Object {
    Object::Array(v.iter().map(|x| Object::Real(*x)).collect())
}

/// A range along the wall, ordered and within it, if it has a length.
fn range(t0: f64, t1: f64) -> Option<(f64, f64)> {
    let (a, b) = (t0.min(t1).clamp(0.0, 1.0), t0.max(t1).clamp(0.0, 1.0));
    (b - a > 1e-9).then_some((a, b))
}

/// What `/PCWall` says, when there is one.
pub fn wall_data(d: &Dict) -> Option<WallData> {
    let w = d.get(WALL)?.as_dict()?;
    let gaps = numbers(w, b"Gaps")
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|g| {
            let face = if g[0] < 0.5 { 0 } else { 1 };
            range(g[1], g[2]).map(|(a, b)| (face, a, b))
        })
        .take(MAX_PARTS)
        .collect();
    let open = numbers(w, b"Open").as_chunks::<2>().0.iter().filter_map(|o| range(o[0], o[1])).take(MAX_PARTS).collect();
    Some(WallData { ja: flag(w, b"JA"), jb: flag(w, b"JB"), gaps, open })
}

fn data_dict(data: &WallData) -> Dict {
    let mut w = Dict::new();
    w.set(b"JA".to_vec(), Object::Bool(data.ja));
    w.set(b"JB".to_vec(), Object::Bool(data.jb));
    let gaps: Vec<f64> = data.gaps.iter().take(MAX_PARTS).flat_map(|(f, a, b)| [f64::from(*f), *a, *b]).collect();
    w.set(b"Gaps".to_vec(), num_array(&gaps));
    let open: Vec<f64> = data.open.iter().take(MAX_PARTS).flat_map(|(a, b)| [*a, *b]).collect();
    w.set(b"Open".to_vec(), num_array(&open));
    w
}

fn quad_of(d: &Dict) -> Option<[Pt; 4]> {
    let v = numbers(d, b"Vertices");
    match v.as_slice() {
        [a, b, c, e, f, g, h, i] => Some([[*a, *b], [*c, *e], [*f, *g], [*h, *i]]),
        _ => None,
    }
}

fn text(d: &Dict, key: &[u8]) -> String {
    d.get(key).and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default()
}

/// What `/PCOpening` says, when there is one: the wall it is in and the spec.
pub fn opening_data(d: &Dict) -> Option<(String, OpeningSpec)> {
    let o = d.get(OPENING)?.as_dict()?;
    let host = text(o, b"Host");
    let kind = match o.name(b"Kind") {
        Some(b"Window") => OpeningKind::Window,
        _ => OpeningKind::Door,
    };
    let num = |k: &[u8]| o.get(k).and_then(Object::as_f64).filter(|v| v.is_finite());
    let (t0, t1) = range(num(b"T0")?, num(b"T1")?)?;
    let side = if num(b"Side").unwrap_or(1.0) < 0.0 { -1.0 } else { 1.0 };
    let mark: String = text(o, b"Mark").chars().take(12).collect();
    (!host.is_empty()).then_some((host, OpeningSpec { kind, t0, t1, side, flip: flag(o, b"Flip"), mark }))
}

/// The smart walls on `page`.
pub fn walls(doc: &Document, page: usize) -> Vec<Wall> {
    let Ok(p) = page_ref(doc, page) else { return Vec::new() };
    annots(doc, p)
        .iter()
        .enumerate()
        .filter_map(|(index, e)| {
            let obj = doc.resolve(e);
            let d = obj.as_dict()?;
            if d.name(b"Subtype") != Some(b"Polygon") {
                return None;
            }
            let data = wall_data(d)?;
            let quad = quad_of(d)?;
            axis(&quad)?;
            let locked = d.get(b"F").and_then(Object::as_int).unwrap_or(0) & 128 != 0;
            Some(Wall { index, nm: text(d, b"NM"), quad, data, locked })
        })
        .collect()
}

/// The doors and windows on `page`.
pub fn openings(doc: &Document, page: usize) -> Vec<Opening> {
    let Ok(p) = page_ref(doc, page) else { return Vec::new() };
    annots(doc, p)
        .iter()
        .enumerate()
        .filter_map(|(index, e)| {
            let obj = doc.resolve(e);
            let (host, spec) = opening_data(obj.as_dict()?)?;
            Some(Opening { index, host, spec })
        })
        .collect()
}

/// `range` less the `cuts` in it.
fn minus(range: (f64, f64), cuts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = vec![range];
    for &(c0, c1) in cuts {
        out = out
            .into_iter()
            .flat_map(|(a, b)| {
                if c1 <= a || c0 >= b { vec![(a, b)] } else { [(a, c0.max(a)), (c1.min(b), b)].into_iter().filter(|(x, y)| y - x > 1e-9).collect() }
            })
            .collect();
    }
    out
}

fn line(c: &mut String, p: Pt, q: Pt) {
    c.push_str(&format!("{} {} m {} {} l S\n", n(p[0]), n(p[1]), n(q[0]), n(q[1])));
}

fn rg(c: Rgb, op: &str) -> String {
    format!("{} {} {} {op}\n", n(c[0]), n(c[1]), n(c[2]))
}

/// A wall's drawing: its outline filled, open where its doors and windows are, its faces drawn
/// but for their gaps and openings, its ends drawn unless joined, and the jambs of its openings.
pub(crate) fn wall_content(d: &Dict, stroke: Option<Rgb>, fill: Option<Rgb>, w: f64) -> Option<String> {
    let data = wall_data(d)?;
    let q = quad_of(d)?;
    let ax = axis(&q)?;
    let mut c = String::new();
    // The outline filled, but where the doors and windows open it.
    if let Some(f) = fill {
        c.push_str(&rg(f, "rg"));
        for (a, b) in minus((0.0, 1.0), &data.open) {
            let (pa0, pa1) = if a <= 1e-9 { (q[0], q[3]) } else { (ax.at(a, ax.h0), ax.at(a, ax.h1)) };
            let (pb0, pb1) = if b >= 1.0 - 1e-9 { (q[1], q[2]) } else { (ax.at(b, ax.h0), ax.at(b, ax.h1)) };
            c.push_str(&format!(
                "{} {} m {} {} l {} {} l {} {} l h f\n",
                n(pa0[0]),
                n(pa0[1]),
                n(pb0[0]),
                n(pb0[1]),
                n(pb1[0]),
                n(pb1[1]),
                n(pa1[0]),
                n(pa1[1])
            ));
        }
    }
    let Some(s) = stroke else { return Some(c) };
    c.push_str(&format!("{}{} w 0 J 1 j\n", rg(s, "RG"), n(w)));
    for (face, h, from, to) in [(0u8, ax.h0, q[0], q[1]), (1u8, ax.h1, q[3], q[2])] {
        let mut cuts: Vec<(f64, f64)> = data.open.clone();
        cuts.extend(data.gaps.iter().filter(|g| g.0 == face).map(|g| (g.1, g.2)));
        let (t_from, t_to) = (ax.t_of(from), ax.t_of(to));
        for (a, b) in minus((t_from.min(t_to), t_from.max(t_to)), &cuts) {
            // The face's own ends where they are its corners (mitred), else along it.
            let pa = if (a - t_from).abs() < 1e-9 {
                from
            } else if (a - t_to).abs() < 1e-9 {
                to
            } else {
                ax.at(a, h)
            };
            let pb = if (b - t_to).abs() < 1e-9 {
                to
            } else if (b - t_from).abs() < 1e-9 {
                from
            } else {
                ax.at(b, h)
            };
            line(&mut c, pa, pb);
        }
    }
    if !data.ja {
        line(&mut c, q[3], q[0]);
    }
    if !data.jb {
        line(&mut c, q[1], q[2]);
    }
    for &(t0, t1) in &data.open {
        line(&mut c, ax.at(t0, ax.h0), ax.at(t0, ax.h1));
        line(&mut c, ax.at(t1, ax.h0), ax.at(t1, ax.h1));
    }
    Some(c)
}

/// Where a door or window draws, worked out from its wall: its ends on the centre line, the
/// direction it opens to, and half the wall's thickness.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    from: Pt,
    to: Pt,
    along: Pt,
    normal: Pt,
    half: f64,
}

fn place(ax: &Axis, spec: &OpeningSpec) -> Place {
    // The centre of the wall, between its faces (the outline's middle line may sit off it).
    let centre = (ax.h0 + ax.h1) / 2.0;
    let side = if spec.side < 0.0 { -1.0 } else { 1.0 };
    // Face 0 is the one at h0: the normal points to it for side 1.
    let toward0 = if ax.h0 >= ax.h1 { ax.left } else { mul(ax.left, -1.0) };
    Place { from: ax.at(spec.t0, centre), to: ax.at(spec.t1, centre), along: ax.along, normal: mul(toward0, side), half: ax.thickness() / 2.0 }
}

/// A door's or window's outline points: what selects it and what other viewers can draw.
fn outline(kind: OpeningKind, at: &Place, flip: bool) -> Vec<Pt> {
    match kind {
        OpeningKind::Door => {
            let (hinge, strike) = if flip { (at.to, at.from) } else { (at.from, at.to) };
            let width = sub(strike, hinge)[0].hypot(sub(strike, hinge)[1]);
            let h = add(hinge, mul(at.normal, at.half));
            let s = add(strike, mul(at.normal, at.half));
            let tip = add(h, mul(at.normal, width));
            // The swing: a quarter circle from the open leaf's tip to the strike side.
            let mut pts = vec![h, tip];
            for i in 1..=12 {
                let a = std::f64::consts::FRAC_PI_2 * f64::from(i) / 12.0;
                let toward = mul(sub(s, h), a.sin());
                pts.push(add(add(h, mul(at.normal, width * a.cos())), toward));
            }
            pts
        }
        OpeningKind::Window => {
            let half = at.half.max(MIN_WINDOW / 2.0);
            let c = |o: Pt, k: f64| add(o, mul(at.normal, half * k));
            vec![c(at.from, -1.0), c(at.to, -1.0), c(at.to, 1.0), c(at.from, 1.0), c(at.from, -1.0)]
        }
    }
}

/// Where a mark is written: in the door's swing, or beside the window.
fn mark_at(kind: OpeningKind, at: &Place) -> (Pt, f64) {
    let width = sub(at.to, at.from)[0].hypot(sub(at.to, at.from)[1]);
    let size = (width * 0.22).clamp(4.0, 14.0);
    let centre = mid(at.from, at.to);
    let off = match kind {
        OpeningKind::Door => at.half + width * 0.35,
        OpeningKind::Window => at.half.max(MIN_WINDOW / 2.0) + size * 1.2,
    };
    (add(centre, mul(at.normal, off)), size)
}

/// A door's or window's drawing (and whether it needs the label font).
pub(crate) fn opening_content(d: &Dict, stroke: Option<Rgb>) -> Option<(String, bool)> {
    let o = d.get(OPENING)?.as_dict()?;
    let (_, spec) = opening_data(d)?;
    let pt = |k: &[u8]| -> Option<Pt> {
        match numbers(o, k).as_slice() {
            [x, y] => Some([*x, *y]),
            _ => None,
        }
    };
    let at = Place {
        from: pt(b"From")?,
        to: pt(b"To")?,
        along: pt(b"Along")?,
        normal: pt(b"Normal")?,
        half: o.get(b"Half").and_then(Object::as_f64).filter(|h| h.is_finite() && *h >= 0.0)?,
    };
    let col = stroke.unwrap_or([0.0; 3]);
    let mut c = format!("{}{} w 1 J 1 j\n", rg(col, "RG"), n(PEN));
    match spec.kind {
        OpeningKind::Door => {
            let pts = outline(OpeningKind::Door, &at, spec.flip);
            if let [h, tip, rest @ ..] = pts.as_slice() {
                line(&mut c, *h, *tip);
                c.push_str(&format!("{} {} m\n", n(tip[0]), n(tip[1])));
                for p in rest {
                    c.push_str(&format!("{} {} l\n", n(p[0]), n(p[1])));
                }
                c.push_str("S\n");
            }
        }
        OpeningKind::Window => {
            let pts = outline(OpeningKind::Window, &at, false);
            if let [p0, p1, p2, p3, ..] = pts.as_slice() {
                c.push_str(&format!(
                    "1 1 1 rg {} {} m {} {} l {} {} l {} {} l h B\n",
                    n(p0[0]),
                    n(p0[1]),
                    n(p1[0]),
                    n(p1[1]),
                    n(p2[0]),
                    n(p2[1]),
                    n(p3[0]),
                    n(p3[1])
                ));
            }
            line(&mut c, at.from, at.to);
        }
    }
    let label = !spec.mark.is_empty();
    if label {
        let (p, size) = mark_at(spec.kind, &at);
        let w = crate::appearance::text_width(&spec.mark, size);
        let mark: String = spec.mark.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-').collect();
        c.push_str(&format!("{}BT /Helv {} Tf {} {} Td ({mark}) Tj ET\n", rg(col, "rg"), n(size), n(p[0] - w / 2.0), n(p[1] - size * 0.35)));
    }
    Some((c, label))
}

/// A door's or window's `/Rect`: its outline and its mark, with room for its lines.
fn opening_rect(spec: &OpeningSpec, at: &Place) -> [f64; 4] {
    let mut pts = outline(spec.kind, at, spec.flip);
    if !spec.mark.is_empty() {
        let (p, size) = mark_at(spec.kind, at);
        let half = crate::appearance::text_width(&spec.mark, size) / 2.0;
        pts.push([p[0] - half, p[1] - size * 0.35]);
        pts.push([p[0] + half, p[1] + size * 0.8]);
    }
    let b = crate::bounds(pts.into_iter().filter(|p| finite(*p))).unwrap_or_default();
    crate::grow(b, PEN + 1.0)
}

fn opening_dict(host: &str, spec: &OpeningSpec, at: &Place) -> Dict {
    let mut o = Dict::new();
    o.set(b"Host".to_vec(), PdfString::text(host));
    o.set(b"Kind".to_vec(), Object::name(spec.kind.name()));
    o.set(b"T0".to_vec(), Object::Real(spec.t0));
    o.set(b"T1".to_vec(), Object::Real(spec.t1));
    o.set(b"Side".to_vec(), Object::Real(spec.side));
    o.set(b"Flip".to_vec(), Object::Bool(spec.flip));
    o.set(b"Mark".to_vec(), PdfString::text(&spec.mark));
    o.set(b"From".to_vec(), num_array(&at.from));
    o.set(b"To".to_vec(), num_array(&at.to));
    o.set(b"Along".to_vec(), num_array(&at.along));
    o.set(b"Normal".to_vec(), num_array(&at.normal));
    o.set(b"Half".to_vec(), Object::Real(at.half));
    o
}

/// Adds a smart wall with outline `quad`; returns its index on the page.
pub fn add_wall(
    doc: &mut Document,
    page: usize,
    quad: [Pt; 4],
    data: &WallData,
    style: &Style,
    author: &str,
    meta: &Meta,
) -> Result<usize, AnnotError> {
    axis(&quad).ok_or_else(|| AnnotError::Invalid("a wall needs a length and a thickness".into()))?;
    let new = NewAnnotation {
        page,
        shape: Shape::Polygon { vertices: quad.to_vec(), cloud: false },
        style: style.clone(),
        contents: String::new(),
        author: author.into(),
    };
    let index = add_annotation(doc, &new, meta)?;
    let (_, r) = annot_ref(doc, page, index)?;
    let data = data.clone();
    doc.update_dict(r, |d| {
        d.set(WALL.to_vec(), Object::Dict(data_dict(&data)));
        d.set(b"Subj".to_vec(), PdfString::text("Wall"));
    })?;
    set_appearance(doc, r)?;
    Ok(index)
}

/// Changes a smart wall: its outline, which ends are joined, and a gap added to a face. Its doors
/// and windows follow.
#[allow(clippy::too_many_arguments)]
pub fn set_wall(
    doc: &mut Document,
    page: usize,
    index: usize,
    quad: Option<[Pt; 4]>,
    ja: Option<bool>,
    jb: Option<bool>,
    gap: Option<(u8, f64, f64)>,
    meta: &Meta,
) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    let d = annot_dict(doc, r);
    let mut data = wall_data(&d).ok_or_else(|| AnnotError::Invalid("that isn't a smart wall".into()))?;
    if let Some(q) = quad {
        axis(&q).ok_or_else(|| AnnotError::Invalid("a wall needs a length and a thickness".into()))?;
        crate::geometry::set_vertices(doc, page, index, &q, false, meta)?;
    }
    if let Some(v) = ja {
        data.ja = v;
    }
    if let Some(v) = jb {
        data.jb = v;
    }
    if let Some((face, a, b)) = gap
        && let Some((a, b)) = range(a, b)
        && data.gaps.len() < MAX_PARTS
    {
        data.gaps.push((face.min(1), a, b));
    }
    doc.update_dict(r, |d| d.set(WALL.to_vec(), Object::Dict(data_dict(&data))))?;
    sync(doc, page, index, meta)
}

/// Adds a door or window to the smart wall at `wall`; returns its index on the page.
pub fn add_opening(
    doc: &mut Document,
    page: usize,
    wall: usize,
    spec: &OpeningSpec,
    color: Rgb,
    author: &str,
    meta: &Meta,
) -> Result<usize, AnnotError> {
    let w = walls(doc, page).into_iter().find(|w| w.index == wall).ok_or_else(|| AnnotError::Invalid("that isn't a smart wall".into()))?;
    if w.nm.is_empty() {
        return Err(AnnotError::Invalid("the wall has no name to be found by".into()));
    }
    let (t0, t1) = range(spec.t0, spec.t1).ok_or_else(|| AnnotError::Invalid("a door or window needs a width".into()))?;
    let spec = OpeningSpec { t0, t1, side: if spec.side < 0.0 { -1.0 } else { 1.0 }, mark: spec.mark.chars().take(12).collect(), ..spec.clone() };
    let ax = axis(&w.quad).ok_or_else(|| AnnotError::Invalid("the wall has no length".into()))?;
    let at = place(&ax, &spec);
    let style = Style { color, opacity: 1.0, width: PEN, fill: None };
    let new = NewAnnotation {
        page,
        shape: Shape::PolyLine { vertices: outline(spec.kind, &at, spec.flip), start: crate::LineEnding::None, end: crate::LineEnding::None },
        style,
        contents: spec.mark.clone(),
        author: author.into(),
    };
    let index = add_annotation(doc, &new, meta)?;
    let (_, r) = annot_ref(doc, page, index)?;
    let host = w.nm.clone();
    let kind = spec.kind;
    doc.update_dict(r, |d| {
        d.set(OPENING.to_vec(), Object::Dict(opening_dict(&host, &spec, &at)));
        d.set(b"Subj".to_vec(), PdfString::text(kind.name()));
    })?;
    // Drawn, with its label's room, as its wall is brought up to it.
    sync(doc, page, wall, meta)?;
    Ok(index)
}

/// Brings the doors and windows of the wall at `wall` up to its outline, and its openings up to
/// them (a copy of a wall has none of its doors, so none of its openings).
pub fn sync(doc: &mut Document, page: usize, wall: usize, meta: &Meta) -> Result<(), AnnotError> {
    let Some(w) = walls(doc, page).into_iter().find(|w| w.index == wall) else { return Ok(()) };
    let Some(ax) = axis(&w.quad) else { return Ok(()) };
    let mine: Vec<Opening> = if w.nm.is_empty() { Vec::new() } else { openings(doc, page).into_iter().filter(|o| o.host == w.nm).collect() };
    for o in &mine {
        let at = place(&ax, &o.spec);
        crate::geometry::set_vertices(doc, page, o.index, &outline(o.spec.kind, &at, o.spec.flip), false, meta)?;
        let (_, r) = annot_ref(doc, page, o.index)?;
        let (host, spec) = (o.host.clone(), o.spec.clone());
        let rect = opening_rect(&spec, &at);
        doc.update_dict(r, |d| {
            d.set(OPENING.to_vec(), Object::Dict(opening_dict(&host, &spec, &at)));
            d.set(b"Rect".to_vec(), num_array(&rect));
        })?;
        set_appearance(doc, r)?;
    }
    let open: Vec<(f64, f64)> = mine.iter().map(|o| (o.spec.t0, o.spec.t1)).take(MAX_PARTS).collect();
    let (_, r) = annot_ref(doc, page, wall)?;
    let data = WallData { open, ..w.data };
    doc.update_dict(r, |d| d.set(WALL.to_vec(), Object::Dict(data_dict(&data))))?;
    set_appearance(doc, r)
}

/// After the markup at `index` moved by `delta`: a wall's doors and windows follow it; a door or
/// window slides along its wall by as much of the move as runs along it.
pub fn after_move(doc: &mut Document, page: usize, index: usize, delta: Option<Pt>, meta: &Meta) -> Result<(), AnnotError> {
    if walls(doc, page).iter().any(|w| w.index == index) {
        return sync(doc, page, index, meta);
    }
    let Some(o) = openings(doc, page).into_iter().find(|o| o.index == index) else { return Ok(()) };
    let Some(w) = walls(doc, page).into_iter().find(|w| w.nm == o.host) else { return Ok(()) };
    if let (Some(d), Some(ax)) = (delta.filter(|d| finite(*d)), axis(&w.quad)) {
        let width = o.spec.t1 - o.spec.t0;
        let t0 = (o.spec.t0 + dot(d, ax.along) / ax.len).clamp(0.0, (1.0 - width).max(0.0));
        let spec = OpeningSpec { t0, t1: t0 + width, ..o.spec.clone() };
        let (_, r) = annot_ref(doc, page, index)?;
        let at = place(&ax, &spec);
        let host = o.host.clone();
        doc.update_dict(r, |dd| dd.set(OPENING.to_vec(), Object::Dict(opening_dict(&host, &spec, &at))))?;
    }
    sync(doc, page, w.index, meta)
}

/// Flips the door or window at `index`: its hinge to the other end, or its swing to the other side.
pub fn flip(doc: &mut Document, page: usize, index: usize, hinge: bool, meta: &Meta) -> Result<(), AnnotError> {
    let o = openings(doc, page).into_iter().find(|o| o.index == index).ok_or_else(|| AnnotError::Invalid("that isn't a door or window".into()))?;
    let w = walls(doc, page).into_iter().find(|w| w.nm == o.host).ok_or_else(|| AnnotError::Invalid("its wall is gone".into()))?;
    let spec = if hinge { OpeningSpec { flip: !o.spec.flip, ..o.spec.clone() } } else { OpeningSpec { side: -o.spec.side, ..o.spec.clone() } };
    let ax = axis(&w.quad).ok_or_else(|| AnnotError::Invalid("the wall has no length".into()))?;
    let at = place(&ax, &spec);
    let (_, r) = annot_ref(doc, page, index)?;
    let host = o.host.clone();
    doc.update_dict(r, |d| d.set(OPENING.to_vec(), Object::Dict(opening_dict(&host, &spec, &at))))?;
    sync(doc, page, w.index, meta)
}

/// Deletes the door or window at `index`, and its wall closes up. Returns whether it was one.
pub fn delete_opening(doc: &mut Document, page: usize, index: usize, meta: &Meta) -> Result<bool, AnnotError> {
    let Some(o) = openings(doc, page).into_iter().find(|o| o.index == index) else { return Ok(false) };
    delete_annotation(doc, page, index)?;
    if let Some(w) = walls(doc, page).into_iter().find(|w| w.nm == o.host) {
        sync(doc, page, w.index, meta)?;
    }
    Ok(true)
}

/// Deletes the doors and windows on `page` whose wall is gone.
pub fn delete_orphans(doc: &mut Document, page: usize) -> Result<(), AnnotError> {
    let names: Vec<String> = walls(doc, page).into_iter().map(|w| w.nm).filter(|n| !n.is_empty()).collect();
    let doomed: Vec<usize> = openings(doc, page).into_iter().filter(|o| !names.contains(&o.host)).map(|o| o.index).collect();
    for i in doomed.into_iter().rev() {
        delete_annotation(doc, page, i)?;
    }
    Ok(())
}

/// Makes the door or window at `index` a plain drawing, in no wall (a copy that has none).
pub fn detach(doc: &mut Document, page: usize, index: usize) -> Result<(), AnnotError> {
    if !openings(doc, page).iter().any(|o| o.index == index) {
        return Ok(());
    }
    let (_, r) = annot_ref(doc, page, index)?;
    doc.update_dict(r, |d| {
        d.remove(OPENING);
    })?;
    set_appearance(doc, r)
}

/// Moves the door or window at `index` into the wall at `wall` (named `host`), where it is along
/// that wall as it was along its own: a copied door into the copy of its wall.
pub fn rehost(doc: &mut Document, page: usize, index: usize, host: &str, wall: usize, meta: &Meta) -> Result<(), AnnotError> {
    let Some(o) = openings(doc, page).into_iter().find(|o| o.index == index) else { return Ok(()) };
    let Some(w) = walls(doc, page).into_iter().find(|w| w.index == wall && w.nm == host) else { return Ok(()) };
    let Some(ax) = axis(&w.quad) else { return Ok(()) };
    let at = place(&ax, &o.spec);
    let (_, r) = annot_ref(doc, page, index)?;
    let spec = o.spec.clone();
    doc.update_dict(r, |d| d.set(OPENING.to_vec(), Object::Dict(opening_dict(host, &spec, &at))))?;
    sync(doc, page, wall, meta)
}

/// The middle of the door or window at `index`, if it is one.
pub fn opening_centre(doc: &Document, page: usize, index: usize) -> Option<Pt> {
    let p = page_ref(doc, page).ok()?;
    let obj = doc.resolve(annots(doc, p).get(index)?);
    let o = obj.as_dict()?.get(OPENING)?.as_dict()?;
    let pt = |k: &[u8]| match numbers(o, k).as_slice() {
        [x, y] => Some([*x, *y]),
        _ => None,
    };
    Some(mid(pt(b"From")?, pt(b"To")?))
}

/// The `/NM` of the wall at `index` on `page`, if it is a smart wall.
pub fn wall_name(doc: &Document, page: usize, index: usize) -> Option<String> {
    walls(doc, page).into_iter().find(|w| w.index == index).map(|w| w.nm).filter(|n| !n.is_empty())
}

/// The `/NM` of the wall the door or window at `index` is in.
pub fn host_of(doc: &Document, page: usize, index: usize) -> Option<String> {
    openings(doc, page).into_iter().find(|o| o.index == index).map(|o| o.host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_lose_their_cuts() {
        assert_eq!(minus((0.0, 1.0), &[(0.2, 0.3), (0.6, 0.7)]), vec![(0.0, 0.2), (0.3, 0.6), (0.7, 1.0)]);
        assert_eq!(minus((0.0, 1.0), &[(-1.0, 2.0)]), vec![]);
        assert_eq!(minus((0.2, 0.4), &[(0.5, 0.6)]), vec![(0.2, 0.4)]);
    }

    #[test]
    fn a_walls_axis_is_its_centre_line() {
        // 100 long along x, 6 thick, face 0 on top.
        let q = [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]];
        let ax = axis(&q).unwrap();
        assert_eq!((ax.a, ax.along, ax.len), ([0.0, 0.0], [1.0, 0.0], 100.0));
        assert_eq!((ax.h0, ax.h1), (3.0, -3.0));
        assert_eq!(ax.at(0.5, 3.0), [50.0, 3.0]);
        assert!((ax.t_of([25.0, 7.0]) - 0.25).abs() < 1e-12);
        assert!(axis(&[[0.0, 0.0]; 4]).is_none());
        assert!(axis(&[[0.0, 0.0], [100.0, 0.0], [100.0, 0.0], [0.0, 0.0]]).is_none(), "no thickness");
    }

    #[test]
    fn doors_swing_from_their_hinge_to_the_side_they_open_to() {
        let ax = axis(&[[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]]).unwrap();
        let spec = OpeningSpec { kind: OpeningKind::Door, t0: 0.4, t1: 0.7, side: 1.0, flip: false, mark: "D1".into() };
        let at = place(&ax, &spec);
        assert_eq!((at.from, at.to, at.normal, at.half), ([40.0, 0.0], [70.0, 0.0], [0.0, 1.0], 3.0));
        let pts = outline(OpeningKind::Door, &at, false);
        assert_eq!(pts[0], [40.0, 3.0], "the hinge, on the face it opens to");
        assert_eq!(pts[1], [40.0, 33.0], "the leaf, open square to the wall");
        let last = *pts.last().unwrap();
        assert!((last[0] - 70.0).abs() < 1e-9 && (last[1] - 3.0).abs() < 1e-9, "the swing ends at the strike: {last:?}");
        // The other side, the other hinge.
        let other = place(&ax, &OpeningSpec { side: -1.0, ..spec.clone() });
        assert_eq!(other.normal, [0.0, -1.0]);
        assert_eq!(outline(OpeningKind::Door, &at, true)[0], [70.0, 3.0]);
    }
}
