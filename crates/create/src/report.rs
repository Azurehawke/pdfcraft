//! RevuCraft's summary report: markups, measurements, statuses, your own columns and photos as a
//! new PDF, grouped (by layer) with a summary at the top; the customer report for an inspection
//! or a drop schedule for a network job. The caller composes the entries (in the user's language);
//! this lays them out on Letter pages with the photos embedded.

use pdfcraft_cos::{Dict, Document, Object};
use pdfcraft_fonts::{helvetica_width, literal, win_ansi, wrap};

use crate::{CreateError, LETTER, add_page, image_xobject, set_title};

const MARGIN: f64 = 54.0;
const PHOTO_W: f64 = 300.0;
const PHOTO_H: f64 = 220.0;
/// Entries reported at most, and photos embedded at most.
const MAX_ENTRIES: usize = 20_000;
const MAX_PHOTOS: usize = 2_000;

/// One markup in the report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportEntry {
    /// The group it is listed under (its layer).
    pub group: String,
    /// Its first line: number, type and label.
    pub heading: String,
    /// Short facts, written on one line separated by dots (page, author, value, status, fields).
    pub details: Vec<String>,
    /// Its comment or description.
    pub text: String,
    /// Its photo (a JPEG), if it has one.
    pub photo: Option<Vec<u8>>,
}

/// A page being filled: its content and the pictures it uses.
struct Sheet {
    content: Vec<u8>,
    images: Vec<(String, pdfcraft_cos::ObjRef)>,
}

struct Writer {
    sheets: Vec<Sheet>,
    y: f64,
    width: f64,
    height: f64,
}

impl Writer {
    fn sheet(&mut self) -> &mut Sheet {
        if self.sheets.is_empty() {
            self.sheets.push(Sheet { content: Vec::new(), images: Vec::new() });
        }
        // Not empty: a sheet was just added when there was none.
        let last = self.sheets.len().saturating_sub(1);
        &mut self.sheets[last]
    }

    /// Room for `h` points, else a new page.
    fn room(&mut self, h: f64) {
        if self.sheets.is_empty() || self.y - h < MARGIN {
            self.sheets.push(Sheet { content: Vec::new(), images: Vec::new() });
            self.y = self.height - MARGIN;
        }
    }

    fn line(&mut self, font: &str, size: f64, gray: f64, text: &str) {
        self.room(size * 1.35);
        self.y -= size * 1.35;
        let y = self.y;
        let s = self.sheet();
        s.content.extend_from_slice(format!("{gray:.2} g BT /{font} {size:.2} Tf {MARGIN:.2} {:.2} Td ", y + size * 0.3).as_bytes());
        s.content.extend_from_slice(&literal(&win_ansi(text)));
        s.content.extend_from_slice(b" Tj ET\n");
    }

    fn para(&mut self, font: &str, size: f64, gray: f64, text: &str) {
        for l in wrap(text, size, self.width - 2.0 * MARGIN) {
            self.line(font, size, gray, &l);
        }
    }

    fn rule(&mut self) {
        self.room(6.0);
        self.y -= 4.0;
        let (y, w) = (self.y, self.width);
        self.sheet().content.extend_from_slice(format!("0.7 G 0.5 w {MARGIN:.2} {y:.2} m {:.2} {y:.2} l S\n", w - MARGIN).as_bytes());
        self.y -= 2.0;
    }

    fn gap(&mut self, h: f64) {
        self.y -= h;
    }

    fn photo(&mut self, doc: &mut Document, jpeg: &[u8], count: &mut usize) {
        if *count >= MAX_PHOTOS {
            return;
        }
        let Ok((image, (w, h))) = image_xobject(doc, "photo.jpg", jpeg) else { return };
        if !(w > 0.0 && h > 0.0) {
            return;
        }
        let k = (PHOTO_W / w).min(PHOTO_H / h);
        let (dw, dh) = (w * k, h * k);
        self.room(dh + 6.0);
        self.y -= dh + 4.0;
        let y = self.y;
        *count += 1;
        let name = format!("Im{count}");
        let s = self.sheet();
        s.content.extend_from_slice(format!("q {dw:.2} 0 0 {dh:.2} {MARGIN:.2} {y:.2} cm /{name} Do Q\n").as_bytes());
        s.images.push((name, image));
        self.y -= 2.0;
    }
}

fn font(base: &str) -> Object {
    let mut f = Dict::new();
    f.set(b"Type".to_vec(), Object::name("Font"));
    f.set(b"Subtype".to_vec(), Object::name("Type1"));
    f.set(b"BaseFont".to_vec(), Object::name(base));
    f.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
    Object::Dict(f)
}

/// The report as a new document: `title` and `subtitle` at the top, then `summary` lines, then
/// the entries under their groups (in the order given). `page_label` writes the footer
/// ("Page {n} of {total}").
pub fn report(
    title: &str,
    subtitle: &str,
    summary: &[String],
    entries: &[ReportEntry],
    page_label: impl Fn(usize, usize) -> String,
) -> Result<Document, CreateError> {
    let (width, height) = LETTER;
    let mut doc = Document::new_empty();
    let mut w = Writer { sheets: Vec::new(), y: height - MARGIN, width, height };
    w.para("F2", 16.0, 0.0, title);
    w.para("F1", 9.0, 0.35, subtitle);
    w.gap(6.0);
    for s in summary {
        w.para("F1", 10.0, 0.0, s);
    }
    let mut photos = 0usize;
    let mut group: Option<&str> = None;
    for e in entries.iter().take(MAX_ENTRIES) {
        if group != Some(e.group.as_str()) {
            group = Some(e.group.as_str());
            w.gap(10.0);
            w.room(40.0);
            w.para("F2", 13.0, 0.0, &e.group);
            w.rule();
        }
        w.gap(6.0);
        w.room(30.0);
        w.para("F2", 10.5, 0.0, &e.heading);
        let details: Vec<&str> = e.details.iter().map(String::as_str).filter(|d| !d.is_empty()).collect();
        if !details.is_empty() {
            w.para("F1", 9.0, 0.35, &details.join("  ·  "));
        }
        if !e.text.trim().is_empty() {
            w.para("F1", 10.0, 0.0, e.text.trim());
        }
        if let Some(jpeg) = &e.photo {
            w.photo(&mut doc, jpeg, &mut photos);
        }
    }
    if w.sheets.is_empty() {
        w.sheets.push(Sheet { content: Vec::new(), images: Vec::new() });
    }
    let f1 = doc.add(font("Helvetica"));
    let f2 = doc.add(font("Helvetica-Bold"));
    let total = w.sheets.len();
    for (i, sheet) in w.sheets.into_iter().enumerate() {
        let mut content = sheet.content;
        let footer = page_label(i + 1, total);
        let fw = helvetica_width(&footer, 8.0);
        content.extend_from_slice(format!("0.45 g BT /F1 8 Tf {:.2} {:.2} Td ", width - MARGIN - fw, MARGIN / 2.0).as_bytes());
        content.extend_from_slice(&literal(&win_ansi(&footer)));
        content.extend_from_slice(b" Tj ET\n");
        let mut fonts = Dict::new();
        fonts.set(b"F1".to_vec(), Object::Ref(f1));
        fonts.set(b"F2".to_vec(), Object::Ref(f2));
        let mut res = Dict::new();
        res.set(b"Font".to_vec(), Object::Dict(fonts));
        if !sheet.images.is_empty() {
            let mut xo = Dict::new();
            for (name, r) in sheet.images {
                xo.set(name.into_bytes(), Object::Ref(r));
            }
            res.set(b"XObject".to_vec(), Object::Dict(xo));
        }
        add_page(&mut doc, width, height, res, Some(content))?;
    }
    set_title(&mut doc, title);
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_cos::{SaveOptions, write_full};
    use std::sync::Arc;

    fn jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(40, 30, image::Rgb([10, 120, 200]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg).unwrap();
        out
    }

    #[test]
    fn a_report_lists_groups_and_embeds_photos() {
        let mut entries = Vec::new();
        for i in 0..60 {
            entries.push(ReportEntry {
                group: if i < 30 { "Data".into() } else { "Photos".into() },
                heading: format!("{} Data jack — PC-{i}", i + 1),
                details: vec!["Page 1".into(), "Progress: Pulled".into(), String::new()],
                text: "Behind the desk, left of the window.".into(),
                photo: (i % 20 == 0).then(jpeg),
            });
        }
        // A photo that isn't one is left out, not a failure.
        entries.push(ReportEntry { group: "Photos".into(), heading: "Broken".into(), photo: Some(b"nope".to_vec()), ..Default::default() });
        let doc = report("plan.pdf — Markup report", "2026-10-16", &["61 markups".into()], &entries, |n, t| format!("Page {n} of {t}")).unwrap();
        let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
        let back = Document::open(Arc::new(bytes)).unwrap();
        let pages = pdfcraft_model::pages(&back);
        assert!(pages.len() >= 3, "{} pages", pages.len());
        // Without object streams, so the page resources are readable as text.
        let plain = SaveOptions { object_streams: false, ..SaveOptions::default() };
        let text = String::from_utf8_lossy(&write_full(&back, &plain).unwrap()).to_string();
        assert!(text.contains("/Im1"), "the photo is drawn");
        // Nothing at all still makes a page.
        assert!(report("Empty", "", &[], &[], |n, t| format!("{n}/{t}")).is_ok());
    }
}
