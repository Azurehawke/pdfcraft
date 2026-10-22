//! RevuCraft: a sheet's guides, as Illustrator's: lines dragged out of the rulers that the
//! drawing tools snap to. Kept in the page dictionary as `/PCGuides << /V [..] /H [..] >>`, in
//! view space (points from the sheet's top-left corner as shown upright, `/Rotate` applied), so a
//! vertical guide stays vertical however the page is stored. They are not page content and never
//! print.

use pdfcraft_cos::{Dict, Document as Cos, Object};

use crate::{Document, EditError};

/// The page key.
const KEY: &[u8] = b"PCGuides";

/// Guides a sheet keeps, each way; more are refused.
pub const MAX: usize = 200;

/// How far off the sheet a guide may sit, in points (the pasteboard).
const REACH: f64 = 14_400.0;

/// A sheet's guides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guides {
    /// Vertical guides: their distance from the sheet's left edge.
    pub vertical: Vec<f64>,
    /// Horizontal guides: their distance from the sheet's top edge.
    pub horizontal: Vec<f64>,
}

impl Guides {
    pub fn is_empty(&self) -> bool {
        self.vertical.is_empty() && self.horizontal.is_empty()
    }
}

fn invalid(msg: &str) -> EditError {
    EditError::Source(msg.to_string())
}

fn numbers(cos: &Cos, d: &Dict, key: &[u8]) -> Vec<f64> {
    d.get(key)
        .map(|o| cos.resolve(o))
        .and_then(|o| {
            o.as_array().map(|a| a.iter().filter_map(|x| cos.resolve(x).as_f64()).filter(|v| v.is_finite() && v.abs() <= REACH).take(MAX).collect())
        })
        .unwrap_or_default()
}

/// The guides on `page` (none when it has none or they can't be read).
pub(crate) fn read(cos: &Cos, page: usize) -> Guides {
    let Ok(refs) = pdfcraft_annot::page_refs(cos) else { return Guides::default() };
    let Some(r) = refs.get(page).copied() else { return Guides::default() };
    let page = cos.get(r);
    let Some(d) = page.as_dict().and_then(|p| p.get(KEY)).map(|o| cos.resolve(o)) else { return Guides::default() };
    let Some(d) = d.as_dict() else { return Guides::default() };
    Guides { vertical: numbers(cos, d, b"V"), horizontal: numbers(cos, d, b"H") }
}

/// Sets the guides on `page`; none removes the key.
pub(crate) fn write(cos: &mut Cos, page: usize, guides: &Guides) -> Result<(), EditError> {
    let ok = |v: &[f64]| v.len() <= MAX && v.iter().all(|x| x.is_finite() && x.abs() <= REACH);
    if !ok(&guides.vertical) || !ok(&guides.horizontal) {
        return Err(invalid("invalid guides"));
    }
    let refs = pdfcraft_annot::page_refs(cos).map_err(|e| invalid(&e.to_string()))?;
    let r = refs.get(page).copied().ok_or_else(|| invalid("no such page"))?;
    let array = |v: &[f64]| Object::Array(v.iter().map(|x| Object::Real(*x)).collect());
    cos.update_dict(r, |p| {
        if guides.is_empty() {
            p.remove(KEY);
        } else {
            let mut d = Dict::new();
            d.set(b"V".to_vec(), array(&guides.vertical));
            d.set(b"H".to_vec(), array(&guides.horizontal));
            p.set(KEY.to_vec(), Object::Dict(d));
        }
    })
    .map_err(|e| invalid(&e.to_string()))
}

impl Document {
    /// The guides on sheet `page`.
    pub fn guides(&self, page: usize) -> Guides {
        self.editor.as_ref().map(|e| read(&e.cos, page)).unwrap_or_default()
    }
}
