//! RevuCraft: an arrow (a straight line) whose ends are glued to a connection site of another
//! markup's box.
//!
//! A glued end is `/PCGlueStart` or `/PCGlueEnd` on the line: `[ (name) site ]`, where `name` is
//! the target markup's `/NM` and `site` a connection site of its box (see [`site_point`]). After
//! every comment edit, each glued end is moved to its site, so the arrow follows a box that moves
//! or resizes. A glue to a markup that is gone leaves that end where it was. Glues are to boxes
//! only (squares, circles, text boxes, callouts' boxes, stamps); a line's end can't be glued to
//! another line yet.

use pdfcraft_cos::{Dict, Document, ObjRef, Object};

use crate::callout_glue::{SITES, site_point};
use crate::{
    AnnotError, Meta, annot_dict, annot_ref, annots, callout_tip, normalize, page_ref, page_refs, set_appearance, text_box_of, touch, unlocked,
};

/// The glue keys (`/PCGlueStart`, `/PCGlueEnd`).
pub const START: &[u8] = b"PCGlueStart";
pub const END: &[u8] = b"PCGlueEnd";
/// How far (points) an arrow's rectangle reaches past a moved end, so the end isn't clipped.
const REACH: f64 = 12.0;

/// The `/NM` of a markup, if it has one.
fn name_of(d: &Dict) -> Option<String> {
    match d.get(b"NM")? {
        Object::String(s) => Some(s.to_text()),
        _ => None,
    }
}

/// A glued end's target name and site, if the key holds a well-formed glue.
fn glue_of(d: &Dict, key: &[u8]) -> Option<(String, u32)> {
    let a = d.get(key)?.as_array()?;
    let name = match a.first()? {
        Object::String(s) => s.to_text(),
        _ => return None,
    };
    let site = match a.get(1)? {
        Object::Int(i) => u32::try_from(*i).ok()?,
        _ => return None,
    };
    Some((name, site))
}

/// The box a markup offers its connection sites on: a callout's text box, or a rectangle's `/Rect`
/// for the box kinds that can be glued to. `None` for anything else (lines, ink, polygons).
fn box_of(d: &Dict) -> Option<[f64; 4]> {
    if callout_tip(d).is_some() {
        return text_box_of(d);
    }
    if !matches!(d.name(b"Subtype"), Some(b"Square" | b"Circle" | b"FreeText" | b"Stamp")) {
        return None;
    }
    let v: Vec<f64> = d.get(b"Rect")?.as_array()?.iter().filter_map(Object::as_f64).collect();
    match v.as_slice() {
        [a, b, c, e] if [a, b, c, e].iter().all(|x| x.is_finite()) => Some(normalize([*a, *b, *c, *e])),
        _ => None,
    }
}

/// The box of the markup named `name` on the page at `page_index`.
fn find_target(doc: &Document, page_index: usize, name: &str) -> Option<[f64; 4]> {
    let p = page_ref(doc, page_index).ok()?;
    annots(doc, p).iter().find_map(|entry| {
        let obj = doc.resolve(entry);
        let d = obj.as_dict()?;
        if name_of(d).as_deref() == Some(name) { box_of(d) } else { None }
    })
}

/// Move the glued ends of the arrow at `r` (on page `page_index`) to their sites. Returns whether
/// anything moved.
fn reroute(doc: &mut Document, r: ObjRef, page_index: usize) -> Result<bool, AnnotError> {
    let d = annot_dict(doc, r);
    if d.name(b"Subtype") != Some(b"Line") {
        return Ok(false);
    }
    let Some(mut l) =
        d.get(b"L").and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).collect::<Vec<f64>>()).filter(|v| v.len() == 4)
    else {
        return Ok(false);
    };
    let mut moved = Vec::new();
    for (key, at) in [(START, 0usize), (END, 2usize)] {
        let Some((name, site)) = glue_of(&d, key) else { continue };
        let Some(bx) = find_target(doc, page_index, &name) else { continue };
        let Some(p) = site_point(bx, site) else { continue };
        if let (Some(&x), Some(&y)) = (l.get(at), l.get(at + 1))
            && ((x - p[0]).abs() > 1e-9 || (y - p[1]).abs() > 1e-9)
        {
            if let Some(v) = l.get_mut(at) {
                *v = p[0];
            }
            if let Some(v) = l.get_mut(at + 1) {
                *v = p[1];
            }
            moved.push(p);
        }
    }
    if moved.is_empty() {
        return Ok(false);
    }
    // The arrow's rectangle grows to hold the moved ends.
    let own: Vec<f64> = d.get(b"Rect").and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).collect()).unwrap_or_default();
    let mut rect = match own.as_slice() {
        [a, b, c, e] => normalize([*a, *b, *c, *e]),
        _ => [0.0; 4],
    };
    for p in &moved {
        rect = [rect[0].min(p[0] - REACH), rect[1].min(p[1] - REACH), rect[2].max(p[0] + REACH), rect[3].max(p[1] + REACH)];
    }
    let reals = |v: &[f64]| Object::Array(v.iter().map(|x| Object::Real(*x)).collect());
    doc.update_dict(r, |d| {
        d.set(b"L".to_vec(), reals(&l));
        d.set(b"Rect".to_vec(), reals(&rect));
    })?;
    set_appearance(doc, r)?;
    Ok(true)
}

/// Glue one end of the arrow at `(page, index)` to a site of the box named `target` (`Some`), or
/// free it (`None`). The end moves to the site at once.
pub fn set_arrow_glue(
    doc: &mut Document,
    page: usize,
    index: usize,
    start: bool,
    target: Option<(String, u32)>,
    meta: &Meta,
) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    unlocked(doc, r)?;
    if annot_dict(doc, r).name(b"Subtype") != Some(b"Line") {
        return Err(AnnotError::Invalid("only an arrow's ends can be glued".into()));
    }
    let key = if start { START } else { END };
    if let Some((name, site)) = &target {
        if *site as usize >= SITES {
            return Err(AnnotError::Invalid("no such connection site".into()));
        }
        if find_target(doc, page, name).is_none() {
            return Err(AnnotError::Invalid("nothing with a connection site is there".into()));
        }
    }
    doc.update_dict(r, |d| {
        match &target {
            Some((name, site)) => {
                d.set(key.to_vec(), Object::Array(vec![Object::String(pdfcraft_cos::PdfString::text(name)), Object::Int(i64::from(*site))]));
            }
            None => {
                d.remove(key);
            }
        }
        touch(d, meta);
    })?;
    reroute(doc, r, page)?;
    Ok(())
}

/// Whether one end of the arrow at `(page, index)` is glued.
pub fn is_glued(doc: &Document, page: usize, index: usize, start: bool) -> bool {
    let Ok(p) = page_ref(doc, page) else { return false };
    let Some(entry) = annots(doc, p).get(index).cloned() else { return false };
    let obj = doc.resolve(&entry);
    let Some(d) = obj.as_dict() else { return false };
    glue_of(d, if start { START } else { END }).is_some()
}

/// The markups on `page` whose boxes have connection sites: their names and boxes.
pub fn glue_targets(doc: &Document, page: usize) -> Vec<(String, [f64; 4])> {
    let Ok(p) = page_ref(doc, page) else { return Vec::new() };
    annots(doc, p)
        .iter()
        .filter_map(|entry| {
            let obj = doc.resolve(entry);
            let d = obj.as_dict()?;
            Some((name_of(d)?, box_of(d)?))
        })
        .collect()
}

/// Move every glued arrow's glued ends to their sites (after any comment edit). Returns nothing
/// changed when no arrow is glued.
pub fn reroute_all(doc: &mut Document) -> Result<(), AnnotError> {
    for (page_index, p) in page_refs(doc)?.into_iter().enumerate() {
        let count = annots(doc, p).len();
        for index in 0..count {
            let Ok((_, r)) = annot_ref(doc, page_index, index) else { continue };
            let d = annot_dict(doc, r);
            if glue_of(&d, START).is_none() && glue_of(&d, END).is_none() {
                continue;
            }
            // A locked arrow stays where it is.
            if unlocked(doc, r).is_err() {
                continue;
            }
            reroute(doc, r, page_index)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glue_must_be_a_name_and_a_whole_site() {
        let mut d = Dict::new();
        d.set(START.to_vec(), Object::Array(vec![Object::String(pdfcraft_cos::PdfString::text("box-1")), Object::Int(4)]));
        assert_eq!(glue_of(&d, START), Some(("box-1".to_string(), 4)));
        d.set(END.to_vec(), Object::Array(vec![Object::Int(1), Object::Int(4)]));
        assert_eq!(glue_of(&d, END), None);
        d.set(END.to_vec(), Object::Array(vec![Object::String(pdfcraft_cos::PdfString::text("x")), Object::Int(-2)]));
        assert_eq!(glue_of(&d, END), None);
    }
}
