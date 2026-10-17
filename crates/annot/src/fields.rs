//! RevuCraft's custom columns: your own fields on a markup (drop ID, port, cable type, progress),
//! kept in the annotation as `/PCFields`, a dictionary of text values by column name. Other viewers
//! ignore it; the Markups list, the Properties box, exports and the summary report read it.
//!
//! Field names and values come from the user and from files, so both are cleaned and capped.

use pdfcraft_cos::{Dict, Document, Object, PdfString};

use crate::{AnnotError, Meta, annot_ref, annots, page_refs};

/// Fields kept on one markup at most, and the longest name and value (characters).
pub const MAX_FIELDS: usize = 64;
pub const MAX_NAME: usize = 40;
pub const MAX_VALUE: usize = 500;
/// Markups read at most when listing every field.
const MAX_LISTED: usize = 200_000;

/// A field name or value cleaned: no control characters (a value may keep line breaks), trimmed,
/// cut to `max` characters.
fn clean(s: &str, max: usize, lines: bool) -> String {
    let kept: String = s.chars().filter(|c| !c.is_control() || (lines && *c == '\n')).collect();
    kept.trim().chars().take(max).collect()
}

/// A column name as stored.
pub fn clean_name(s: &str) -> String {
    clean(s, MAX_NAME, false)
}

/// The fields of an annotation dictionary, in the order stored.
pub fn fields_of(doc: &Document, d: &Dict) -> Vec<(String, String)> {
    let Some(obj) = d.get(b"PCFields").map(|o| doc.resolve(o)) else { return Vec::new() };
    let Some(f) = obj.as_dict() else { return Vec::new() };
    f.iter()
        .take(MAX_FIELDS)
        .filter_map(|(k, v)| {
            let name = clean_name(&String::from_utf8_lossy(k));
            let value = doc.resolve(v).as_string().map(PdfString::to_text)?;
            let value = clean(&value, MAX_VALUE, true);
            (!name.is_empty() && !value.is_empty()).then_some((name, value))
        })
        .collect()
}

/// Set the field `name` of the annotation at `(page, index)` to `value`; an empty value removes it.
pub fn set_field(doc: &mut Document, page: usize, index: usize, name: &str, value: &str, meta: &Meta) -> Result<(), AnnotError> {
    let name = clean_name(name);
    if name.is_empty() {
        return Err(AnnotError::Invalid("a column needs a name".into()));
    }
    let value = clean(value, MAX_VALUE, true);
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let current = crate::annot_dict(doc, r);
    let mut fields = match current.get(b"PCFields").map(|o| doc.resolve(o)) {
        Some(o) => o.as_dict().cloned().unwrap_or_default(),
        None => Dict::new(),
    };
    if value.is_empty() {
        fields.remove(name.as_bytes());
    } else {
        if fields.get(name.as_bytes()).is_none() && fields.len() >= MAX_FIELDS {
            return Err(AnnotError::Invalid("this markup has as many fields as it can hold".into()));
        }
        fields.set(name.as_bytes().to_vec(), PdfString::text(&value));
    }
    doc.update_dict(r, |d| {
        if fields.is_empty() {
            d.remove(b"PCFields");
        } else {
            d.set(b"PCFields".to_vec(), Object::Dict(fields.clone()));
        }
        crate::touch(d, meta);
    })?;
    Ok(())
}

/// A markup's fields: (name, value) pairs.
pub type Fields = Vec<(String, String)>;

/// Every markup that has fields: (page, index, fields).
pub fn all_fields(doc: &Document) -> Vec<(usize, usize, Fields)> {
    let mut out = Vec::new();
    let mut seen = 0usize;
    for (page, p) in page_refs(doc).unwrap_or_default().into_iter().enumerate() {
        for (index, entry) in annots(doc, p).iter().enumerate() {
            seen += 1;
            if seen > MAX_LISTED {
                return out;
            }
            let obj = doc.resolve(entry);
            let Some(d) = obj.as_dict() else { continue };
            if !d.contains(b"PCFields") {
                continue;
            }
            let f = fields_of(doc, d);
            if !f.is_empty() {
                out.push((page, index, f));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewAnnotation, Shape, Style, add_annotation};
    use pdfcraft_cos::{SaveOptions, write_full};
    use std::sync::Arc;

    #[test]
    fn fields_are_set_kept_and_removed() {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        let mut doc = Document::open(Arc::new(pdf.to_vec())).unwrap();
        let shape = Shape::Line { from: [10.0, 10.0], to: [100.0, 10.0], start: crate::LineEnding::None, end: crate::LineEnding::None };
        let new = NewAnnotation { page: 0, style: Style::default_for(&shape), shape, contents: String::new(), author: "Steve".into() };
        let i = add_annotation(&mut doc, &new, &Meta::default()).unwrap();
        set_field(&mut doc, 0, i, " Drop ID ", "D-101\u{7}", &Meta::default()).unwrap();
        set_field(&mut doc, 0, i, "Progress", "Pulled", &Meta::default()).unwrap();
        set_field(&mut doc, 0, i, "Progress", "Tested", &Meta::default()).unwrap();
        let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
        let mut doc = Document::open(Arc::new(bytes)).unwrap();
        let all = all_fields(&doc);
        assert_eq!(all.len(), 1);
        let f = &all[0].2;
        assert!(f.contains(&("Drop ID".into(), "D-101".into())) && f.contains(&("Progress".into(), "Tested".into())), "{f:?}");
        set_field(&mut doc, 0, i, "Drop ID", "", &Meta::default()).unwrap();
        set_field(&mut doc, 0, i, "Progress", " ", &Meta::default()).unwrap();
        assert!(all_fields(&doc).is_empty());
        assert!(set_field(&mut doc, 0, i, "  ", "x", &Meta::default()).is_err());
        assert!(set_field(&mut doc, 0, 99, "Port", "1", &Meta::default()).is_err());
    }
}
