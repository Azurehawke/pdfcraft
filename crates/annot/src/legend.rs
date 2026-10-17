//! RevuCraft's legends: a box on the sheet listing every symbol used (on that page, or in the
//! whole document) with its picture, name and count. It is a custom stamp like a symbol, marked
//! `/PCLegend`, and it keeps itself up to date: after every change the engine calls
//! [`refresh_legends`], which redraws a legend only when what it lists has changed.

use pdfcraft_cos::{Dict, Document, Object, PdfString, Stream};

use crate::symbols::{Symbol, placed};
use crate::{AnnotError, Meta, NewAnnotation, Rgb, Shape, Style, add_annotation, annot_ref, annots, n, page_refs};

/// Layout in points (a legend is drawn at paper size).
const PAD: f64 = 6.0;
const ROW: f64 = 16.0;
const ICON: f64 = 12.0;
const TEXT: f64 = 8.5;
const TITLE: f64 = 10.0;
/// Rows listed at most (symbol kinds).
const MAX_ROWS: usize = 60;
/// Legends refreshed at most per change.
const MAX_LEGENDS: usize = 200;

/// One line of a legend: a symbol kind, how many, and how it is drawn.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: String,
    name: String,
    count: usize,
    art: String,
    color: Rgb,
}

fn rows(doc: &Document, page: Option<usize>) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for p in placed(doc).into_iter().filter(|p| page.is_none_or(|pg| pg == p.page)) {
        if let Some(r) = out.iter_mut().find(|r| r.id == p.symbol) {
            r.count += 1;
            continue;
        }
        if out.len() >= MAX_ROWS {
            continue;
        }
        let Some(d) = crate::photo_callout::dict_at(doc, p.page, p.index) else { continue };
        let art = match Symbol::from_id(&p.symbol) {
            Some(s) => s.artwork(),
            None => match d.get(b"PCArt").map(|o| doc.resolve(o)).as_deref() {
                Some(Object::Stream(s)) => s.decoded_within(crate::svg_art::MAX_ART).ok().and_then(|b| String::from_utf8(b).ok()).unwrap_or_default(),
                _ => String::new(),
            },
        };
        let color = match d.get(b"C").and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).collect::<Vec<_>>()).as_deref() {
            Some([r, g, b]) => [*r, *g, *b].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }),
            _ => [0.0; 3],
        };
        out.push(Row { id: p.symbol.clone(), name: crate::symbols::display_name(&p.symbol), count: 1, art, color });
    }
    out
}

/// What a legend lists, as text: it is redrawn only when this changes.
fn key(rows: &[Row]) -> String {
    rows.iter().map(|r| format!("{}={}@{:.3},{:.3},{:.3}", r.id, r.count, r.color[0], r.color[1], r.color[2])).collect::<Vec<_>>().join(";")
}

fn font(base: &str) -> Object {
    let mut f = Dict::new();
    f.set(b"Type".to_vec(), Object::name("Font"));
    f.set(b"Subtype".to_vec(), Object::name("Type1"));
    f.set(b"BaseFont".to_vec(), Object::name(base));
    f.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
    Object::Dict(f)
}

fn text(c: &mut Vec<u8>, font: &str, size: f64, x: f64, y: f64, s: &str) {
    c.extend_from_slice(format!("BT /{font} {} Tf {} {} Td ", n(size), n(x), n(y)).as_bytes());
    c.extend_from_slice(&pdfcraft_fonts::literal(&pdfcraft_fonts::win_ansi(s)));
    c.extend_from_slice(b" Tj ET\n");
}

/// The legend's picture: a form XObject, and its width and height in points.
fn picture(title: &str, rows: &[Row]) -> (Stream, f64, f64) {
    let empty = "No symbols yet";
    let name_w = rows.iter().map(|r| pdfcraft_fonts::helvetica_width(&r.name, TEXT)).fold(pdfcraft_fonts::helvetica_width(empty, TEXT), f64::max);
    let count_w = rows.iter().map(|r| pdfcraft_fonts::helvetica_width(&r.count.to_string(), TEXT)).fold(0.0, f64::max);
    let title_w = pdfcraft_fonts::helvetica_width(title, TITLE) * 1.06;
    let width = (PAD + ICON + 6.0 + name_w + 12.0 + count_w + PAD).max(title_w + 2.0 * PAD).max(90.0);
    let lines = rows.len().max(1) as f64;
    let height = PAD + TITLE + 6.0 + lines * ROW + PAD;
    let mut c: Vec<u8> = Vec::new();
    // The box: white, with a thin border.
    c.extend_from_slice(format!("q 1 g 0 G 0.75 w 0.375 0.375 {} {} re B Q\n", n(width - 0.75), n(height - 0.75)).as_bytes());
    c.extend_from_slice(b"0 g\n");
    text(&mut c, "F1", TITLE, PAD, height - PAD - TITLE, title);
    let rule_y = height - PAD - TITLE - 3.0;
    c.extend_from_slice(format!("q 0 G 0.5 w {} {} m {} {} l S Q\n", n(PAD), n(rule_y), n(width - PAD), n(rule_y)).as_bytes());
    if rows.is_empty() {
        text(&mut c, "F2", TEXT, PAD, rule_y - ROW + 4.0, empty);
    }
    for (i, r) in rows.iter().enumerate() {
        let y = rule_y - 3.0 - (i as f64 + 1.0) * ROW + (ROW - ICON) / 2.0;
        // The symbol, drawn at icon size in its colour.
        let k = ICON / 100.0;
        let col = r.color.map(n).join(" ");
        c.extend_from_slice(format!("q {col} RG {col} rg {} 0 0 {} {} {} cm 6 w 1 J 1 j\n", n(k), n(k), n(PAD), n(y)).as_bytes());
        c.extend_from_slice(r.art.as_bytes());
        c.extend_from_slice(b"\nQ\n0 g\n");
        text(&mut c, "F2", TEXT, PAD + ICON + 6.0, y + 2.5, &r.name);
        let count = r.count.to_string();
        let cw = pdfcraft_fonts::helvetica_width(&count, TEXT);
        text(&mut c, "F1", TEXT, width - PAD - cw, y + 2.5, &count);
    }
    let mut fonts = Dict::new();
    fonts.set(b"F1".to_vec(), font("Helvetica-Bold"));
    fonts.set(b"F2".to_vec(), font("Helvetica"));
    let mut res = Dict::new();
    res.set(b"Font".to_vec(), Object::Dict(fonts));
    let mut d = Dict::new();
    d.set(b"Type".to_vec(), Object::name("XObject"));
    d.set(b"Subtype".to_vec(), Object::name("Form"));
    d.set(b"FormType".to_vec(), Object::Int(1));
    d.set(b"BBox".to_vec(), Object::Array([0.0, 0.0, width, height].iter().map(|v| Object::Real(*v)).collect()));
    d.set(b"Resources".to_vec(), Object::Dict(res));
    (Stream::flate(d, &c), width, height)
}

/// Place a legend on `page` with its top-left corner at `top_left` (user space), listing the
/// symbols on that page, or in the whole document with `whole_document`. Returns its index.
pub fn add_legend(
    doc: &mut Document,
    page: usize,
    top_left: [f64; 2],
    whole_document: bool,
    title: &str,
    author: &str,
    meta: &Meta,
) -> Result<usize, AnnotError> {
    if !top_left.iter().all(|v| v.is_finite() && v.abs() < 1e7) {
        return Err(AnnotError::Invalid("invalid legend position".into()));
    }
    let title = crate::symbols::clean_label(title);
    let title = if title.is_empty() { "Legend".to_string() } else { title };
    let list = rows(doc, (!whole_document).then_some(page));
    let (stream, width, height) = picture(&title, &list);
    let pic = doc.add(Object::Stream(stream));
    let rect = [top_left[0], top_left[1] - height, top_left[0] + width, top_left[1]];
    let shape = Shape::CustomStamp { rect, name: "Legend".into(), picture: pic, image: false, size: (width, height) };
    let new = NewAnnotation { page, style: Style::default_for(&shape), shape, contents: title.clone(), author: author.to_string() };
    let index = add_annotation(doc, &new, meta)?;
    let (_, r) = annot_ref(doc, page, index)?;
    doc.update_dict(r, |d| {
        d.set(b"Subj".to_vec(), PdfString::text("Legend"));
        d.set(b"PCLegend".to_vec(), Object::Bool(true));
        d.set(b"PCLegendTitle".to_vec(), PdfString::text(&title));
        d.set(b"PCLegendScope".to_vec(), Object::name(if whole_document { "Document" } else { "Page" }));
        d.set(b"PCLegendKey".to_vec(), PdfString::text(&key(&list)));
    })?;
    Ok(index)
}

/// Redraw every legend whose list has changed (keeping its top-left corner and scale). Returns
/// whether any was redrawn.
pub fn refresh_legends(doc: &mut Document) -> Result<bool, AnnotError> {
    let mut found: Vec<(usize, usize)> = Vec::new();
    for (page, p) in page_refs(doc).unwrap_or_default().into_iter().enumerate() {
        for (index, entry) in annots(doc, p).iter().enumerate() {
            if found.len() >= MAX_LEGENDS {
                break;
            }
            let obj = doc.resolve(entry);
            if obj.as_dict().is_some_and(|d| matches!(d.get(b"PCLegend"), Some(Object::Bool(true)))) {
                found.push((page, index));
            }
        }
    }
    if found.is_empty() {
        return Ok(false);
    }
    let mut changed = false;
    for (page, index) in found {
        let (_, r) = annot_ref(doc, page, index)?;
        let d = crate::annot_dict(doc, r);
        let whole = d.name(b"PCLegendScope") == Some(b"Document");
        let list = rows(doc, (!whole).then_some(page));
        let new_key = key(&list);
        let old_key = d.get(b"PCLegendKey").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
        if new_key == old_key {
            continue;
        }
        let title = d.get(b"PCLegendTitle").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_else(|| "Legend".into());
        let rect: Vec<f64> = d.get(b"Rect").and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).collect()).unwrap_or_default();
        let [x0, y0, x1, y1] = rect[..] else { continue };
        let old_size: Vec<f64> =
            d.get(b"PCPictureSize").and_then(Object::as_array).map(|a| a.iter().filter_map(Object::as_f64).collect()).unwrap_or_default();
        // Its scale as the user sized it (user units per picture unit).
        let k = match old_size[..] {
            [w, _] if w > 0.0 && w.is_finite() => ((x1 - x0).abs() / w).clamp(0.05, 50.0),
            _ => 1.0,
        };
        let (left, top) = (x0.min(x1), y0.max(y1));
        let (stream, width, height) = picture(&title, &list);
        let pic = doc.add(Object::Stream(stream));
        let new_rect = [left, top - height * k, left + width * k, top];
        doc.update_dict(r, |d| {
            d.set(b"Rect".to_vec(), Object::Array(new_rect.iter().map(|v| Object::Real(*v)).collect()));
            d.set(b"PCPicture".to_vec(), Object::Ref(pic));
            d.set(b"PCPictureSize".to_vec(), Object::Array(vec![Object::Real(width), Object::Real(height)]));
            d.set(b"PCLegendKey".to_vec(), PdfString::text(&new_key));
        })?;
        crate::set_appearance(doc, r)?;
        changed = true;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{NewSymbol, add_symbol};
    use pdfcraft_cos::{SaveOptions, write_full};
    use std::sync::Arc;

    #[test]
    fn a_legend_lists_and_follows_the_symbols() {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        let mut doc = Document::open(Arc::new(pdf.to_vec())).unwrap();
        let jack = NewSymbol {
            page: 0,
            symbol: Symbol::DataJack,
            center: [100.0, 100.0],
            size: 12.0,
            label: String::new(),
            color: [0.0, 0.4, 0.9],
            author: String::new(),
            rotation: 0.0,
            custom: None,
        };
        add_symbol(&mut doc, &jack, &Meta::default()).unwrap();
        let legend = add_legend(&mut doc, 0, [450.0, 380.0], false, "Legend", "Steve", &Meta::default()).unwrap();
        // Nothing changed: nothing redrawn.
        assert!(!refresh_legends(&mut doc).unwrap());
        add_symbol(&mut doc, &NewSymbol { center: [150.0, 100.0], ..jack.clone() }, &Meta::default()).unwrap();
        add_symbol(&mut doc, &NewSymbol { symbol: Symbol::WirelessAp, ..jack.clone() }, &Meta::default()).unwrap();
        assert!(refresh_legends(&mut doc).unwrap());
        let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
        let doc = Document::open(Arc::new(bytes)).unwrap();
        let d = crate::photo_callout::dict_at(&doc, 0, legend).unwrap();
        let key = d.get(b"PCLegendKey").and_then(Object::as_string).map(PdfString::to_text).unwrap();
        assert!(key.contains("=2@") && key.contains("=1@"), "{key}");
        // Its top edge stays where it was put.
        let rect: Vec<f64> = d.get(b"Rect").and_then(Object::as_array).unwrap().iter().filter_map(Object::as_f64).collect();
        assert!((rect[3] - 380.0).abs() < 1e-6);
        assert!(add_legend(&mut Document::open(Arc::new(pdf.to_vec())).unwrap(), 0, [f64::NAN, 0.0], false, "", "", &Meta::default()).is_err());
    }
}
