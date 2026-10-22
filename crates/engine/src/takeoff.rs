//! RevuCraft: the document's quantities for a workbook's Takeoff sheet, and the record of where
//! they were last sent.
//!
//! Every measurement is a row named by its own name ([`crate::measure::names`]); placed symbols
//! are counted by symbol and layer, each count named after its symbol (`Data_jack_count`, with
//! the layer when the symbol is on several). The record (`/PCTakeoff` in the catalog) keeps the
//! workbook's path, where it is from the PDF, its size and date as written, and a fingerprint of
//! the quantities sent, so the app can tell when the drawing has changed since, or the workbook
//! was saved over.

use std::collections::HashSet;

use pdfcraft_cos::{Dict, Document as Cos, Object, PdfString};
pub use pdfcraft_create::takeoff::{EXTENSIONS, SHEET, Sent, Takeoff, TakeoffRow, can_send, new_workbook, write};

use crate::cad_links::LinkedFile;
use crate::measure::{Kind, names};
use crate::{Document, EditError};

/// The catalog key of the record.
const KEY: &[u8] = b"PCTakeoff";

/// Where the quantities were last sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub path: String,
    /// The workbook's path from the PDF (`/`-separated), when both had a place on disk.
    pub relative: Option<String>,
    /// The workbook as written.
    pub size: u64,
    pub mtime: i64,
    /// [`Takeoff::fingerprint`] of what was sent.
    pub sent: String,
    /// When (seconds since 1970).
    pub at: i64,
}

fn invalid(msg: &str) -> EditError {
    EditError::Source(msg.to_string())
}

fn text(d: &Dict, key: &[u8]) -> String {
    d.get(key).and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default()
}

/// The record, if quantities were sent from this document.
pub(crate) fn record(cos: &Cos) -> Option<Record> {
    let root = cos.root()?;
    let cat = cos.get(root);
    let d = cos.resolve(cat.as_dict()?.get(KEY)?);
    let d = d.as_dict()?;
    let path = text(d, b"Path");
    if path.is_empty() {
        return None;
    }
    let int = |k: &[u8]| d.get(k).and_then(|o| cos.resolve(o).as_int()).unwrap_or(0);
    Some(Record {
        path,
        relative: Some(text(d, b"RelPath")).filter(|r| !r.is_empty()),
        size: u64::try_from(int(b"Size")).unwrap_or(0),
        mtime: int(b"MTime"),
        sent: text(d, b"Sent"),
        at: int(b"At"),
    })
}

/// Records that the quantities with fingerprint `sent` were written to `file`.
pub(crate) fn set_record(cos: &mut Cos, file: &LinkedFile, sent: &str) -> Result<(), EditError> {
    if file.path.is_empty() || file.path.len() > 4096 || sent.len() > 64 {
        return Err(invalid("invalid workbook for the quantities"));
    }
    let root = cos.root().ok_or_else(|| invalid("the document has no catalog"))?;
    let mut d = Dict::new();
    d.set(b"Path".to_vec(), PdfString::text(&file.path));
    if let Some(r) = file.relative.as_deref().filter(|r| !r.is_empty() && r.len() <= 4096) {
        d.set(b"RelPath".to_vec(), PdfString::text(r));
    }
    d.set(b"Size".to_vec(), Object::Int(i64::try_from(file.size).unwrap_or(i64::MAX)));
    d.set(b"MTime".to_vec(), Object::Int(file.mtime));
    d.set(b"At".to_vec(), Object::Int(file.now));
    d.set(b"Sent".to_vec(), PdfString::text(sent));
    cos.update_dict(root, |c| c.set(KEY.to_vec(), Object::Dict(d))).map_err(|e| invalid(&e.to_string()))
}

/// A measurement's type as the Takeoff sheet writes it (in English, so formulas that pick rows
/// by type keep working in every language).
fn kind_name(kind: Kind, cable: bool) -> &'static str {
    match kind {
        Kind::Distance => "Distance",
        Kind::Perimeter if cable => "Cable run",
        Kind::Perimeter => "Perimeter",
        Kind::Area => "Area",
    }
}

impl Document {
    /// The document's quantities: its measurements, then its symbol counts. `columns` are your
    /// own columns, written after the fixed ones.
    pub fn takeoff(&self, columns: &[String]) -> Takeoff {
        let layers = self.layer_list();
        let layer_name = |l: Option<(u32, u16)>| -> String {
            l.and_then(|l| layers.iter().find(|x| (x.id.num, x.id.generation) == l)).map(|x| x.name.clone()).unwrap_or_default()
        };
        let fields = self.all_annotation_fields();
        let measured = self.measurements().map(|l| l.measurements).unwrap_or_default();
        let mut rows = Vec::with_capacity(measured.len());
        let mut taken: HashSet<String> = HashSet::new();
        for m in measured {
            let layer = self.info.annotations.iter().find(|a| a.page == m.page && a.index == m.index).and_then(|a| a.layer);
            let own = fields.iter().find(|(p, i, _)| *p == m.page && *i == m.index).map(|(_, _, f)| f.as_slice()).unwrap_or_default();
            let values = columns.iter().map(|c| own.iter().find(|(n, _)| n == c).map(|(_, v)| v.clone()).unwrap_or_default()).collect();
            taken.insert(m.name.to_lowercase());
            rows.push(TakeoffRow {
                name: m.name,
                kind: kind_name(m.reading.kind, m.cable).to_string(),
                label: m.label,
                layer: layer_name(layer),
                page: Some(m.page.saturating_add(1)),
                quantity: m.reading.value,
                unit: m.reading.unit,
                shown: m.reading.label,
                fields: values,
            });
        }
        // Symbol counts by symbol and layer, in the order they first appear.
        let placed = self.editor.as_ref().map(|e| pdfcraft_annot::symbols::placed(&e.cos)).unwrap_or_default();
        let mut groups: Vec<(String, Option<pdfcraft_cos::ObjRef>, usize)> = Vec::new();
        for p in &placed {
            match groups.iter_mut().find(|g| g.0 == p.symbol && g.1 == p.layer) {
                Some(g) => g.2 = g.2.saturating_add(1),
                None => groups.push((p.symbol.clone(), p.layer, 1)),
            }
        }
        for (symbol, layer, count) in &groups {
            let shown = pdfcraft_annot::symbols::display_name(symbol);
            let layer_text = layer_name(layer.map(|l| (l.num, l.generation)));
            let several = groups.iter().filter(|g| g.0 == *symbol).count() > 1;
            let base = if several && !layer_text.is_empty() { format!("{shown} count {layer_text}") } else { format!("{shown} count") };
            let base = names::clean(&base).unwrap_or_else(|| "Count".into());
            let name = names::next_name(&base, false, &taken);
            taken.insert(name.to_lowercase());
            rows.push(TakeoffRow {
                name,
                kind: shown,
                layer: layer_text,
                quantity: *count as f64,
                unit: "ea".into(),
                shown: count.to_string(),
                fields: vec![String::new(); columns.len()],
                ..TakeoffRow::default()
            });
        }
        Takeoff { columns: columns.to_vec(), rows }
    }

    /// Whether a measurement has no name, or one another has too: Send quantities names them
    /// first ([`crate::Edit::NameMeasurements`]).
    pub fn measurements_need_names(&self) -> bool {
        let Ok(listing) = self.measurements() else { return false };
        let mut seen = HashSet::new();
        listing.measurements.iter().any(|m| names::clean(&m.name).as_deref() != Some(m.name.as_str()) || !seen.insert(m.name.to_lowercase()))
    }

    /// How the stroke of the shape at (`page`, `index`) lies against its path; `None` when it
    /// isn't a closed shape.
    pub fn stroke_align(&self, page: usize, index: usize) -> Option<pdfcraft_annot::stroke_align::StrokeAlign> {
        let e = self.editor.as_ref()?;
        let d = pdfcraft_annot::photo_callout::dict_at(&e.cos, page, index)?;
        pdfcraft_annot::stroke_align::effective(&d)
    }

    /// The name of the measurement at (`page`, `index`); `None` when it isn't a measurement.
    pub fn measurement_name(&self, page: usize, index: usize) -> Option<String> {
        let e = self.editor.as_ref()?;
        let d = pdfcraft_annot::photo_callout::dict_at(&e.cos, page, index)?;
        matches!(d.name(b"IT"), Some(b"LineDimension" | b"PolyLineDimension" | b"PolygonDimension")).then(|| text(&d, names::KEY))
    }

    /// Where the quantities were last sent, if they were.
    pub fn takeoff_record(&self) -> Option<Record> {
        self.editor.as_ref().and_then(|e| record(&e.cos))
    }
}
