//! Layers you manage, as in a CAD program: optional content groups (ISO 32000-2 §8.11) that
//! markups, symbols and photos are placed on. A layer is a standard OCG in the catalog's
//! `/OCProperties`, so every PDF viewer can show and hide it; RevuCraft adds a colour
//! (`/PCColor`) and a lock (`/PCLocked`). Locking a layer sets the standard Locked flag (`/F`
//! bit 8) on every annotation placed on it, so nothing on it can be moved, restyled or deleted
//! until it is unlocked; annotations remember whether they were locked before (`/PCLayerLocked`)
//! so unlocking the layer leaves individually locked ones locked.
//!
//! An annotation is on a layer through its `/OC` entry (§12.5.2), which viewers honour when
//! drawing it.

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString};

use crate::{AnnotError, FLAG_LOCKED, Rgb, annot_ref, annots, page_refs};

/// Layers at most: the catalog is untrusted input.
const MAX_LAYERS: usize = 4096;
/// Name length limit, in characters.
const MAX_NAME: usize = 128;

/// A layer as RevuCraft shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerInfo {
    pub id: ObjRef,
    pub name: String,
    /// The colour new symbols and markups on the layer take (black when none was set).
    pub color: Rgb,
    pub locked: bool,
    /// The line width (points) lines drafted on the layer take ("ByLayer"), when one is set
    /// (`/PCWidth`).
    pub width: Option<f64>,
}

/// The line widths a layer may have, in points.
pub const WIDTHS: std::ops::RangeInclusive<f64> = 0.05..=72.0;

/// The catalog's `/OCProperties` dictionary, and the object holding it when it is indirect.
fn properties(doc: &Document) -> Option<(Option<ObjRef>, Dict)> {
    let root = doc.root()?;
    let catalog = doc.get(root);
    let entry = catalog.as_dict()?.get(b"OCProperties")?.clone();
    match entry {
        Object::Ref(r) => doc.get(r).as_dict().cloned().map(|d| (Some(r), d)),
        Object::Dict(d) => Some((None, d)),
        _ => None,
    }
}

fn write_properties(doc: &mut Document, at: Option<ObjRef>, props: Dict) -> Result<(), AnnotError> {
    match at {
        Some(r) => doc.set(r, Object::Dict(props)),
        None => {
            let root = doc.root().ok_or(AnnotError::NoPageTree)?;
            doc.update_dict(root, |d| d.set(b"OCProperties".to_vec(), Object::Dict(props)))?;
        }
    }
    Ok(())
}

/// The OCGs listed in `/OCProperties /OCGs`, in order.
fn group_refs(doc: &Document, props: &Dict) -> Vec<ObjRef> {
    let Some(list) = props.get(b"OCGs").map(|o| doc.resolve(o)) else { return Vec::new() };
    let mut out: Vec<ObjRef> = list.as_array().map(|a| a.iter().filter_map(Object::as_ref).collect()).unwrap_or_default();
    out.truncate(MAX_LAYERS);
    out
}

fn color_of(doc: &Document, d: &Dict) -> Rgb {
    let values: Vec<f64> = d
        .get(b"PCColor")
        .map(|o| doc.resolve(o))
        .and_then(|o| o.as_array().map(|a| a.iter().filter_map(Object::as_f64).collect()))
        .unwrap_or_default();
    match values.as_slice() {
        [r, g, b] if [r, g, b].iter().all(|v| v.is_finite()) => [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)],
        _ => [0.0; 3],
    }
}

/// Every layer of the document, in the catalog's order.
pub fn list(doc: &Document) -> Vec<LayerInfo> {
    let Some((_, props)) = properties(doc) else { return Vec::new() };
    group_refs(doc, &props)
        .into_iter()
        .filter_map(|id| {
            let obj = doc.get(id);
            let d = obj.as_dict()?;
            let name = d.get(b"Name").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
            let locked = matches!(d.get(b"PCLocked").map(|o| doc.resolve(o)).as_deref(), Some(Object::Bool(true)));
            let width = d.get(b"PCWidth").map(|o| doc.resolve(o)).and_then(|o| o.as_f64()).filter(|w| *w == 0.0 || WIDTHS.contains(w));
            Some(LayerInfo { id, name, color: color_of(doc, d), locked, width })
        })
        .collect()
}

fn check_name(name: &str) -> Result<String, AnnotError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(AnnotError::Invalid(format!("a layer name must be 1 to {MAX_NAME} printable characters")));
    }
    Ok(name.to_string())
}

fn check_color(color: Rgb) -> Result<Rgb, AnnotError> {
    if color.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)) {
        Ok(color)
    } else {
        Err(AnnotError::Invalid("a layer colour needs three values from 0 to 1".into()))
    }
}

fn color_array(c: Rgb) -> Object {
    Object::Array(c.iter().map(|v| Object::Real(*v)).collect())
}

/// Add a layer, visible, at the end of the layer list. Returns its OCG.
pub fn add(doc: &mut Document, name: &str, color: Rgb) -> Result<ObjRef, AnnotError> {
    let name = check_name(name)?;
    let color = check_color(color)?;
    let (at, mut props) = properties(doc).unwrap_or((None, Dict::new()));
    let mut groups = group_refs(doc, &props);
    if groups.len() >= MAX_LAYERS {
        return Err(AnnotError::Invalid("the document has too many layers".into()));
    }
    let mut ocg = Dict::new();
    ocg.set(b"Type".to_vec(), Object::name("OCG"));
    ocg.set(b"Name".to_vec(), PdfString::text(&name));
    ocg.set(b"PCColor".to_vec(), color_array(color));
    let id = doc.add(Object::Dict(ocg));
    groups.push(id);
    // The list is written in full (inline), keeping every group already in it.
    props.set(b"OCGs".to_vec(), Object::Array(groups.iter().map(|r| Object::Ref(*r)).collect()));
    // The default configuration lists it in the layers panel order; visible by default (no /OFF).
    let mut config = props.get(b"D").map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default();
    let mut order = config.get(b"Order").map(|o| doc.resolve(o)).and_then(|o| o.as_array().cloned()).unwrap_or_default();
    if order.len() < MAX_LAYERS * 4 {
        order.push(Object::Ref(id));
    }
    config.set(b"Order".to_vec(), Object::Array(order));
    if !config.contains(b"Name") {
        config.set(b"Name".to_vec(), PdfString::text("Layers"));
    }
    props.set(b"D".to_vec(), Object::Dict(config));
    write_properties(doc, at, props)?;
    Ok(id)
}

fn layer(doc: &Document, id: ObjRef) -> Result<Dict, AnnotError> {
    let ours = properties(doc).is_some_and(|(_, props)| group_refs(doc, &props).contains(&id));
    let obj = doc.get(id);
    match obj.as_dict() {
        Some(d) if ours && d.name(b"Type") == Some(b"OCG") => Ok(d.clone()),
        _ => Err(AnnotError::Invalid("that layer doesn't exist".into())),
    }
}

pub fn rename(doc: &mut Document, id: ObjRef, name: &str) -> Result<(), AnnotError> {
    let name = check_name(name)?;
    layer(doc, id)?;
    doc.update_dict(id, |d| d.set(b"Name".to_vec(), PdfString::text(&name)))?;
    Ok(())
}

pub fn set_color(doc: &mut Document, id: ObjRef, color: Rgb) -> Result<(), AnnotError> {
    let color = check_color(color)?;
    layer(doc, id)?;
    doc.update_dict(id, |d| d.set(b"PCColor".to_vec(), color_array(color)))?;
    Ok(())
}

/// Set the line width lines drafted on layer `id` take (`None`: none, the ribbon's width).
pub fn set_width(doc: &mut Document, id: ObjRef, width: Option<f64>) -> Result<(), AnnotError> {
    // 0 is a hairline.
    if width.is_some_and(|w| w != 0.0 && !WIDTHS.contains(&w)) {
        return Err(AnnotError::Invalid("a layer's line width must be a hairline (0) or from 0.05 to 72 pt".into()));
    }
    layer(doc, id)?;
    doc.update_dict(id, |d| match width {
        Some(w) => d.set(b"PCWidth".to_vec(), Object::Real(w)),
        None => {
            d.remove(b"PCWidth");
        }
    })?;
    Ok(())
}

/// The layer an annotation is on (its `/OC`, when that is one of the document's layers).
pub fn layer_of(d: &Dict) -> Option<ObjRef> {
    d.reference(b"OC")
}

/// Every annotation on `id`, as (page, index).
fn on_layer(doc: &Document, id: ObjRef) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (page, p) in page_refs(doc).unwrap_or_default().into_iter().enumerate() {
        for (index, entry) in annots(doc, p).iter().enumerate() {
            let obj = doc.resolve(entry);
            if obj.as_dict().and_then(layer_of) == Some(id) {
                out.push((page, index));
            }
        }
    }
    out
}

/// Lock or unlock a layer and everything on it.
pub fn set_locked(doc: &mut Document, id: ObjRef, locked: bool) -> Result<(), AnnotError> {
    layer(doc, id)?;
    doc.update_dict(id, |d| {
        if locked {
            d.set(b"PCLocked".to_vec(), Object::Bool(true));
        } else {
            d.remove(b"PCLocked");
        }
    })?;
    for (page, index) in on_layer(doc, id) {
        let (_, r) = annot_ref(doc, page, index)?;
        doc.update_dict(r, |d| apply_lock(d, locked))?;
    }
    Ok(())
}

/// Set or clear the Locked flag for a layer lock, remembering an annotation's own lock.
fn apply_lock(d: &mut Dict, locked: bool) {
    let flags = d.get(b"F").and_then(Object::as_int).unwrap_or(0);
    if locked {
        if flags & FLAG_LOCKED != 0 && !d.contains(b"PCLayerLocked") {
            // Already locked on its own: unlocking the layer must leave it so.
            return;
        }
        d.set(b"PCLayerLocked".to_vec(), Object::Bool(true));
        d.set(b"F".to_vec(), Object::Int(flags | FLAG_LOCKED));
    } else if d.contains(b"PCLayerLocked") {
        d.remove(b"PCLayerLocked");
        d.set(b"F".to_vec(), Object::Int(flags & !FLAG_LOCKED));
    }
}

/// Put the annotation at (page, index) on `layer`, or on no layer. Placing it on a locked layer
/// locks it.
pub fn assign(doc: &mut Document, page: usize, index: usize, layer_id: Option<ObjRef>) -> Result<(), AnnotError> {
    let locked = match layer_id {
        Some(id) => layer(doc, id)?.get(b"PCLocked").is_some_and(|o| matches!(o, Object::Bool(true))),
        None => false,
    };
    let (_, r) = annot_ref(doc, page, index)?;
    doc.update_dict(r, |d| {
        match layer_id {
            Some(id) => d.set(b"OC".to_vec(), Object::Ref(id)),
            None => {
                d.remove(b"OC");
            }
        }
        apply_lock(d, locked);
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Meta, NewAnnotation, Shape, Style, add_annotation};
    use pdfcraft_cos::{SaveOptions, write_full};
    use std::sync::Arc;

    fn blank() -> Document {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        Document::open(Arc::new(pdf.to_vec())).unwrap()
    }

    fn square(doc: &mut Document) -> usize {
        let shape = Shape::Rectangle { rect: [10.0, 10.0, 50.0, 50.0] };
        let new = NewAnnotation { page: 0, style: Style::default_for(&shape), shape, contents: String::new(), author: "A".into() };
        add_annotation(doc, &new, &Meta::default()).unwrap()
    }

    #[test]
    fn layers_are_created_named_coloured_and_survive_saving() {
        let mut doc = blank();
        assert!(list(&doc).is_empty());
        let data = add(&mut doc, "Data", [0.0, 0.4, 0.9]).unwrap();
        let wifi = add(&mut doc, " Wi-Fi ", [0.9, 0.5, 0.1]).unwrap();
        rename(&mut doc, wifi, "Wireless").unwrap();
        set_color(&mut doc, data, [0.1, 0.2, 0.3]).unwrap();
        assert!(add(&mut doc, "   ", [0.0; 3]).is_err());
        assert!(add(&mut doc, "Bad", [2.0, 0.0, 0.0]).is_err());
        assert!(rename(&mut doc, ObjRef::new(999, 0), "x").is_err());
        let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
        let doc = Document::open(Arc::new(bytes)).unwrap();
        let names: Vec<(String, Rgb)> = list(&doc).into_iter().map(|l| (l.name, l.color)).collect();
        assert_eq!(names, [("Data".to_string(), [0.1, 0.2, 0.3]), ("Wireless".to_string(), [0.9, 0.5, 0.1])]);
    }

    #[test]
    fn annotations_go_on_layers_and_locking_the_layer_locks_them() {
        let mut doc = blank();
        let layer_id = add(&mut doc, "Cameras", [0.0; 3]).unwrap();
        let a = square(&mut doc);
        let b = square(&mut doc);
        assign(&mut doc, 0, a, Some(layer_id)).unwrap();
        assign(&mut doc, 0, b, Some(layer_id)).unwrap();
        // b is locked on its own before the layer is.
        let (_, rb) = annot_ref(&mut doc, 0, b).unwrap();
        doc.update_dict(rb, |d| d.set(b"F".to_vec(), Object::Int(4 | FLAG_LOCKED))).unwrap();
        set_locked(&mut doc, layer_id, true).unwrap();
        let flags = |doc: &mut Document, i: usize| {
            let (_, r) = annot_ref(doc, 0, i).unwrap();
            doc.get(r).as_dict().and_then(|d| d.get(b"F").and_then(Object::as_int)).unwrap_or(0)
        };
        assert!(flags(&mut doc, a) & FLAG_LOCKED != 0);
        assert!(crate::move_annotation(&mut doc, 0, a, 5.0, 5.0, &Meta::default()).is_err(), "locked by its layer");
        set_locked(&mut doc, layer_id, false).unwrap();
        assert_eq!(flags(&mut doc, a) & FLAG_LOCKED, 0);
        assert!(flags(&mut doc, b) & FLAG_LOCKED != 0, "its own lock stays");
        // A new annotation placed on a locked layer is locked too; off the layer, it isn't.
        set_locked(&mut doc, layer_id, true).unwrap();
        let c = square(&mut doc);
        assign(&mut doc, 0, c, Some(layer_id)).unwrap();
        assert!(flags(&mut doc, c) & FLAG_LOCKED != 0);
        let (_, rc) = annot_ref(&mut doc, 0, c).unwrap();
        assert_eq!(layer_of(&doc.get(rc).as_dict().cloned().unwrap()), Some(layer_id));
        assert!(list(&doc)[0].locked);
    }
}
