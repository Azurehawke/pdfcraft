//! RevuCraft: a callout's arrow glued to a connection site of its box.
//!
//! A callout's leader meets its box where the box's side faces the arrow's knee, unless the user
//! has glued it: then it meets a fixed site of the box (the middle of a side, or a corner), so it
//! stays there as the box is moved or resized. A glue is `/PCGlue n` on the callout, `n` being the
//! site (see [`site_point`]). Other viewers see only the leader's end point, which is kept up to
//! date, so the sheet reads the same everywhere.
//!
//! The leader is re-attached by the one place that refits a callout's box (`apply_text_box`), which
//! calls [`attach`]; this module only stores the glue and gives the sites.

use pdfcraft_cos::{Dict, Document, Object};

use crate::{AnnotError, Meta, annot_dict, annot_ref, annots, apply_text_box, callout_tip, normalize, page_ref, text_box_of, touch, unlocked};

/// The glue key (`/PCGlue`).
pub const GLUE: &[u8] = b"PCGlue";
/// The sites a box has: four side middles, then four corners.
pub const SITES: usize = 8;

/// Connection site `site` of the box `rect` (in page space); `None` for a site it doesn't have.
/// Sites run top, right, bottom, left (the side middles), then top-left, top-right, bottom-right
/// and bottom-left (the corners).
pub fn site_point(rect: [f64; 4], site: u32) -> Option<[f64; 2]> {
    let [x0, y0, x1, y1] = normalize(rect);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    Some(match site {
        0 => [cx, y1],
        1 => [x1, cy],
        2 => [cx, y0],
        3 => [x0, cy],
        4 => [x0, y1],
        5 => [x1, y1],
        6 => [x1, y0],
        7 => [x0, y0],
        _ => return None,
    })
}

/// The site a callout is glued to, if it is glued to a real one.
pub fn glued_site(d: &Dict) -> Option<u32> {
    let v = d.get(GLUE)?.as_f64()?;
    if !v.is_finite() || v.fract() != 0.0 || !(0.0..SITES as f64).contains(&v) {
        return None;
    }
    // In range and whole, so the cast is exact.
    Some(v as u32)
}

/// Where a callout's leader meets its box: the glued site when there is one, else `fallback` (the
/// side facing the knee).
pub fn attach(d: &Dict, rect: [f64; 4], fallback: [f64; 2]) -> [f64; 2] {
    glued_site(d).and_then(|s| site_point(rect, s)).unwrap_or(fallback)
}

/// A callout's glue as the interface shows it: the site it is glued to (if any), its box and where
/// its arrow meets the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glue {
    pub site: Option<u32>,
    pub rect: [f64; 4],
    pub attach: [f64; 2],
}

/// The glue of the callout at `(page, index)`, or `None` when that markup isn't a callout.
pub fn glue_at(doc: &Document, page: usize, index: usize) -> Option<Glue> {
    let p = page_ref(doc, page).ok()?;
    let entry = annots(doc, p).get(index).cloned()?;
    let obj = doc.resolve(&entry);
    let d = obj.as_dict()?;
    callout_tip(d)?;
    let rect = text_box_of(d)?;
    let cl: Vec<f64> = d.get(b"CL")?.as_array()?.iter().filter_map(Object::as_f64).collect();
    let [x, y] = match cl.as_slice() {
        [_, _, _, _, x, y] if x.is_finite() && y.is_finite() => [*x, *y],
        _ => return None,
    };
    Some(Glue { site: glued_site(d), rect, attach: [x, y] })
}

/// Glue a callout's arrow to a site of its box, or unglue it (`None`). The leader is re-attached
/// at once.
pub fn set_callout_glue(doc: &mut Document, page: usize, index: usize, site: Option<u32>, meta: &Meta) -> Result<(), AnnotError> {
    if site.is_some_and(|s| s as usize >= SITES) {
        return Err(AnnotError::Invalid("no such connection site".into()));
    }
    let (_, r) = annot_ref(doc, page, index)?;
    unlocked(doc, r)?;
    let before = annot_dict(doc, r);
    if callout_tip(&before).is_none() {
        return Err(AnnotError::Invalid("not a callout".into()));
    }
    let tb = text_box_of(&before).ok_or_else(|| AnnotError::Invalid("the callout has no text box".into()))?;
    doc.update_dict(r, |d| {
        match site {
            Some(s) => {
                d.set(GLUE.to_vec(), Object::Int(i64::from(s)));
            }
            None => {
                d.remove(GLUE);
            }
        }
        touch(d, meta);
    })?;
    apply_text_box(doc, r, tb, meta, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_the_side_middles_then_the_corners() {
        let r = [0.0, 0.0, 10.0, 20.0];
        assert_eq!(site_point(r, 0), Some([5.0, 20.0]));
        assert_eq!(site_point(r, 1), Some([10.0, 10.0]));
        assert_eq!(site_point(r, 2), Some([5.0, 0.0]));
        assert_eq!(site_point(r, 3), Some([0.0, 10.0]));
        assert_eq!(site_point(r, 7), Some([0.0, 0.0]));
        // A reversed rectangle has the same sites; a site past the last has none.
        assert_eq!(site_point([10.0, 20.0, 0.0, 0.0], 1), Some([10.0, 10.0]));
        assert_eq!(site_point(r, SITES as u32), None);
    }

    #[test]
    fn only_whole_in_range_glues_count() {
        let glue = |v: Object| {
            let mut d = Dict::new();
            d.set(GLUE.to_vec(), v);
            glued_site(&d)
        };
        assert_eq!(glue(Object::Int(3)), Some(3));
        assert_eq!(glue(Object::Real(4.0)), Some(4));
        assert_eq!(glue(Object::Int(SITES as i64)), None);
        assert_eq!(glue(Object::Int(-1)), None);
        assert_eq!(glue(Object::Real(1.5)), None);
        assert_eq!(glue(Object::Real(f64::NAN)), None);
        assert_eq!(glue(Object::name("Left")), None);
        // Unglued, the fallback stays.
        assert_eq!(attach(&Dict::new(), [0.0, 0.0, 10.0, 10.0], [7.0, 7.0]), [7.0, 7.0]);
    }
}
