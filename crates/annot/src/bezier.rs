//! RevuCraft: Bézier curves drawn with the pen tool (as VectorCraft's pen and Illustrator's Pen
//! tool): anchor points, each with a handle before and after it (equal to the anchor at a corner).
//!
//! A curve is an Ink annotation, so other viewers show it: `/InkList` holds the curve flattened
//! into short lines, the appearance draws the real curves, and `/PCPath` keeps the anchors and
//! handles (six numbers a point: anchor, handle before, handle after) with `/PCPathClosed` when the
//! curve closes on itself.

use pdfcraft_cos::{Dict, Document, Object, PdfString};

use crate::{
    AnnotError, Meta, NewAnnotation, Shape, Style, add_annotation, annot_dict, annot_ref, bounds, finite, grow, n, num_array, set_appearance, touch,
};

pub const PATH: &[u8] = b"PCPath";
pub const CLOSED: &[u8] = b"PCPathClosed";
/// Points kept on one curve, at most.
pub const MAX_NODES: usize = 4096;
/// Lines each curve segment is flattened into for `/InkList`.
const STEPS: usize = 16;
/// The largest coordinate taken (points).
const MAX_COORD: f64 = 1e7;

type Pt = [f64; 2];

/// One point of a curve: where it is, and its handles (before and after it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub at: Pt,
    pub before: Pt,
    pub after: Pt,
}

impl Node {
    /// A corner: both handles on the point.
    pub fn corner(at: Pt) -> Self {
        Node { at, before: at, after: at }
    }

    /// Whether both handles sit on the point.
    pub fn is_corner(&self) -> bool {
        near(self.before, self.at) && near(self.after, self.at)
    }

    /// The point with its handles moved by `d` along with it.
    pub fn moved(&self, d: Pt) -> Self {
        let m = |p: Pt| [p[0] + d[0], p[1] + d[1]];
        Node { at: m(self.at), before: m(self.before), after: m(self.after) }
    }

    /// A smooth point whose handle after it is at `after`, the one before mirrored.
    pub fn smooth(at: Pt, after: Pt) -> Self {
        Node { at, before: [2.0 * at[0] - after[0], 2.0 * at[1] - after[1]], after }
    }

    fn points(&self) -> [Pt; 3] {
        [self.at, self.before, self.after]
    }
}

fn near(a: Pt, b: Pt) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6
}

/// The cubic segments of a curve: (start, control, control, end).
pub fn segments(nodes: &[Node], closed: bool) -> Vec<[Pt; 4]> {
    let mut out: Vec<[Pt; 4]> = nodes.windows(2).filter_map(|w| Some(seg(w.first()?, w.get(1)?))).collect();
    if closed
        && nodes.len() > 2
        && let (Some(last), Some(first)) = (nodes.last(), nodes.first())
    {
        out.push(seg(last, first));
    }
    out
}

fn seg(a: &Node, b: &Node) -> [Pt; 4] {
    [a.at, a.after, b.before, b.at]
}

/// The point at `t` (0 to 1) along a cubic segment.
pub fn point_at(s: &[Pt; 4], t: f64) -> Pt {
    let u = 1.0 - t;
    let [a, b, c, d] = s;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    [w[0] * a[0] + w[1] * b[0] + w[2] * c[0] + w[3] * d[0], w[0] * a[1] + w[1] * b[1] + w[2] * c[1] + w[3] * d[1]]
}

/// The curve as short lines (each segment in [`STEPS`] pieces).
pub fn flatten(nodes: &[Node], closed: bool) -> Vec<Pt> {
    let mut out: Vec<Pt> = nodes.first().map(|f| vec![f.at]).unwrap_or_default();
    for s in segments(nodes, closed) {
        for i in 1..=STEPS {
            out.push(point_at(&s, i as f64 / STEPS as f64));
        }
    }
    out
}

/// The length of the curve (points), from its flattening into fine pieces.
pub fn length(nodes: &[Node], closed: bool) -> f64 {
    let mut total = 0.0;
    for s in segments(nodes, closed) {
        let mut prev = s[0];
        for i in 1..=64 {
            let p = point_at(&s, f64::from(i) / 64.0);
            total += (p[0] - prev[0]).hypot(p[1] - prev[1]);
            prev = p;
        }
    }
    total
}

/// The segment and place along it (`t`) nearest `p`, and how far away it is.
pub fn nearest(nodes: &[Node], closed: bool, p: Pt) -> Option<(usize, f64, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, s) in segments(nodes, closed).iter().enumerate() {
        for k in 0..=48 {
            let t = f64::from(k) / 48.0;
            let q = point_at(s, t);
            let d = (q[0] - p[0]).hypot(q[1] - p[1]);
            if best.is_none_or(|(_, _, bd)| d < bd) {
                best = Some((i, t, d));
            }
        }
    }
    best
}

/// The curve with a new point at `t` along segment `seg` (the curve keeps its shape: de
/// Casteljau's split).
pub fn split(nodes: &[Node], closed: bool, seg_index: usize, t: f64) -> Vec<Node> {
    let segs = segments(nodes, closed);
    let Some(s) = segs.get(seg_index) else { return nodes.to_vec() };
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: Pt, b: Pt| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let [p0, p1, p2, p3] = *s;
    let (a, b, c) = (lerp(p0, p1), lerp(p1, p2), lerp(p2, p3));
    let (d, e) = (lerp(a, b), lerp(b, c));
    let mid = lerp(d, e);
    let mut out = nodes.to_vec();
    let next = if seg_index + 1 < nodes.len() { seg_index + 1 } else { 0 };
    if let Some(n0) = out.get_mut(seg_index) {
        n0.after = a;
    }
    if let Some(n1) = out.get_mut(next) {
        n1.before = c;
    }
    let new = Node { at: mid, before: d, after: e };
    if next == 0 {
        out.push(new);
    } else {
        out.insert(next, new);
    }
    out
}

/// A curve's points and whether it closes, from an annotation dictionary.
pub fn path_of(d: &Dict) -> Option<(Vec<Node>, bool)> {
    let v: Vec<f64> = d.get(PATH)?.as_array()?.iter().filter_map(Object::as_f64).take(MAX_NODES * 6).collect();
    if v.len() < 12 || !v.len().is_multiple_of(6) || !finite(&v) {
        return None;
    }
    let nodes = v.as_chunks::<6>().0.iter().map(|c| Node { at: [c[0], c[1]], before: [c[2], c[3]], after: [c[4], c[5]] }).collect();
    Some((nodes, matches!(d.get(CLOSED), Some(Object::Bool(true)))))
}

fn check(nodes: &[Node]) -> Result<(), AnnotError> {
    if nodes.len() < 2 {
        return Err(AnnotError::Invalid("a curve needs two points".into()));
    }
    if nodes.len() > MAX_NODES {
        return Err(AnnotError::Invalid("too many points on the curve".into()));
    }
    if !nodes.iter().flat_map(Node::points).all(|p| finite(&p) && p.iter().all(|v| v.abs() < MAX_COORD)) {
        return Err(AnnotError::Invalid("invalid point".into()));
    }
    Ok(())
}

/// Write the curve into an Ink annotation's dictionary: its points, its flattening and its
/// rectangle (the points and handles, which the curve stays within, with room for the pen).
fn write(d: &mut Dict, nodes: &[Node], closed: bool, meta: &Meta) {
    let numbers: Vec<f64> = nodes.iter().flat_map(|n| n.points()).flatten().collect();
    d.set(PATH.to_vec(), num_array(&numbers));
    if closed {
        d.set(CLOSED.to_vec(), Object::Bool(true));
    } else {
        d.remove(CLOSED);
    }
    let flat = flatten(nodes, closed);
    d.set(b"InkList".to_vec(), Object::Array(vec![num_array(&flat.concat())]));
    let w = crate::appearance::border_width(d);
    if let Some(b) = bounds(nodes.iter().flat_map(Node::points)) {
        d.set(b"Rect".to_vec(), num_array(&grow(b, w / 2.0 + 1.0)));
    }
    touch(d, meta);
}

/// Draw a new curve on `page`. Returns its index.
pub fn add_path(
    doc: &mut Document,
    page: usize,
    nodes: &[Node],
    closed: bool,
    style: &Style,
    author: &str,
    meta: &Meta,
) -> Result<usize, AnnotError> {
    check(nodes)?;
    let closed = closed && nodes.len() > 2;
    let new = NewAnnotation {
        page,
        shape: Shape::Ink { strokes: vec![flatten(nodes, closed)] },
        style: style.clone(),
        contents: String::new(),
        author: author.into(),
    };
    let index = add_annotation(doc, &new, meta)?;
    let (_, r) = annot_ref(doc, page, index)?;
    doc.update_dict(r, |d| {
        write(d, nodes, closed, meta);
        d.set(b"Subj".to_vec(), PdfString::text("Curve"));
    })?;
    set_appearance(doc, r)?;
    Ok(index)
}

/// Replace the points of the curve at `(page, index)`.
pub fn set_path(doc: &mut Document, page: usize, index: usize, nodes: &[Node], closed: bool, meta: &Meta) -> Result<(), AnnotError> {
    check(nodes)?;
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let before = annot_dict(doc, r);
    if path_of(&before).is_none() {
        return Err(AnnotError::Invalid("not a curve drawn with the pen".into()));
    }
    let closed = closed && nodes.len() > 2;
    doc.update_dict(r, |d| write(d, nodes, closed, meta))?;
    if let Err(e) = set_appearance(doc, r) {
        doc.set(r, Object::Dict(before));
        return Err(e);
    }
    Ok(())
}

/// The curves on `page`: (index, points, closed).
pub fn paths(doc: &Document, page: usize) -> Vec<(usize, Vec<Node>, bool)> {
    let Ok(p) = crate::page_ref(doc, page) else { return Vec::new() };
    crate::annots(doc, p)
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let obj = doc.resolve(e);
            let (nodes, closed) = path_of(obj.as_dict()?)?;
            Some((i, nodes, closed))
        })
        .collect()
}

/// The appearance's path: the curve as PDF curves (`m`, `c`, closed with `h`).
pub(crate) fn content(d: &Dict) -> Option<String> {
    let (nodes, closed) = path_of(d)?;
    let first = nodes.first()?;
    let mut c = format!("{} {} m\n", n(first.at[0]), n(first.at[1]));
    for [_, b, cc, e] in segments(&nodes, closed) {
        c.push_str(&format!("{} {} {} {} {} {} c\n", n(b[0]), n(b[1]), n(cc[0]), n(cc[1]), n(e[0]), n(e[1])));
    }
    if closed {
        c.push_str("h\n");
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_curve_splits_without_changing_shape() {
        let nodes = vec![Node::smooth([0.0, 0.0], [10.0, 20.0]), Node::smooth([40.0, 0.0], [50.0, -20.0])];
        let before = segments(&nodes, false);
        let p = point_at(&before[0], 0.3);
        let after = split(&nodes, false, 0, 0.3);
        assert_eq!(after.len(), 3);
        assert!((after[1].at[0] - p[0]).abs() < 1e-9 && (after[1].at[1] - p[1]).abs() < 1e-9);
        // The halves together trace the same curve.
        let segs = segments(&after, false);
        let q = point_at(&segs[1], 0.5);
        let r = point_at(&before[0], 0.3 + 0.7 * 0.5);
        assert!((q[0] - r[0]).abs() < 1e-9 && (q[1] - r[1]).abs() < 1e-9);
    }

    #[test]
    fn corners_closed_curves_and_lengths() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]].map(Node::corner).to_vec();
        assert!(square.iter().all(Node::is_corner));
        assert_eq!(segments(&square, false).len(), 3);
        assert_eq!(segments(&square, true).len(), 4);
        assert!((length(&square, true) - 40.0).abs() < 1e-6);
        assert_eq!(flatten(&square, false).len(), 1 + 3 * STEPS);
        let (seg, t, d) = nearest(&square, true, [5.0, -1.0]).unwrap();
        assert_eq!(seg, 0);
        assert!((t - 0.5).abs() < 0.05 && (d - 1.0).abs() < 1e-6);
        assert!(check(&square[..1]).is_err());
        assert!(check(&[Node::corner([f64::NAN, 0.0]), Node::corner([1.0, 1.0])]).is_err());
    }
}
