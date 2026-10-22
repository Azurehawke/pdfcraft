//! RevuCraft: where a closed shape's stroke lies against its path, as VectorCraft's Align Stroke:
//! centred on it, inside it or outside it. Kept in the annotation as `/PCStrokeAlign`
//! (`/Center`, `/Inside`, `/Outside`) and drawn in its appearance: the stroke is drawn twice as
//! wide and clipped to the path's inside or outside, so corners come out exact. Open paths
//! (lines, connected lines) always stroke centred; clouds keep their scallops.
//!
//! A rectangle or oval's path is its `/Rect` less its `/RD` (ISO 32000-2 §12.5.6.8), the
//! `/RD` leaving room for a stroke centred on or outside the path; a polygon's is its vertices,
//! its `/Rect` grown to hold the stroke. A rectangle or oval without the key strokes inside its
//! `/Rect`, as PdfCraft has always drawn them.

use pdfcraft_cos::{Dict, Document, Object};

use crate::{AnnotError, annot_ref};

/// The annotation key.
pub const KEY: &[u8] = b"PCStrokeAlign";

/// Where the stroke lies against the path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrokeAlign {
    #[default]
    Center,
    Inside,
    Outside,
}

impl StrokeAlign {
    pub const ALL: [StrokeAlign; 3] = [StrokeAlign::Center, StrokeAlign::Inside, StrokeAlign::Outside];

    pub fn name(self) -> &'static str {
        match self {
            StrokeAlign::Center => "Center",
            StrokeAlign::Inside => "Inside",
            StrokeAlign::Outside => "Outside",
        }
    }

    pub fn from_name(s: &[u8]) -> Option<Self> {
        match s {
            b"Center" => Some(StrokeAlign::Center),
            b"Inside" => Some(StrokeAlign::Inside),
            b"Outside" => Some(StrokeAlign::Outside),
            _ => None,
        }
    }

    /// How far the stroke of `width` reaches outside the path.
    pub fn reach(self, width: f64) -> f64 {
        let w = if width.is_finite() { width.max(0.0) } else { 0.0 };
        match self {
            StrokeAlign::Center => w / 2.0,
            StrokeAlign::Inside => 0.0,
            StrokeAlign::Outside => w,
        }
    }
}

/// The alignment set on the annotation, if any.
pub fn of(d: &Dict) -> Option<StrokeAlign> {
    d.name(KEY).and_then(StrokeAlign::from_name)
}

/// Whether the annotation is a closed shape whose stroke can be aligned.
pub fn can_align(d: &Dict) -> bool {
    let cloudy = d.get(b"BE").and_then(Object::as_dict).is_some_and(|b| b.name(b"S") == Some(b"C"));
    match d.name(b"Subtype") {
        Some(b"Square" | b"Circle") => true,
        Some(b"Polygon") => !cloudy && !d.contains(b"IT"),
        _ => false,
    }
}

/// How the shape's stroke lies now: its own setting, else how it has always been drawn
/// (rectangles and ovals inside their box, polygons centred). `None` when it can't be aligned.
pub fn effective(d: &Dict) -> Option<StrokeAlign> {
    if !can_align(d) {
        return None;
    }
    Some(of(d).unwrap_or(match d.name(b"Subtype") {
        Some(b"Square" | b"Circle") => StrokeAlign::Inside,
        _ => StrokeAlign::Center,
    }))
}

fn nums(d: &Dict, key: &[u8]) -> Option<Vec<f64>> {
    let v: Vec<f64> = d.get(key)?.as_array()?.iter().map(Object::as_f64).collect::<Option<_>>()?;
    v.iter().all(|x| x.is_finite()).then_some(v)
}

fn array(v: &[f64]) -> Object {
    Object::Array(v.iter().map(|x| Object::Real(*x)).collect())
}

/// The rectangle or oval's path: its `/Rect` less its `/RD`.
pub fn shape_box(d: &Dict) -> Option<[f64; 4]> {
    let r = nums(d, b"Rect").filter(|r| r.len() == 4)?;
    let r = [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])];
    let rd = nums(d, b"RD").filter(|v| v.len() == 4 && v.iter().all(|x| *x >= 0.0)).unwrap_or_else(|| vec![0.0; 4]);
    let (l, b, rt, t) = (rd.first().copied()?, rd.get(1).copied()?, rd.get(2).copied()?, rd.get(3).copied()?);
    let inner = [r[0] + l, r[1] + b, r[2] - rt, r[3] - t];
    (inner[2] > inner[0] && inner[3] > inner[1]).then_some(inner)
}

/// Leave room in `/Rect` (and `/RD`) for the aligned stroke at the annotation's width, keeping
/// its path where it is. True when the dictionary changed. Called before each redraw, so a
/// changed width or alignment always fits.
pub(crate) fn fit(d: &mut Dict) -> bool {
    let Some(align) = of(d).filter(|_| can_align(d)) else { return false };
    let reach = align.reach(crate::appearance::border_width(d));
    match d.name(b"Subtype") {
        Some(b"Square" | b"Circle") => {
            let Some(p) = shape_box(d) else { return false };
            let rect = [p[0] - reach, p[1] - reach, p[2] + reach, p[3] + reach];
            let rd = [reach; 4];
            let same = nums(d, b"Rect").is_some_and(|r| r.iter().zip(rect).all(|(a, b)| (a - b).abs() < 1e-9))
                && nums(d, b"RD").is_some_and(|r| r.iter().zip(rd).all(|(a, b)| (a - b).abs() < 1e-9));
            if same {
                return false;
            }
            d.set(b"Rect".to_vec(), array(&rect));
            d.set(b"RD".to_vec(), array(&rd));
            true
        }
        _ => {
            let Some(v) = nums(d, b"Vertices").filter(|v| v.len() >= 4 && v.len() % 2 == 0) else { return false };
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for p in v.as_chunks::<2>().0 {
                x0 = x0.min(p[0]);
                y0 = y0.min(p[1]);
                x1 = x1.max(p[0]);
                y1 = y1.max(p[1]);
            }
            let m = reach + 1.0;
            let need = [x0 - m, y0 - m, x1 + m, y1 + m];
            let Some(r) = nums(d, b"Rect").filter(|r| r.len() == 4) else { return false };
            let r = [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])];
            if r[0] <= need[0] && r[1] <= need[1] && r[2] >= need[2] && r[3] >= need[3] {
                return false;
            }
            d.set(b"Rect".to_vec(), array(&[r[0].min(need[0]), r[1].min(need[1]), r[2].max(need[2]), r[3].max(need[3])]));
            true
        }
    }
}

/// Align the stroke of the shape at `(page, index)`; refused for a shape that isn't closed.
pub fn set(doc: &mut Document, page: usize, index: usize, align: StrokeAlign) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let mut d = crate::annot_dict(doc, r);
    if !can_align(&d) {
        return Err(AnnotError::Invalid("only a closed shape's stroke can be aligned (lines stroke on their centre)".into()));
    }
    let before = d.clone();
    // A rectangle or oval drawn before alignment strokes inside its box: that box is its path.
    if of(&d).is_none() && matches!(d.name(b"Subtype"), Some(b"Square" | b"Circle")) {
        d.remove(b"RD");
    }
    d.set(KEY.to_vec(), Object::name(align.name()));
    fit(&mut d);
    doc.set(r, Object::Dict(d));
    if let Err(e) = crate::set_appearance(doc, r) {
        doc.set(r, Object::Dict(before));
        return Err(e);
    }
    Ok(())
}
