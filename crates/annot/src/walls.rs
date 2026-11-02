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
    /// The `/NM` of the wall each joined end is joined to ("" when not known).
    pub ja_with: String,
    pub jb_with: String,
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
    Some(WallData { ja: flag(w, b"JA"), jb: flag(w, b"JB"), ja_with: text(w, b"JAWith"), jb_with: text(w, b"JBWith"), gaps, open })
}

fn data_dict(data: &WallData) -> Dict {
    let mut w = Dict::new();
    w.set(b"JA".to_vec(), Object::Bool(data.ja));
    w.set(b"JB".to_vec(), Object::Bool(data.jb));
    if !data.ja_with.is_empty() {
        w.set(b"JAWith".to_vec(), PdfString::text(&data.ja_with));
    }
    if !data.jb_with.is_empty() {
        w.set(b"JBWith".to_vec(), PdfString::text(&data.jb_with));
    }
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
fn mark_at(kind: OpeningKind, at: &Place, text_size: Option<f64>) -> (Pt, f64) {
    let width = sub(at.to, at.from)[0].hypot(sub(at.to, at.from)[1]);
    let size = text_size.unwrap_or((width * 0.22).clamp(4.0, 14.0));
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
        let (p, size) = mark_at(spec.kind, &at, crate::text_size(d));
        let w = crate::appearance::text_width(&spec.mark, size);
        let mark: String = spec.mark.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-').collect();
        c.push_str(&format!("{}BT /Helv {} Tf {} {} Td ({mark}) Tj ET\n", rg(col, "rg"), n(size), n(p[0] - w / 2.0), n(p[1] - size * 0.35)));
    }
    Some((c, label))
}

/// A door's or window's `/Rect`: its outline and its mark, with room for its lines.
fn opening_rect(spec: &OpeningSpec, at: &Place, text_size: Option<f64>) -> [f64; 4] {
    let mut pts = outline(spec.kind, at, spec.flip);
    if !spec.mark.is_empty() {
        let (p, size) = mark_at(spec.kind, at, text_size);
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
    with: (Option<String>, Option<String>),
    meta: &Meta,
) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    let d = annot_dict(doc, r);
    let mut data = wall_data(&d).ok_or_else(|| AnnotError::Invalid("that isn't a smart wall".into()))?;
    if let Some(v) = with.0 {
        data.ja_with = v;
    }
    if let Some(v) = with.1 {
        data.jb_with = v;
    }
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

/// The shortest piece a cut may leave, in points.
const MIN_PIECE: f64 = 2.0;

/// Cuts the plain smart wall `index` across its thickness at `t` (0 to 1 along its centre line)
/// into two walls; returns the index of the new piece, which is named `new_nm`. The first piece
/// keeps `index` and its name, open at the cut. The second keeps the far end and its joint, and
/// the wall it was joined to now names it. A wall with a gap or an opening is refused.
pub fn split(doc: &mut Document, page: usize, index: usize, t: f64, new_nm: &str, meta: &Meta) -> Result<usize, AnnotError> {
    let all = walls(doc, page);
    let w = all.iter().find(|w| w.index == index).ok_or_else(|| AnnotError::Invalid("that isn't a smart wall".into()))?;
    if !w.data.gaps.is_empty() {
        return Err(AnnotError::Invalid("a wall with a gap in it can't be cut yet".into()));
    }
    if !w.data.open.is_empty() {
        return Err(AnnotError::Invalid("a wall with a door or window in it can't be cut yet".into()));
    }
    if new_nm.is_empty() {
        return Err(AnnotError::Invalid("the new piece needs a name".into()));
    }
    let ax = axis(&w.quad).ok_or_else(|| AnnotError::Invalid("a wall needs a length and a thickness".into()))?;
    if !t.is_finite() || t <= 0.0 || t >= 1.0 || t * ax.len < MIN_PIECE || (1.0 - t) * ax.len < MIN_PIECE {
        return Err(AnnotError::Invalid("cut the wall inside its length, leaving pieces at least 2 points long".into()));
    }
    let [c0, c1, c2, c3] = w.quad;
    // Where the cut crosses each face: along face 0 (corners 0→1) and face 1 (corners 3→2).
    let p0 = add(c0, mul(sub(c1, c0), t));
    let p1 = add(c3, mul(sub(c2, c3), t));
    let partner = all.iter().find(|n| n.index != index && n.nm == w.data.jb_with && w.data.jb).cloned();
    let (kept, far) = ([c0, p0, p1, c3], [p0, c1, c2, p1]);

    // The far piece is a copy of the wall, so it keeps the style and layer; its joint at the cut is
    // open, and its far joint is the original one.
    let far_meta = Meta { date: meta.date.clone(), id: new_nm.to_string() };
    let new_index = crate::geometry::set_vertices(doc, page, index, &far, true, &far_meta)?;
    set_wall(doc, page, new_index, None, Some(false), None, None, (Some(String::new()), None), meta)?;
    set_wall(doc, page, index, Some(kept), None, Some(false), None, (None, Some(String::new())), meta)?;
    if let Some(p) = partner
        && let Some(pe) = joint_end(&p, &w.nm)
    {
        let (ja_with, jb_with) = if pe == 0 { (Some(new_nm.to_string()), None) } else { (None, Some(new_nm.to_string())) };
        set_wall(doc, page, p.index, None, None, None, None, (ja_with, jb_with), meta)?;
    }
    Ok(new_index)
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
    if !fits(doc, page, &w, t0, t1, None) {
        return Err(AnnotError::Invalid("there's no room for a door or window there".into()));
    }
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

/// The usual size of a door's or window's mark (points), from its width.
pub(crate) fn default_mark_size(d: &Dict) -> Option<f64> {
    let o = d.get(OPENING)?.as_dict()?;
    let pt = |k: &[u8]| -> Option<Pt> {
        match numbers(o, k).as_slice() {
            [x, y] => Some([*x, *y]),
            _ => None,
        }
    };
    let (from, to) = (pt(b"From")?, pt(b"To")?);
    let width = sub(to, from)[0].hypot(sub(to, from)[1]);
    width.is_finite().then(|| (width * 0.22).clamp(4.0, 14.0))
}

/// The index of the wall that hosts the door or window `d` on `page`.
pub(crate) fn host_wall(doc: &Document, page: usize, d: &Dict) -> Option<usize> {
    let host = d.get(OPENING)?.as_dict()?.get(b"Host").and_then(Object::as_string).map(PdfString::to_text)?;
    walls(doc, page).into_iter().find(|w| !w.nm.is_empty() && w.nm == host).map(|w| w.index)
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
        let rect = opening_rect(&spec, &at, crate::text_size(&crate::annot_dict(doc, r)));
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

/// The parts of the wall a door or window can't take: the other doors and windows in it (all but
/// the one at `except`), where walls meet its faces, and its joined corners (as far in as the wall
/// is thick, where the mitre is).
fn taken(doc: &Document, page: usize, w: &Wall, except: Option<usize>) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = if w.nm.is_empty() {
        Vec::new()
    } else {
        openings(doc, page).into_iter().filter(|o| o.host == w.nm && Some(o.index) != except).map(|o| (o.spec.t0, o.spec.t1)).collect()
    };
    out.extend(w.data.gaps.iter().map(|g| (g.1, g.2)));
    if let Some(ax) = axis(&w.quad) {
        let corner = (ax.thickness() / ax.len).min(0.5);
        if w.data.ja {
            out.push((0.0, corner));
        }
        if w.data.jb {
            out.push((1.0 - corner, 1.0));
        }
    }
    out
}

/// Whether a door or window from `t0` to `t1` along the wall `w` has room there (`except`: the one
/// being moved, which doesn't stand in its own way).
pub fn fits(doc: &Document, page: usize, w: &Wall, t0: f64, t1: f64, except: Option<usize>) -> bool {
    let (a, b) = (t0.min(t1), t0.max(t1));
    a.is_finite()
        && b.is_finite()
        && a >= -1e-9
        && b <= 1.0 + 1e-9
        && taken(doc, page, w, except).iter().all(|(c0, c1)| b <= c0 + 1e-9 || a >= c1 - 1e-9)
}

/// How far past a joint's end (points) a joined wall may still be taken to meet it.
const JOINT_REACH: f64 = 2.0;
/// Walls this close to parallel don't have a joint.
const PARALLEL: f64 = 0.1;
/// Walls one move may reach through joints, at most.
const MAX_STRETCH: usize = 256;

fn meet(p: Pt, q: Pt, r: Pt, s: Pt) -> Option<Pt> {
    let (d1, d2) = (sub(q, p), sub(s, r));
    let den = d1[0] * d2[1] - d1[1] * d2[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let t = ((r[0] - p[0]) * d2[1] - (r[1] - p[1]) * d2[0]) / den;
    let x = add(p, mul(d1, t));
    finite(x).then_some(x)
}

/// The corners of the joint at `end` of wall `n` that a moved wall `w` (end `w_end`) meets: each
/// on one of `n`'s faces, and where it meets the nearer pairing of `w`'s faces (its moved faces).
/// `None` when the walls are parallel, or the joint has moved past the wall's end.
fn mitre(w: &[Pt; 4], w_end: usize, n: &[Pt; 4], n_end: usize) -> Option<[Pt; 4]> {
    let (wa, na) = (axis(w)?, axis(n)?);
    if (wa.along[0] * na.along[1] - wa.along[1] * na.along[0]).abs() < PARALLEL {
        return None;
    }
    let (k0, k1) = if n_end == 0 { (0, 3) } else { (1, 2) };
    let (wf0, wf1) = ((w[0], w[1]), (w[3], w[2]));
    let (nf0, nf1) = ((n[0], n[1]), (n[3], n[2]));
    let pairing = |a: (Pt, Pt), b: (Pt, Pt)| Some((meet(nf0.0, nf0.1, a.0, a.1)?, meet(nf1.0, nf1.1, b.0, b.1)?));
    let straight = pairing(wf0, wf1);
    let crossed = pairing(wf1, wf0);
    let cost = |p: Option<(Pt, Pt)>| p.map(|(x, y)| dist(x, n[k0]) + dist(y, n[k1]));
    let (x, y) = match (cost(straight), cost(crossed)) {
        (Some(a), Some(b)) if b < a => crossed?,
        (Some(_), _) => straight?,
        (None, Some(_)) => crossed?,
        (None, None) => return None,
    };
    let end_t = if w_end == 0 { 0.0 } else { 1.0 };
    let joint_mid = mul(add(x, y), 0.5);
    if (wa.t_of(joint_mid) - end_t).abs() * wa.len > wa.thickness() / 2.0 + JOINT_REACH {
        return None;
    }
    let mut out = *n;
    *out.get_mut(k0)? = x;
    *out.get_mut(k1)? = y;
    axis(&out)?;
    Some(out)
}

fn dist(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The end of `n` joined to the wall named `with`, if it is one.
fn joint_end(n: &Wall, with: &str) -> Option<usize> {
    if n.data.ja && n.data.ja_with == with {
        Some(0)
    } else if n.data.jb && n.data.jb_with == with {
        Some(1)
    } else {
        None
    }
}

/// Frees end `end` of wall `index` (and its end on `partner`, if it is still there).
fn release(doc: &mut Document, page: usize, index: usize, end: usize, partner: Option<usize>) -> Result<(), AnnotError> {
    let Some(w) = walls(doc, page).into_iter().find(|w| w.index == index) else { return Ok(()) };
    let mut data = w.data.clone();
    if end == 0 {
        data.ja = false;
        data.ja_with.clear();
    } else {
        data.jb = false;
        data.jb_with.clear();
    }
    let (_, r) = annot_ref(doc, page, index)?;
    doc.update_dict(r, |d| d.set(WALL.to_vec(), Object::Dict(data_dict(&data))))?;
    set_appearance(doc, r)?;
    if let Some(p) = partner.and_then(|p| walls(doc, page).into_iter().find(|n| n.index == p))
        && let Some(pe) = joint_end(&p, &w.nm)
    {
        let mut pd = p.data.clone();
        if pe == 0 {
            pd.ja = false;
            pd.ja_with.clear();
        } else {
            pd.jb = false;
            pd.jb_with.clear();
        }
        let (_, pr) = annot_ref(doc, page, p.index)?;
        doc.update_dict(pr, |d| d.set(WALL.to_vec(), Object::Dict(data_dict(&pd))))?;
        set_appearance(doc, pr)?;
    }
    Ok(())
}

/// Frees every joint that names the wall `gone` (just deleted): the walls it was joined to have an
/// open end again.
pub fn forget_partner(doc: &mut Document, page: usize, gone: &str) -> Result<(), AnnotError> {
    if gone.is_empty() {
        return Ok(());
    }
    for w in walls(doc, page) {
        if w.data.ja && w.data.ja_with == gone {
            release(doc, page, w.index, 0, None)?;
        }
        if w.data.jb && w.data.jb_with == gone {
            release(doc, page, w.index, 1, None)?;
        }
    }
    Ok(())
}

/// After wall `moved` changed shape: the walls joined to it follow its faces (each joint's
/// corners on their outline move to meet them), and a joint is released when the wall has moved
/// past its partner or the partner is gone. Walls reached through joints follow in turn.
pub fn stretch(doc: &mut Document, page: usize, moved: usize, meta: &Meta) -> Result<(), AnnotError> {
    let mut queue = vec![moved];
    let mut seen = vec![moved];
    while let Some(i) = queue.pop() {
        let Some(w) = walls(doc, page).into_iter().find(|w| w.index == i) else { continue };
        for (end, joined, with) in [(0usize, w.data.ja, w.data.ja_with.clone()), (1, w.data.jb, w.data.jb_with.clone())] {
            if !joined {
                continue;
            }
            let partner = walls(doc, page).into_iter().find(|n| n.index != w.index && !with.is_empty() && n.nm == with);
            let moved_quad = partner.as_ref().filter(|n| !n.locked).and_then(|n| {
                let ne = joint_end(n, &w.nm)?;
                mitre(&w.quad, end, &n.quad, ne).map(|q| (n.index, q))
            });
            match moved_quad {
                Some((n, q)) => {
                    crate::geometry::set_vertices(doc, page, n, &q, false, meta)?;
                    sync(doc, page, n, meta)?;
                    if seen.len() < MAX_STRETCH && !seen.contains(&n) {
                        seen.push(n);
                        queue.push(n);
                    }
                }
                None => release(doc, page, w.index, end, partner.map(|p| p.index))?,
            }
        }
    }
    Ok(())
}

/// After the markup at `index` moved by `delta`: a wall's doors and windows follow it; a door or
/// window slides along its wall by as much of the move as runs along it.
pub fn after_move(doc: &mut Document, page: usize, index: usize, delta: Option<Pt>, meta: &Meta) -> Result<(), AnnotError> {
    if walls(doc, page).iter().any(|w| w.index == index) {
        sync(doc, page, index, meta)?;
        return stretch(doc, page, index, meta);
    }
    let Some(o) = openings(doc, page).into_iter().find(|o| o.index == index) else { return Ok(()) };
    let Some(w) = walls(doc, page).into_iter().find(|w| w.nm == o.host) else { return Ok(()) };
    if let (Some(d), Some(ax)) = (delta.filter(|d| finite(*d)), axis(&w.quad)) {
        let width = o.spec.t1 - o.spec.t0;
        // It slides as far as the room it is in: up to the next door or window, a wall meeting
        // this one, or a joined corner.
        let room = minus((0.0, 1.0), &taken(doc, page, &w, Some(index)))
            .into_iter()
            .find(|(a, b)| *a <= o.spec.t0 + 1e-9 && *b >= o.spec.t1 - 1e-9)
            .unwrap_or((0.0, 1.0));
        let t0 = (o.spec.t0 + dot(d, ax.along) / ax.len).clamp(room.0, (room.1 - width).max(room.0));
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
    fn a_joint_remembers_the_wall_it_is_joined_to() {
        let data = WallData { ja: true, ja_with: "w-north".into(), jb: true, jb_with: "w-east".into(), ..Default::default() };
        let mut d = Dict::new();
        d.set(WALL.to_vec(), Object::Dict(data_dict(&data)));
        assert_eq!(wall_data(&d), Some(data));
        // Nothing recorded: the ends are joined as before, to nothing in particular.
        let mut bare = Dict::new();
        bare.set(WALL.to_vec(), Object::Dict(data_dict(&WallData { ja: true, ..Default::default() })));
        assert_eq!(wall_data(&bare).map(|w| w.ja_with), Some(String::new()));
    }

    fn blank() -> Document {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        Document::open(std::sync::Arc::new(pdf.to_vec())).unwrap()
    }

    fn near(a: Pt, b: Pt) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6
    }

    fn meta(id: &str) -> Meta {
        Meta { date: None, id: id.into() }
    }

    /// An L as the Wall tool joins it: W along x from 0 to 100 (its end B corners at 97 and 103),
    /// and N up from its joint, each naming the other there.
    fn joined_l(doc: &mut Document) -> usize {
        let style = Style { color: [0.0; 3], opacity: 1.0, width: 0.75, fill: None };
        let w_data = WallData { jb: true, jb_with: "n".into(), ..Default::default() };
        add_wall(doc, 0, [[0.0, 3.0], [97.0, 3.0], [103.0, -3.0], [0.0, -3.0]], &w_data, &style, "A", &meta("w")).unwrap();
        let n_data = WallData { ja: true, ja_with: "w".into(), ..Default::default() };
        add_wall(doc, 0, [[97.0, 3.0], [97.0, 80.0], [103.0, 80.0], [103.0, -3.0]], &n_data, &style, "A", &meta("n")).unwrap();
        0
    }

    fn wall_named(doc: &Document, nm: &str) -> Wall {
        walls(doc, 0).into_iter().find(|w| w.nm == nm).unwrap()
    }

    #[test]
    fn a_joined_wall_follows_when_the_wall_it_meets_moves() {
        let mut doc = blank();
        joined_l(&mut doc);
        // Move W up 20: its faces move with it, and N's end moves up to meet its lower face.
        let w = wall_named(&doc, "w");
        let up: Vec<[f64; 2]> = w.quad.iter().map(|p| [p[0], p[1] + 20.0]).collect();
        crate::geometry::set_vertices(&mut doc, 0, w.index, &up, false, &meta("m")).unwrap();
        after_move(&mut doc, 0, w.index, None, &meta("m")).unwrap();
        let n = wall_named(&doc, "n");
        assert!(near(n.quad[0], [97.0, 23.0]) && near(n.quad[3], [103.0, 17.0]), "{:?}", n.quad);
        assert!(wall_named(&doc, "w").data.jb, "still joined");
        assert!(n.data.ja, "still joined");
    }

    #[test]
    fn a_joint_is_released_when_its_wall_moves_past_the_other() {
        let mut doc = blank();
        joined_l(&mut doc);
        // W's end B runs on past N, along its own length.
        let w = wall_named(&doc, "w");
        let long = [w.quad[0], [127.0, 3.0], [133.0, -3.0], w.quad[3]];
        crate::geometry::set_vertices(&mut doc, 0, w.index, &long, false, &meta("m")).unwrap();
        after_move(&mut doc, 0, w.index, None, &meta("m")).unwrap();
        let (w, n) = (wall_named(&doc, "w"), wall_named(&doc, "n"));
        assert!(!w.data.jb && w.data.jb_with.is_empty(), "W's end is free again");
        assert!(!n.data.ja && n.data.ja_with.is_empty(), "N's end is free again");
        assert!(near(n.quad[0], [97.0, 3.0]) && near(n.quad[3], [103.0, -3.0]), "N stays where it was");
    }

    #[test]
    fn a_joint_whose_wall_is_gone_is_released() {
        let mut doc = blank();
        let style = Style { color: [0.0; 3], opacity: 1.0, width: 0.75, fill: None };
        let data = WallData { jb: true, jb_with: "gone".into(), ..Default::default() };
        add_wall(&mut doc, 0, [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]], &data, &style, "A", &meta("w")).unwrap();
        let w = wall_named(&doc, "w");
        let moved: Vec<[f64; 2]> = w.quad.iter().map(|p| [p[0], p[1] + 5.0]).collect();
        crate::geometry::set_vertices(&mut doc, 0, w.index, &moved, false, &meta("m")).unwrap();
        after_move(&mut doc, 0, w.index, None, &meta("m")).unwrap();
        assert!(!wall_named(&doc, "w").data.jb);
    }

    #[test]
    fn a_free_wall_is_not_touched() {
        let mut doc = blank();
        let style = Style { color: [0.0; 3], opacity: 1.0, width: 0.75, fill: None };
        add_wall(&mut doc, 0, [[0.0, 3.0], [100.0, 3.0], [100.0, -3.0], [0.0, -3.0]], &WallData::default(), &style, "A", &meta("w")).unwrap();
        let w = wall_named(&doc, "w");
        let moved: Vec<[f64; 2]> = w.quad.iter().map(|p| [p[0], p[1] + 5.0]).collect();
        crate::geometry::set_vertices(&mut doc, 0, w.index, &moved, false, &meta("m")).unwrap();
        after_move(&mut doc, 0, w.index, None, &meta("m")).unwrap();
        assert_eq!(wall_named(&doc, "w").quad[0], [0.0, 8.0]);
    }

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
