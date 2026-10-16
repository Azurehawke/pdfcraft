//! RevuCraft's drafting edits on markup geometry: the points of lines, polylines and polygons
//! read back for grips, snapping, trimming and offsets; their vertices replaced; and markup
//! moved, copied and rotated by an affine matrix. Changed shapes get a new `/Rect` and appearance.

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString};

use crate::{AnnotError, Meta, annot_dict, annot_ref, annots, bounds, finite, num_array, page_ref, set_annots, set_appearance, touch, unlocked};

/// The most vertices a drafting edit writes.
pub const MAX_VERTICES: usize = 4096;
/// The most shapes read from one page.
const MAX_SHAPES: usize = 20_000;

/// The kind of shape a markup's points describe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A line (`/L`, two points).
    Line,
    /// Connected lines (`/Vertices`), open.
    PolyLine,
    /// A closed shape (`/Vertices`).
    Polygon,
}

/// One markup's points, in user space.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    /// The annotation's index on its page.
    pub index: usize,
    pub kind: Kind,
    pub points: Vec<[f64; 2]>,
    pub locked: bool,
}

impl Geometry {
    /// The segments, including the closing one of a polygon.
    pub fn segments(&self) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
        let n = self.points.len();
        let closing = (self.kind == Kind::Polygon && n >= 3).then(|| self.points.last().copied().zip(self.points.first().copied())).flatten();
        self.points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(closing)
    }
}

fn numbers(doc: &Document, d: &Dict, key: &[u8]) -> Vec<f64> {
    d.get(key)
        .map(|o| doc.resolve(o))
        .and_then(|o| o.as_array().map(|a| a.iter().filter_map(|x| doc.resolve(x).as_f64()).filter(|v| v.is_finite()).collect()))
        .unwrap_or_default()
}

fn points_of(v: &[f64]) -> Vec<[f64; 2]> {
    v.as_chunks::<2>().0.to_vec()
}

fn is_locked(doc: &Document, d: &Dict) -> bool {
    d.get(b"F").and_then(|f| doc.resolve(f).as_int()).unwrap_or(0) & 128 != 0
}

/// The lines, polylines and polygons on `page` (replies and pop-ups have none).
pub fn page_geometry(doc: &Document, page: usize) -> Vec<Geometry> {
    let mut out = Vec::new();
    let Ok(p) = page_ref(doc, page) else { return out };
    for (index, entry) in annots(doc, p).iter().enumerate() {
        if out.len() >= MAX_SHAPES {
            break;
        }
        let obj = doc.resolve(entry);
        let Some(d) = obj.as_dict() else { continue };
        let (kind, key): (Kind, &[u8]) = match d.name(b"Subtype") {
            Some(b"Line") => (Kind::Line, b"L"),
            Some(b"PolyLine") => (Kind::PolyLine, b"Vertices"),
            Some(b"Polygon") => (Kind::Polygon, b"Vertices"),
            _ => continue,
        };
        let mut points = points_of(&numbers(doc, d, key));
        if kind == Kind::Line {
            points.truncate(2);
        }
        if points.len() >= 2 {
            out.push(Geometry { index, kind, points, locked: is_locked(doc, d) });
        }
    }
    out
}

/// The room `/Rect` leaves around the points (line width, arrowheads, a cloud or a caption), so a
/// changed shape keeps it.
fn margin(d: &Dict, old: &[[f64; 2]]) -> f64 {
    let rect = d.get(b"Rect").and_then(|o| o.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).collect::<Vec<_>>()).unwrap_or_default();
    match (rect.as_slice(), bounds(old.iter().copied())) {
        ([x0, y0, x1, y1], Some(b)) => {
            let (x0, x1) = (x0.min(*x1), x0.max(*x1));
            let (y0, y1) = (y0.min(*y1), y0.max(*y1));
            let m = (b[0] - x0).max(b[1] - y0).max(x1 - b[2]).max(y1 - b[3]);
            if m.is_finite() { m.clamp(1.0, 200.0) } else { 4.0 }
        }
        _ => 4.0,
    }
}

fn check(points: &[[f64; 2]]) -> Result<(), AnnotError> {
    if points.len() > MAX_VERTICES {
        return Err(AnnotError::Invalid("too many points".into()));
    }
    if !points.iter().all(|p| finite(p)) {
        return Err(AnnotError::Invalid("invalid point".into()));
    }
    Ok(())
}

/// Write `points` into the dictionary of a line, polyline or polygon, with a `/Rect` that keeps
/// the old margin.
fn write_points(d: &mut Dict, points: &[[f64; 2]], old: &[[f64; 2]], meta: &Meta) -> Result<(), AnnotError> {
    let m = margin(d, old);
    let b = bounds(points.iter().copied()).ok_or_else(|| AnnotError::Invalid("no points".into()))?;
    if (b[2] - b[0]).max(b[3] - b[1]) < 1.0 {
        return Err(AnnotError::Invalid("the shape would be too small".into()));
    }
    // A line given more points becomes a polyline (join, a fillet's arc); a polygon given fewer
    // than three, a two-point polyline (explode).
    let subtype = d.name(b"Subtype").map(<[u8]>::to_vec);
    match (subtype.as_deref(), points.len()) {
        (Some(b"Line"), 2) => {
            let (a, b) = (points.first().copied().unwrap_or_default(), points.get(1).copied().unwrap_or_default());
            d.set(b"L".to_vec(), num_array(&[a[0], a[1], b[0], b[1]]));
        }
        (Some(b"Line"), n) if n > 2 => {
            d.remove(b"L");
            d.set(b"Subtype".to_vec(), Object::name("PolyLine"));
            d.set(b"Vertices".to_vec(), num_array(&points.concat()));
        }
        (Some(b"PolyLine"), n) if n >= 2 => d.set(b"Vertices".to_vec(), num_array(&points.concat())),
        (Some(b"Polygon"), n) if n >= 3 => d.set(b"Vertices".to_vec(), num_array(&points.concat())),
        (Some(b"Polygon"), 2) => {
            d.set(b"Subtype".to_vec(), Object::name("PolyLine"));
            for k in [&b"IC"[..], b"BE", b"IT", b"Measure"] {
                d.remove(k);
            }
            d.set(b"Vertices".to_vec(), num_array(&points.concat()));
        }
        _ => return Err(AnnotError::Invalid("not enough points for this shape".into())),
    }
    d.set(b"Rect".to_vec(), num_array(&[b[0] - m, b[1] - m, b[2] + m, b[3] + m]));
    fit_dimension(d);
    touch(d, meta);
    Ok(())
}

/// A dimension line's `/Rect`: its extension lines, dimension line and text.
fn fit_dimension(d: &mut Dict) {
    let Some(ll) = d.get(b"LL").and_then(Object::as_f64).filter(|v| v.is_finite() && v.abs() > 1e-9) else { return };
    let nums = |k: &[u8]| -> Vec<f64> {
        d.get(k).and_then(|o| o.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).filter(|v| v.is_finite()).collect()).unwrap_or_default()
    };
    let l = nums(b"L");
    let [x1, y1, x2, y2] = l[..] else { return };
    let len = (x2 - x1).hypot(y2 - y1);
    if len < 1e-9 {
        return;
    }
    let ext = d.get(b"LLE").and_then(Object::as_f64).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0);
    let reach = ll + ll.signum() * ext;
    let (nx, ny) = ((y2 - y1) / len, -(x2 - x1) / len);
    let mut pts = vec![[x1, y1], [x2, y2], [x1 + nx * reach, y1 + ny * reach], [x2 + nx * reach, y2 + ny * reach]];
    pts.extend(points_of(&nums(b"PCDimPoints")));
    let text = d.get(b"PCMeasureValue").and_then(|o| o.as_string()).map(|t| t.to_text().chars().count()).unwrap_or(0);
    let pad = (text as f64 * 3.2 + 6.0).max(16.0);
    if let Some(b) = bounds(pts.into_iter()) {
        d.set(b"Rect".to_vec(), num_array(&[b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]));
    }
}

/// A copy of the annotation `r` on page `p`, added at the end of the page's list, without its
/// pop-up and with a new name. Returns its index and reference.
fn duplicate(doc: &mut Document, p: ObjRef, r: ObjRef, meta: &Meta) -> Result<(usize, ObjRef), AnnotError> {
    let mut d = annot_dict(doc, r);
    if d.name(b"Subtype") == Some(b"Popup") || d.get(b"IRT").is_some() {
        return Err(AnnotError::Invalid("replies and pop-ups can't be copied".into()));
    }
    d.remove(b"Popup");
    d.set(b"NM".to_vec(), PdfString::text(&meta.id));
    d.set(b"P".to_vec(), Object::Ref(p));
    touch(&mut d, meta);
    let copy = doc.add(Object::Dict(d));
    let mut list = annots(doc, p);
    list.push(Object::Ref(copy));
    let index = list.len().saturating_sub(1);
    set_annots(doc, p, list)?;
    Ok((index, copy))
}

/// Replace the points of the line, polyline or polygon at `(page, index)`, or of a copy of it
/// when `copy` is set. Returns the index of the shape changed.
pub fn set_vertices(doc: &mut Document, page: usize, index: usize, points: &[[f64; 2]], copy: bool, meta: &Meta) -> Result<usize, AnnotError> {
    check(points)?;
    let (p, r) = annot_ref(doc, page, index)?;
    if !copy {
        unlocked(doc, r)?;
    }
    let d = annot_dict(doc, r);
    let key: &[u8] = match d.name(b"Subtype") {
        Some(b"Line") => b"L",
        Some(b"PolyLine" | b"Polygon") => b"Vertices",
        _ => return Err(AnnotError::Invalid("only lines and shapes made of lines have points to change".into())),
    };
    let old = points_of(&numbers(doc, &d, key));
    let mut changed = d;
    write_points(&mut changed, points, &old, meta)?;
    // A dimension's extension lines then start at its (new) ends.
    changed.remove(b"PCDimPoints");
    let (index, target) = if copy { duplicate(doc, p, r, meta)? } else { (index, r) };
    if copy {
        changed.remove(b"Popup");
        changed.set(b"NM".to_vec(), PdfString::text(&meta.id));
    }
    doc.update_dict(target, |t| *t = changed)?;
    set_appearance(doc, target)?;
    Ok(index)
}

/// Apply `m` (`[a b c d e f]`, as PDF writes matrices) to a point.
pub fn apply(m: &[f64; 6], p: [f64; 2]) -> [f64; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

/// The matrix that turns points by `degrees` (counter-clockwise) about `center`.
pub fn rotation(center: [f64; 2], degrees: f64) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    [c, s, -s, c, center[0] - c * center[0] + s * center[1], center[1] - s * center[0] - c * center[1]]
}

/// Move, turn (or both) the markup at `(page, index)` by `m`, or a copy of it when `copy` is set.
/// Lines, polylines, polygons and drawings have their points transformed; other markup (boxes,
/// notes, stamps and symbols) keeps its shape and upright look, and is moved to where its centre
/// goes. Returns the index of the markup changed.
pub fn transform(doc: &mut Document, page: usize, index: usize, m: &[f64; 6], copy: bool, meta: &Meta) -> Result<usize, AnnotError> {
    if !finite(m) {
        return Err(AnnotError::Invalid("invalid transformation".into()));
    }
    let (p, r) = annot_ref(doc, page, index)?;
    if !copy {
        unlocked(doc, r)?;
    }
    let (index, target) = if copy { duplicate(doc, p, r, meta)? } else { (index, r) };
    let d = annot_dict(doc, target);
    let key: Option<&[u8]> = match d.name(b"Subtype") {
        Some(b"Line") => Some(b"L"),
        Some(b"PolyLine" | b"Polygon") => Some(b"Vertices"),
        _ => None,
    };
    if let Some(key) = key {
        let old = points_of(&numbers(doc, &d, key));
        let new: Vec<[f64; 2]> = old.iter().map(|q| apply(m, *q)).collect();
        check(&new)?;
        let dim_points: Vec<[f64; 2]> = points_of(&numbers(doc, &d, b"PCDimPoints")).iter().map(|q| apply(m, *q)).collect();
        check(&dim_points)?;
        let mut changed = d;
        write_points(&mut changed, &new, &old, meta)?;
        if dim_points.len() == 2 {
            changed.set(b"PCDimPoints".to_vec(), num_array(&dim_points.concat()));
        }
        // The dimension line stays as far off in proportion, and on its side after a mirror
        // (which turns clockwise into anticlockwise).
        let det = m[0] * m[3] - m[1] * m[2];
        if let Some(ll) = changed.get(b"LL").and_then(Object::as_f64)
            && det.is_finite()
        {
            let scaled = ll * det.abs().sqrt() * det.signum();
            if scaled.is_finite() {
                changed.set(b"LL".to_vec(), Object::Real(scaled));
                fit_dimension(&mut changed);
            }
        }
        doc.update_dict(target, |t| *t = changed)?;
        set_appearance(doc, target)?;
        return Ok(index);
    }
    if d.name(b"Subtype") == Some(b"Ink") {
        let strokes: Vec<Vec<[f64; 2]>> = match d.get(b"InkList").map(|o| doc.resolve(o)).as_deref() {
            Some(Object::Array(list)) => list
                .iter()
                .map(|s| doc.resolve(s).as_array().map(|a| points_of(&a.iter().filter_map(|x| x.as_f64()).collect::<Vec<_>>())).unwrap_or_default())
                .collect(),
            _ => Vec::new(),
        };
        let all: Vec<[f64; 2]> = strokes.iter().flatten().copied().collect();
        let moved: Vec<Vec<[f64; 2]>> = strokes.iter().map(|s| s.iter().map(|q| apply(m, *q)).collect()).collect();
        let flat: Vec<[f64; 2]> = moved.iter().flatten().copied().collect();
        check(&flat)?;
        if let Some(b) = bounds(flat.iter().copied()) {
            let mut changed = d;
            let margin = margin(&changed, &all);
            changed.set(b"InkList".to_vec(), Object::Array(moved.iter().map(|s| num_array(&s.concat())).collect()));
            changed.set(b"Rect".to_vec(), num_array(&[b[0] - margin, b[1] - margin, b[2] + margin, b[3] + margin]));
            touch(&mut changed, meta);
            doc.update_dict(target, |t| *t = changed)?;
            set_appearance(doc, target)?;
        }
        return Ok(index);
    }
    let rect = numbers(doc, &d, b"Rect");
    let [x0, y0, x1, y1] = rect[..] else { return Err(AnnotError::Invalid("the markup has no rectangle".into())) };
    // Rectangles and ovals scale, mirror and turn by quarter turns with the matrix (they stay
    // upright, so other turns only move them).
    let upright = (m[1].abs() < 1e-9 && m[2].abs() < 1e-9) || (m[0].abs() < 1e-9 && m[3].abs() < 1e-9);
    let moves_only = (m[0] - 1.0).abs() < 1e-12 && m[1] == 0.0 && m[2] == 0.0 && (m[3] - 1.0).abs() < 1e-12;
    if upright && !moves_only && matches!(d.name(b"Subtype"), Some(b"Square" | b"Circle")) {
        let a = apply(m, [x0, y0]);
        let b = apply(m, [x1, y1]);
        crate::set_rect(doc, page, index, [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])], meta)?;
        return Ok(index);
    }
    // Everything else moves with its centre.
    let c = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
    let to = apply(m, c);
    crate::move_annotation(doc, page, index, to[0] - c[0], to[1] - c[1], meta)?;
    Ok(index)
}

/// Whether the annotation at `(page, index)` exists and is not locked (for tests and callers that
/// check before offering an edit).
pub fn editable(doc: &Document, page: usize, index: usize) -> bool {
    let Ok(p) = page_ref(doc, page) else { return false };
    annots(doc, p).get(index).map(|e| doc.resolve(e)).and_then(|o| o.as_dict().map(|d| !is_locked(doc, d))).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LineEnding, NewAnnotation, Shape, Style, add_annotation};

    fn doc() -> Document {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        Document::open(std::sync::Arc::new(pdf.to_vec())).unwrap()
    }

    fn meta() -> Meta {
        Meta { date: None, id: "test".into() }
    }

    fn add(doc: &mut Document, shape: Shape) -> usize {
        let style = Style { color: [0.0; 3], opacity: 1.0, width: 1.0, fill: None };
        add_annotation(doc, &NewAnnotation { page: 0, shape, style, contents: String::new(), author: "t".into() }, &meta()).unwrap()
    }

    #[test]
    fn vertices_are_replaced_and_copied() {
        let mut d = doc();
        let i = add(
            &mut d,
            Shape::PolyLine { vertices: vec![[10.0, 10.0], [100.0, 10.0], [100.0, 80.0]], start: LineEnding::None, end: LineEnding::None },
        );
        set_vertices(&mut d, 0, i, &[[10.0, 10.0], [200.0, 10.0], [200.0, 80.0]], false, &meta()).unwrap();
        let g = page_geometry(&d, 0);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].points[1], [200.0, 10.0]);
        let j = set_vertices(&mut d, 0, i, &[[0.0, 0.0], [50.0, 0.0]], true, &meta()).unwrap();
        assert_eq!(j, 1);
        let g = page_geometry(&d, 0);
        assert_eq!(g.len(), 2);
        assert_eq!(g[1].points, vec![[0.0, 0.0], [50.0, 0.0]]);
        // A line keeps two points.
        let k = add(&mut d, Shape::Line { from: [0.0, 0.0], to: [50.0, 50.0], start: LineEnding::None, end: LineEnding::None });
        assert!(set_vertices(&mut d, 0, k, &[[0.0, 0.0]], false, &meta()).is_err(), "one point is not a line");
        assert!(set_vertices(&mut d, 0, k, &[[0.0, 0.0], [f64::NAN, 1.0]], false, &meta()).is_err());
        assert!(set_vertices(&mut d, 0, 99, &[[0.0, 0.0], [10.0, 1.0]], false, &meta()).is_err());
    }

    #[test]
    fn rotate_and_copy_lines() {
        let mut d = doc();
        let i = add(&mut d, Shape::Line { from: [100.0, 100.0], to: [200.0, 100.0], start: LineEnding::None, end: LineEnding::None });
        transform(&mut d, 0, i, &rotation([100.0, 100.0], 90.0), false, &meta()).unwrap();
        let g = page_geometry(&d, 0);
        let b = g[0].points[1];
        assert!((b[0] - 100.0).abs() < 1e-9 && (b[1] - 200.0).abs() < 1e-9, "{b:?}");
        let j = transform(&mut d, 0, i, &[1.0, 0.0, 0.0, 1.0, 10.0, 0.0], true, &meta()).unwrap();
        let g = page_geometry(&d, 0);
        assert_eq!(g.len(), 2);
        assert_eq!(g[1].index, j);
        assert!((g[1].points[0][0] - 110.0).abs() < 1e-9);
        assert!(transform(&mut d, 0, i, &[f64::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0], false, &meta()).is_err());
    }

    #[test]
    fn shapes_change_kind_with_their_points() {
        let mut d = doc();
        let i = add(&mut d, Shape::Line { from: [0.0, 0.0], to: [50.0, 0.0], start: LineEnding::None, end: LineEnding::None });
        set_vertices(&mut d, 0, i, &[[0.0, 0.0], [50.0, 0.0], [50.0, 40.0]], false, &meta()).unwrap();
        let g = page_geometry(&d, 0);
        assert_eq!(g[0].kind, Kind::PolyLine);
        let j = add(&mut d, Shape::Polygon { vertices: vec![[0.0, 0.0], [50.0, 0.0], [50.0, 40.0]], cloud: false });
        let k = set_vertices(&mut d, 0, j, &[[0.0, 0.0], [50.0, 0.0]], true, &meta()).unwrap();
        let g = page_geometry(&d, 0);
        assert_eq!(g.iter().find(|g| g.index == k).map(|g| g.kind), Some(Kind::PolyLine));
        // A rectangle scales with the matrix.
        let r = add(&mut d, Shape::Rectangle { rect: [100.0, 100.0, 120.0, 110.0] });
        transform(&mut d, 0, r, &[2.0, 0.0, 0.0, 2.0, -100.0, -100.0], false, &meta()).unwrap();
        let p = page_ref(&d, 0).unwrap();
        let entry = annots(&d, p)[r].clone();
        let rect = numbers(&d, d.resolve(&entry).as_dict().unwrap(), b"Rect");
        assert!((rect[2] - rect[0] - 40.0).abs() < 1e-6, "{rect:?}");
    }

    #[test]
    fn boxes_move_with_their_centre() {
        let mut d = doc();
        let i = add(&mut d, Shape::Rectangle { rect: [100.0, 100.0, 120.0, 120.0] });
        transform(&mut d, 0, i, &rotation([100.0, 110.0], 180.0), false, &meta()).unwrap();
        let p = page_ref(&d, 0).unwrap();
        let entry = annots(&d, p)[i].clone();
        let r = numbers(&d, d.resolve(&entry).as_dict().unwrap(), b"Rect");
        let cx = (r[0] + r[2]) / 2.0;
        assert!((cx - 90.0).abs() < 1e-6, "{r:?}");
    }
}
