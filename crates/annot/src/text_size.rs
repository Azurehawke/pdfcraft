//! RevuCraft: the size of the text RevuCraft draws on a markup that isn't a text box: a
//! measurement's caption, a symbol's label, a door's or window's mark, a space's name. Kept as
//! `/PCTextSize` (points); without it each draws at its usual size. Text boxes, typewriter text
//! and callouts keep their own font size (`/DA`).

use pdfcraft_cos::{Dict, Document, Object};

use crate::{AnnotError, Meta, annot_dict, annot_ref, set_appearance};

pub const TEXT_SIZE: &[u8] = b"PCTextSize";
/// The smallest and largest text size kept (points).
pub const MIN_TEXT_SIZE: f64 = 2.0;
pub const MAX_TEXT_SIZE: f64 = 400.0;

/// The text size set on an annotation, when it has a usable one.
pub fn text_size(d: &Dict) -> Option<f64> {
    d.get(TEXT_SIZE).and_then(Object::as_f64).filter(|v| v.is_finite() && (MIN_TEXT_SIZE..=MAX_TEXT_SIZE).contains(v))
}

/// The size the annotation's own text is drawn at (points), when it has text of this kind: the
/// size set, else its usual size. `None` for text boxes and markups without such text.
pub fn drawn_text_size(d: &Dict) -> Option<f64> {
    let usual = if d.contains(b"PCSymbol") {
        return Some(crate::symbols::label_points(d));
    } else if d.contains(crate::walls::OPENING) {
        crate::walls::default_mark_size(d)?
    } else if crate::punch::is_space(d) {
        crate::punch::LABEL
    } else if d.contains(b"PCMeasureValue") && matches!(d.name(b"Subtype"), Some(b"Line" | b"PolyLine" | b"Polygon")) {
        crate::appearance::MEASURE_TEXT
    } else {
        return None;
    };
    Some(text_size(d).unwrap_or(usual))
}

/// [`drawn_text_size`] of the annotation at `(page, index)`.
pub fn drawn_text_size_at(doc: &Document, page: usize, index: usize) -> Option<f64> {
    let p = crate::page_ref(doc, page).ok()?;
    let entry = crate::annots(doc, p).get(index).cloned()?;
    let obj = doc.resolve(&entry);
    drawn_text_size(obj.as_dict()?)
}

/// Draw the text of the markup at `(page, index)` at `size` points (`None`: its usual size), and
/// fit its rectangle round it.
pub fn set_text_size(doc: &mut Document, page: usize, index: usize, size: Option<f64>, meta: &Meta) -> Result<(), AnnotError> {
    if let Some(s) = size
        && !(s.is_finite() && (MIN_TEXT_SIZE..=MAX_TEXT_SIZE).contains(&s))
    {
        return Err(AnnotError::Invalid(format!("a text size is from {MIN_TEXT_SIZE} to {MAX_TEXT_SIZE} points")));
    }
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let d = annot_dict(doc, r);
    if drawn_text_size(&d).is_none() {
        return Err(AnnotError::Invalid("this markup has no text of its own to size".into()));
    }
    // A symbol's label changes its layout: it is drawn again round the same centre.
    if d.contains(b"PCSymbol") {
        return crate::symbols::set_label_size(doc, page, index, size, meta);
    }
    let host = crate::walls::host_of(doc, page, &d);
    let before = d.clone();
    doc.update_dict(r, |d| {
        match size {
            Some(s) => d.set(TEXT_SIZE.to_vec(), Object::Real(s)),
            None => {
                d.remove(TEXT_SIZE);
            }
        }
        crate::geometry::fit_caption(d);
        crate::touch(d, meta);
    })?;
    // A door's or window's mark: its wall lays it out again, rectangle and all.
    if let Some(wall) = host {
        return crate::walls::sync(doc, page, wall, meta);
    }
    if let Err(e) = set_appearance(doc, r) {
        doc.set(r, Object::Dict(before));
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_cos::PdfString;

    #[test]
    fn sizes_out_of_range_are_not_used() {
        let mut d = Dict::new();
        assert_eq!(text_size(&d), None);
        d.set(TEXT_SIZE.to_vec(), Object::Real(14.0));
        assert_eq!(text_size(&d), Some(14.0));
        d.set(TEXT_SIZE.to_vec(), Object::Real(f64::NAN));
        assert_eq!(text_size(&d), None);
        d.set(TEXT_SIZE.to_vec(), Object::Real(10_000.0));
        assert_eq!(text_size(&d), None);
        // Only markups with text of their own have a drawn size.
        assert_eq!(drawn_text_size(&Dict::new()), None);
        let mut m = Dict::new();
        m.set(b"Subtype".to_vec(), Object::name("Line"));
        m.set(b"PCMeasureValue".to_vec(), PdfString::text("12 ft"));
        assert_eq!(drawn_text_size(&m), Some(crate::appearance::MEASURE_TEXT));
        m.set(TEXT_SIZE.to_vec(), Object::Real(18.0));
        assert_eq!(drawn_text_size(&m), Some(18.0));
    }

    #[test]
    fn a_measurement_box_fits_its_line_and_caption() {
        let nums = |v: &[f64]| Object::Array(v.iter().map(|x| Object::Real(*x)).collect());
        let mut d = Dict::new();
        d.set(b"Subtype".to_vec(), Object::name("Line"));
        d.set(b"L".to_vec(), nums(&[100.0, 100.0, 300.0, 100.0]));
        d.set(b"PCMeasureValue".to_vec(), PdfString::text("20 ft"));
        d.set(b"Rect".to_vec(), nums(&[0.0, 0.0, 500.0, 400.0]));
        crate::geometry::fit_caption(&mut d);
        let r: Vec<f64> = d.get(b"Rect").and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).collect()).unwrap_or_default();
        let [x0, y0, x1, y1] = r[..] else { panic!("no rect: {r:?}") };
        assert!(x0 > 90.0 && x1 < 310.0, "{r:?}");
        assert!(y0 > 90.0 && y1 < 125.0, "the caption sits just above the line: {r:?}");
        // A bigger caption makes room for itself.
        d.set(TEXT_SIZE.to_vec(), Object::Real(30.0));
        crate::geometry::fit_caption(&mut d);
        let r2: Vec<f64> = d.get(b"Rect").and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).collect()).unwrap_or_default();
        assert!(r2[3] > y1 + 15.0, "{r2:?}");
    }
}
