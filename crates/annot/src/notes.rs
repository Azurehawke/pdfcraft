//! RevuCraft's Markdown notes: a note placed on the plan (a sticky note, `/Subtype /Text`) whose
//! text is Markdown in a `.md` file beside the PDF. `/PCNote` names that file, relative to the
//! PDF's folder. Older notes (no `/InFile`) also keep a copy of the Markdown in `/Contents`, so
//! the PDF still carries them when they travel without their folder. Newer notes (`/InFile true`)
//! keep their text only in the file, so their `/Contents` stays empty and the PDF stays small.

use pdfcraft_cos::{Dict, Document, Object, PdfString};

use crate::{AnnotError, annot_ref, annots, page_ref, set_appearance};

pub const NOTE: &[u8] = b"PCNote";

/// The longest file path kept (characters).
const MAX_PATH: usize = 1024;

/// The note's file, relative to the PDF's folder, when the annotation is a Markdown note.
pub fn file_of(d: &Dict) -> Option<String> {
    let n = d.get(NOTE)?.as_dict()?;
    let file = n.get(b"File").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
    Some(file.chars().take(MAX_PATH).collect())
}

/// Whether the note keeps its text only in its file (no copy in `/Contents`).
pub fn in_file_of(d: &Dict) -> bool {
    d.get(NOTE).and_then(Object::as_dict).is_some_and(|n| matches!(n.get(b"InFile"), Some(Object::Bool(true))))
}

/// Make the sticky note at `(page, index)` a Markdown note kept in `file` alone (its text is not
/// copied into the PDF), and draw its icon.
pub fn mark(doc: &mut Document, page: usize, index: usize, file: &str) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    let file: String = file.chars().take(MAX_PATH).collect();
    doc.update_dict(r, |d| {
        let mut n = Dict::new();
        n.set(b"File".to_vec(), PdfString::text(&file));
        n.set(b"InFile".to_vec(), Object::Bool(true));
        d.set(NOTE.to_vec(), Object::Dict(n));
        d.set(b"Subj".to_vec(), PdfString::text("Note"));
    })?;
    set_appearance(doc, r)
}

/// Points the Markdown note at `(page, index)` at `file` (relative to the PDF's folder), keeping
/// whether its text is in the file alone. Used when a rename moves the file.
pub fn set_file(doc: &mut Document, page: usize, index: usize, file: &str) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    let file: String = file.chars().take(MAX_PATH).collect();
    doc.update_dict(r, |d| {
        let mut n = d.get(NOTE).and_then(Object::as_dict).cloned().unwrap_or_default();
        n.set(b"File".to_vec(), PdfString::text(&file));
        d.set(NOTE.to_vec(), Object::Dict(n));
    })?;
    Ok(())
}

/// Whether the Markdown note at `(page, index)` keeps its text only in its file.
pub fn in_file_at(doc: &Document, page: usize, index: usize) -> bool {
    let Ok(p) = page_ref(doc, page) else { return false };
    annots(doc, p).get(index).map(|e| doc.resolve(e)).and_then(|o| o.as_dict().map(in_file_of)).unwrap_or(false)
}

/// The Markdown notes on `page`: (index, file).
pub fn notes(doc: &Document, page: usize) -> Vec<(usize, String)> {
    let Ok(p) = page_ref(doc, page) else { return Vec::new() };
    annots(doc, p)
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let obj = doc.resolve(e);
            let d = obj.as_dict()?;
            (d.name(b"Subtype") == Some(b"Text")).then_some(())?;
            Some((i, file_of(d)?))
        })
        .collect()
}

/// The note icon: a page with a folded corner, its lines, and a heading mark.
pub(crate) fn icon(col: crate::Rgb) -> String {
    let n = crate::n;
    let mut s = format!("{} {} {} rg 0.2 0.2 0.2 RG 0.8 w 1 j 1 J\n", n(col[0]), n(col[1]), n(col[2]));
    // The page, its top-right corner folded.
    s.push_str("3.5 1.5 m 16.5 1.5 l 16.5 14 l 12 18.5 l 3.5 18.5 l h B\n");
    s.push_str("12 18.5 m 12 14 l 16.5 14 l S\n");
    // A heading mark (#) and two lines of text.
    s.push_str("1 1 1 RG 1.1 w 6 13.5 m 6 9.5 l 8.5 13.5 m 8.5 9.5 l 5 12.5 m 9.5 12.5 l 5 10.5 m 9.5 10.5 l S\n");
    s.push_str("6 6.5 m 14 6.5 l 6 4 m 12 4 l S\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_flagged_in_file_keeps_its_text_out_of_the_pdf() {
        let mut new = Dict::new();
        let mut n = Dict::new();
        n.set(b"File".to_vec(), PdfString::text("a.md"));
        n.set(b"InFile".to_vec(), Object::Bool(true));
        new.set(NOTE.to_vec(), Object::Dict(n));
        assert!(in_file_of(&new));
        // An older note has no flag, so its copy in /Contents is still its text.
        let mut old = Dict::new();
        let mut m = Dict::new();
        m.set(b"File".to_vec(), PdfString::text("a.md"));
        old.set(NOTE.to_vec(), Object::Dict(m));
        assert!(!in_file_of(&old));
        assert!(!in_file_of(&Dict::new()));
    }

    #[test]
    fn a_note_names_its_file() {
        let mut d = Dict::new();
        assert_eq!(file_of(&d), None);
        let mut n = Dict::new();
        n.set(b"File".to_vec(), PdfString::text("Plan/notes/Note 1.md"));
        d.set(NOTE.to_vec(), Object::Dict(n));
        assert_eq!(file_of(&d).as_deref(), Some("Plan/notes/Note 1.md"));
        assert!(icon([0.1, 0.5, 0.6]).contains(" B\n"));
    }
}
