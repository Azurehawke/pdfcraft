//! RevuCraft: measurement names (`Kitchen_floor`, `Area_3`), kept in the annotation as
//! `/PCName`. A name is given when the measurement is drawn and can be changed; Send quantities
//! writes each measurement to a workbook's Takeoff sheet with its value as a named cell, so a
//! budget says `=Kitchen_floor * 6.50`.
//!
//! Names follow a spreadsheet's rules for defined names: a letter or underscore first, then
//! letters, digits, underscores and full stops; nothing that reads as a cell (`AB12`, `R1C1`);
//! unique in the document, ignoring case. Names typed or read from files are cleaned to fit.

use std::collections::HashSet;

use pdfcraft_cos::{Document, Object, PdfString};

use crate::{Kind, Result, annot, invalid, list};

/// The annotation key holding the name.
pub const KEY: &[u8] = b"PCName";
/// The longest name kept (characters).
pub const MAX_NAME: usize = 80;
/// Numbered names tried at most before giving up on a base.
const MAX_NUMBER: usize = 1_000_000;

/// `s` made a valid name: other characters become `_`, runs of `_` are joined and trimmed, and
/// `_` goes first when it would start with a digit or read as a cell. `None` when nothing is left.
pub fn clean(s: &str) -> Option<String> {
    let mut out = String::new();
    for c in s.trim().chars() {
        let keep = c.is_alphanumeric() || c == '_' || c == '.';
        let c = if keep { c } else { '_' };
        if c == '_' && out.ends_with('_') {
            continue;
        }
        out.push(c);
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        return None;
    }
    let mut name = trimmed.to_string();
    let first = name.chars().next().unwrap_or('_');
    let reserved = name.to_ascii_lowercase().starts_with("_xlnm") || name.eq_ignore_ascii_case("true") || name.eq_ignore_ascii_case("false");
    if !(first.is_alphabetic() || first == '_') || reads_as_cell(&name) || reserved {
        name.insert(0, '_');
    }
    Some(name.chars().take(MAX_NAME).collect())
}

/// Whether `s` reads as a cell reference: A1 style (up to `XFD1048576`) or R1C1 style
/// (`R`, `C`, `R2`, `C3`, `R2C3`).
fn reads_as_cell(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    let letters: String = upper.chars().take_while(char::is_ascii_alphabetic).collect();
    let rest = upper.get(letters.len()..).unwrap_or_default();
    let a1 = (1..=3).contains(&letters.len())
        && (letters.len() < 3 || letters.as_str() <= "XFD")
        && !rest.is_empty()
        && rest.chars().all(|c| c.is_ascii_digit())
        && rest.parse::<u64>().is_ok_and(|n| (1..=1_048_576).contains(&n));
    let digits = |t: &str| t.chars().all(|c| c.is_ascii_digit());
    let r1c1 = match upper.strip_prefix('R') {
        Some(after) => match after.split_once('C') {
            Some((r, c)) => digits(r) && digits(c),
            None => digits(after),
        },
        None => upper.strip_prefix('C').is_some_and(digits),
    };
    a1 || r1c1
}

/// The names in use, lower-cased, leaving out the measurement at `skip`.
fn taken(doc: &Document, skip: Option<(usize, usize)>) -> HashSet<String> {
    list(doc).measurements.into_iter().filter(|m| Some((m.page, m.index)) != skip && !m.name.is_empty()).map(|m| m.name.to_lowercase()).collect()
}

/// `base` itself when free and not `numbered`, else `base_2`, `base_3`… (`base_1` first when
/// `numbered`).
pub fn next_name(base: &str, numbered: bool, taken: &HashSet<String>) -> String {
    if !numbered && !taken.contains(&base.to_lowercase()) {
        return base.to_string();
    }
    let first = if numbered { 1 } else { 2 };
    for n in first..MAX_NUMBER {
        let suffix = format!("_{n}");
        let room = MAX_NAME.saturating_sub(suffix.chars().count());
        let name: String = base.chars().take(room).chain(suffix.chars()).collect();
        if !taken.contains(&name.to_lowercase()) {
            return name;
        }
    }
    format!("_{base}")
}

/// What a new measurement is called: its label when it has one, else its kind, numbered.
pub fn default_base(kind: Kind, cable: bool, label: &str) -> (String, bool) {
    if let Some(name) = clean(label) {
        return (name, false);
    }
    let base = match kind {
        Kind::Distance => "Distance",
        Kind::Perimeter if cable => "Cable_run",
        Kind::Perimeter => "Perimeter",
        Kind::Area => "Area",
    };
    (base.to_string(), true)
}

/// The name a new measurement gets in `doc`.
pub(crate) fn new_name(doc: &Document, kind: Kind, label: &str) -> String {
    let (base, numbered) = default_base(kind, false, label);
    next_name(&base, numbered, &taken(doc, None))
}

/// Whether `name` is one given by default to a measurement of `kind` (`Perimeter_4`).
fn is_default(name: &str, base: &str) -> bool {
    name.strip_prefix(base).and_then(|r| r.strip_prefix('_')).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// The annotation's Locked flag (ISO 32000-2 §12.5.3).
fn locked(d: &pdfcraft_cos::Dict) -> bool {
    d.get(b"F").and_then(Object::as_int).is_some_and(|f| f & 128 != 0)
}

/// Name the measurement at `(page, index)` `name` (cleaned); returns the name kept. Refused
/// when another measurement has it.
pub fn set_name(doc: &mut Document, page: usize, index: usize, name: &str) -> Result<String> {
    let name = clean(name).ok_or_else(|| invalid("a measurement name needs a letter or a digit"))?;
    let (r, mut d) = annot(doc, page, index)?;
    if !matches!(d.name(b"IT"), Some(b"LineDimension" | b"PolyLineDimension" | b"PolygonDimension")) {
        return Err(invalid("only measurements have names"));
    }
    if locked(&d) {
        return Err(invalid("the measurement is locked"));
    }
    if taken(doc, Some((page, index))).contains(&name.to_lowercase()) {
        return Err(invalid(&format!("another measurement is already called {name}")));
    }
    d.set(KEY.to_vec(), PdfString::text(&name));
    doc.update_dict(r, |target| *target = d).map_err(|e| invalid(&e.to_string()))?;
    Ok(name)
}

/// A cable run named by default as a perimeter (`Perimeter_4`) becomes `Cable_run_n`.
pub(crate) fn rename_cable(doc: &mut Document, page: usize, index: usize) -> Result<()> {
    let (r, mut d) = annot(doc, page, index)?;
    let current = d.get(KEY).and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
    if !current.is_empty() && !is_default(&current, "Perimeter") {
        return Ok(());
    }
    let name = next_name("Cable_run", true, &taken(doc, Some((page, index))));
    d.set(KEY.to_vec(), PdfString::text(&name));
    doc.update_dict(r, |target| *target = d).map_err(|e| invalid(&e.to_string()))
}

/// Gives every measurement without a name (drawn before names, or by another app) one, and
/// renames the later of two with the same name (a copy). Returns how many were named.
pub fn name_all(doc: &mut Document) -> Result<usize> {
    let listing = list(doc);
    let mut seen: HashSet<String> = HashSet::new();
    let mut todo = Vec::new();
    for m in &listing.measurements {
        let cleaned = clean(&m.name);
        match cleaned {
            Some(n) if n == m.name && seen.insert(n.to_lowercase()) => {}
            _ => todo.push(m),
        }
    }
    // Every good name is reserved before new ones are made, so none is taken twice.
    let mut taken = seen;
    let mut named = 0;
    for m in todo {
        let (base, numbered) = match clean(&m.name) {
            Some(n) => (n, false),
            None => default_base(m.reading.kind, m.cable, &m.label),
        };
        let name = next_name(&base, numbered, &taken);
        let Ok((r, mut d)) = annot(doc, m.page, m.index) else { continue };
        if locked(&d) {
            continue;
        }
        taken.insert(name.to_lowercase());
        d.set(KEY.to_vec(), PdfString::text(&name));
        doc.update_dict(r, |target| *target = d).map_err(|e| invalid(&e.to_string()))?;
        named += 1;
    }
    Ok(named)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_to_a_spreadsheets_rules() {
        assert_eq!(clean("Kitchen floor").as_deref(), Some("Kitchen_floor"));
        assert_eq!(clean("  Room 101 / east  ").as_deref(), Some("Room_101_east"));
        assert_eq!(clean("101 Lobby").as_deref(), Some("_101_Lobby"), "a digit can't go first");
        assert_eq!(clean("AB12").as_deref(), Some("_AB12"), "reads as a cell");
        assert_eq!(clean("XFD1048576").as_deref(), Some("_XFD1048576"));
        assert_eq!(clean("XFE1").as_deref(), Some("XFE1"), "past the last column");
        assert_eq!(clean("R2C3").as_deref(), Some("_R2C3"));
        assert_eq!(clean("r").as_deref(), Some("_r"));
        assert_eq!(clean("Roof").as_deref(), Some("Roof"));
        assert_eq!(clean("Área_sótano").as_deref(), Some("Área_sótano"), "letters of any language");
        assert_eq!(clean("TRUE").as_deref(), Some("_TRUE"));
        assert_eq!(clean("  /// "), None);
        assert_eq!(clean(&"x".repeat(300)).map(|n| n.chars().count()), Some(MAX_NAME));
    }

    #[test]
    fn numbered_names_skip_the_taken_ones_ignoring_case() {
        let taken: HashSet<String> = ["area_1", "area_2", "kitchen"].into_iter().map(String::from).collect();
        assert_eq!(next_name("Area", true, &taken), "Area_3");
        assert_eq!(next_name("Kitchen", false, &taken), "Kitchen_2");
        assert_eq!(next_name("Hall", false, &taken), "Hall");
        assert!(is_default("Perimeter_12", "Perimeter") && !is_default("Perimeter_east", "Perimeter") && !is_default("Perimeter_", "Perimeter"));
    }
}
