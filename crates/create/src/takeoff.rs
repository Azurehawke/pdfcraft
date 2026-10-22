//! RevuCraft: quantities sent to a workbook's Takeoff sheet.
//!
//! Every measurement (and symbol count) becomes a row of the sheet named `Takeoff`, and its
//! quantity a named cell (`Kitchen_floor` → `Takeoff!$F$5`), so formulas elsewhere in the
//! workbook use the names and follow the drawing.
//!
//! The workbook is changed surgically, part by part inside its package: the Takeoff sheet's part
//! is written anew (RevuCraft owns that sheet), the workbook part gains the sheet if it is new,
//! the names that point at the Takeoff sheet are replaced, and the workbook is marked to work its
//! formulas out when it is next opened. The calculation chain, which lists formula cells, is
//! dropped for the spreadsheet to rebuild. Every other part (other sheets, styles, charts,
//! slicers, the data model, macros) is copied byte for byte, so nothing a round trip through a
//! spreadsheet model would lose is lost.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};

use gridcraft_engine::core::{CellRef, Value, col_to_letters};
use gridcraft_engine::model::Workbook;

use crate::CreateError;
use crate::table::{Book, MAX_BYTES};

/// The sheet RevuCraft writes.
pub const SHEET: &str = "Takeoff";
/// The workbooks quantities can be sent to (XML spreadsheets).
pub const EXTENSIONS: [&str; 2] = ["xlsx", "xlsm"];
/// Rows written at most.
pub const MAX_ROWS: usize = 100_000;
/// The largest XML part read from a workbook.
const MAX_PART: u64 = 64 << 20;
/// Parts in a workbook package at most.
const MAX_ENTRIES: usize = 100_000;

/// The fixed columns, before your own (custom) columns.
const HEADINGS: [&str; 8] = ["Name", "Type", "Label", "Layer", "Page", "Quantity", "Unit", "Shown"];
/// The column holding the quantity (1-based: F).
const QUANTITY_COL: u32 = 6;

/// One row of the Takeoff sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TakeoffRow {
    /// Its defined name (already valid and unique).
    pub name: String,
    /// What it is: Area, Distance, Cable run, or a symbol's name.
    pub kind: String,
    pub label: String,
    pub layer: String,
    /// The page it is on (1-based); none for a count over several pages.
    pub page: Option<usize>,
    pub quantity: f64,
    pub unit: String,
    /// The value as the drawing shows it (`12'-6 1/2"`).
    pub shown: String,
    /// Values of your own columns, in the order of [`Takeoff::columns`].
    pub fields: Vec<String>,
}

/// The quantities of a document, as the Takeoff sheet holds them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Takeoff {
    /// Your own columns' names.
    pub columns: Vec<String>,
    pub rows: Vec<TakeoffRow>,
}

impl Takeoff {
    /// A short fingerprint of everything written (64-bit FNV-1a, hex), to tell whether the
    /// quantities changed since they were sent.
    pub fn fingerprint(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for b in bytes.iter().chain(std::iter::once(&0x1f)) {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        };
        for c in &self.columns {
            eat(c.as_bytes());
        }
        for r in &self.rows {
            for t in [&r.name, &r.kind, &r.label, &r.layer, &r.unit, &r.shown] {
                eat(t.as_bytes());
            }
            eat(&r.page.unwrap_or(0).to_le_bytes());
            eat(&r.quantity.to_bits().to_le_bytes());
            for f in &r.fields {
                eat(f.as_bytes());
            }
        }
        format!("{h:016x}")
    }
}

/// The workbook after sending, and what couldn't be done (names already used elsewhere).
#[derive(Clone, Debug)]
pub struct Sent {
    pub bytes: Vec<u8>,
    pub notes: Vec<String>,
}

fn invalid(msg: impl Into<String>) -> CreateError {
    CreateError::Invalid(msg.into())
}

/// Whether quantities can be sent to the workbook `name`.
pub fn can_send(name: &str) -> bool {
    std::path::Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// A new workbook holding the quantities: a Budget sheet to work in, and the Takeoff sheet.
pub fn new_workbook(t: &Takeoff) -> Result<Sent, CreateError> {
    let mut wb = Workbook::new();
    if let Some(sheet) = wb.sheet_mut(0) {
        sheet.name = "Budget".into();
    }
    let bytes = gridcraft_engine::io::save_bytes(&wb, "Takeoff.xlsx").map_err(|e| invalid(e.to_string()))?;
    write(&bytes, t)
}

impl Book {
    /// Whether the workbook's Takeoff sheet holds `t`'s names and quantities (a spreadsheet
    /// that had the workbook open may have saved older ones over it).
    pub fn has_takeoff(&self, t: &Takeoff) -> bool {
        let Some(sheet) = self.wb.sheets.iter().find(|s| s.name.eq_ignore_ascii_case(SHEET)) else { return false };
        let at = |row: usize, col: u32| u32::try_from(row).ok().map(|r| sheet.value(CellRef::new(r, col)));
        for (i, r) in t.rows.iter().enumerate() {
            let row = i.saturating_add(1);
            let name_ok = matches!(at(row, 0), Some(Value::Text(s)) if *s == *r.name);
            let quantity_ok = match at(row, QUANTITY_COL - 1) {
                Some(Value::Number(n)) => (n - r.quantity).abs() <= 1e-9 * r.quantity.abs().max(1.0),
                Some(Value::Empty) => !r.quantity.is_finite(),
                _ => false,
            };
            if !name_ok || !quantity_ok {
                return false;
            }
        }
        // No rows left over from more measurements.
        matches!(at(t.rows.len().saturating_add(1), 0), Some(Value::Empty) | None)
    }
}

// --- A small XML tag scanner: enough to find and rewrite elements of the package's parts. ---

/// A tag in an XML text: its byte span (`start..end`, through `>`), qualified name and
/// attribute text.
#[derive(Clone, Copy, Debug)]
struct Tag<'a> {
    start: usize,
    end: usize,
    name: &'a str,
    attrs: &'a str,
    closing: bool,
    empty: bool,
}

impl Tag<'_> {
    fn local(&self) -> &str {
        self.name.rsplit_once(':').map_or(self.name, |(_, l)| l)
    }
    fn prefix(&self) -> &str {
        self.name.rsplit_once(':').map_or("", |(p, _)| p)
    }
    /// The value of the attribute whose local name is `local`, unescaped.
    fn attr(&self, local: &str) -> Option<String> {
        attrs(self.attrs).into_iter().find(|(n, _)| n.rsplit_once(':').map_or(*n, |(_, l)| l) == local).map(|(_, v)| unescape(v))
    }
}

/// The attributes of a tag (name, raw value).
fn attrs(s: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(eq) = rest.find('=') {
        let name = rest.get(..eq).unwrap_or_default().trim();
        let after = rest.get(eq + 1..).unwrap_or_default().trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let body = after.get(1..).unwrap_or_default();
        let Some(close) = body.find(q) else { break };
        out.push((name, body.get(..close).unwrap_or_default()));
        rest = body.get(close + 1..).unwrap_or_default();
    }
    out
}

/// Every element tag in `xml`, in order (comments, processing instructions, declarations and
/// CDATA are skipped).
fn tags(xml: &str) -> Vec<Tag<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = xml.get(i..).and_then(|s| s.find('<')) {
        let start = i + off;
        let rest = xml.get(start..).unwrap_or_default();
        let skip_to = |end: &str| rest.find(end).map(|e| start + e + end.len());
        let next = if rest.starts_with("<!--") {
            skip_to("-->")
        } else if rest.starts_with("<![CDATA[") {
            skip_to("]]>")
        } else if rest.starts_with("<?") {
            skip_to("?>")
        } else if rest.starts_with("<!") {
            skip_to(">")
        } else {
            // Through the closing `>`, outside quoted attribute values.
            let mut quote = None;
            let mut end = None;
            for (j, c) in rest.char_indices().skip(1) {
                match (quote, c) {
                    (None, '"' | '\'') => quote = Some(c),
                    (Some(q), c) if c == q => quote = None,
                    (None, '>') => {
                        end = Some(j);
                        break;
                    }
                    _ => {}
                }
            }
            let Some(j) = end else { break };
            let inner = rest.get(1..j).unwrap_or_default();
            let closing = inner.starts_with('/');
            let inner = inner.trim_start_matches('/');
            let empty = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let name_len = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
            let name = inner.get(..name_len).unwrap_or_default();
            out.push(Tag { start, end: start + j + 1, name, attrs: inner.get(name_len..).unwrap_or_default(), closing, empty });
            Some(start + j + 1)
        };
        match next {
            Some(n) if n > i => i = n,
            _ => break,
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(rest.get(..amp).unwrap_or_default());
        let after = rest.get(amp..).unwrap_or_default();
        let Some(semi) = after.find(';').filter(|s| *s <= 10) else {
            out.push('&');
            rest = after.get(1..).unwrap_or_default();
            continue;
        };
        let entity = after.get(1..semi).unwrap_or_default();
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => e.get(2..).and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32),
            e if e.starts_with('#') => e.get(1..).and_then(|d| d.parse::<u32>().ok()).and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = after.get(semi + 1..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = after.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text or an attribute value escaped for XML, without characters XML can't hold.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

/// A text change at byte `start..end` of a part.
struct Splice {
    start: usize,
    end: usize,
    text: String,
}

/// `src` with the splices made (in order of position; insertions at one place keep their order).
fn splice(src: &str, mut edits: Vec<Splice>) -> Result<String, CreateError> {
    edits.sort_by_key(|e| e.start);
    let mut out = String::with_capacity(src.len() + edits.iter().map(|e| e.text.len()).sum::<usize>());
    let mut at = 0;
    for e in edits {
        if e.start < at || e.end < e.start {
            return Err(invalid("the workbook's structure couldn't be followed"));
        }
        out.push_str(src.get(at..e.start).ok_or_else(|| invalid("the workbook's structure couldn't be followed"))?);
        out.push_str(&e.text);
        at = e.end;
    }
    out.push_str(src.get(at..).unwrap_or_default());
    Ok(out)
}

/// The first start tag named `local`.
fn find<'a>(tags: &[Tag<'a>], local: &str) -> Option<Tag<'a>> {
    tags.iter().find(|t| !t.closing && t.local() == local).copied()
}

/// The end tag matching the start tag `open` (by name, counting nesting).
fn closing<'a>(tags: &[Tag<'a>], open: Tag<'a>) -> Option<Tag<'a>> {
    if open.empty {
        return Some(open);
    }
    let mut depth = 0usize;
    for t in tags.iter().filter(|t| t.start > open.start && t.name == open.name) {
        if t.closing {
            if depth == 0 {
                return Some(*t);
            }
            depth -= 1;
        } else if !t.empty {
            depth += 1;
        }
    }
    None
}

/// A part's path from a relationship's target, relative to the folder of the part that has it
/// (`xl` for `xl/workbook.xml`), or absolute from the package root.
fn resolve(folder: &str, target: &str) -> String {
    let joined = match target.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None if folder.is_empty() => target.to_string(),
        None => format!("{folder}/{target}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for p in joined.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The relationships part of `part` (`xl/_rels/workbook.xml.rels` for `xl/workbook.xml`).
fn rels_of(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((folder, file)) => format!("{folder}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

fn folder_of(part: &str) -> &str {
    part.rsplit_once('/').map_or("", |(f, _)| f)
}

/// The package's parts: read on demand, by name.
struct Package {
    archive: zip::ZipArchive<Cursor<Vec<u8>>>,
    names: Vec<String>,
}

impl Package {
    fn open(bytes: &[u8]) -> Result<Package, CreateError> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid("the workbook is too large"));
        }
        let archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|_| invalid("this isn't an XLSX workbook (it isn't a package)"))?;
        if archive.len() > MAX_ENTRIES {
            return Err(invalid("the workbook has too many parts"));
        }
        let names = archive.file_names().map(str::to_string).collect();
        Ok(Package { archive, names })
    }

    fn has(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
    }

    fn text(&mut self, name: &str) -> Result<String, CreateError> {
        let f = self.archive.by_name(name).map_err(|_| invalid(format!("the workbook has no {name}")))?;
        let mut bytes = Vec::new();
        f.take(MAX_PART).read_to_end(&mut bytes).map_err(|e| invalid(format!("{name}: {e}")))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) >= MAX_PART {
            return Err(invalid(format!("{name} is too large")));
        }
        let text = String::from_utf8(bytes).map_err(|_| invalid(format!("{name} isn't UTF-8 text")))?;
        Ok(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
    }
}

/// The namespaces of a transitional and a strict workbook.
struct Spaces {
    main: &'static str,
    rel: &'static str,
}

const TRANSITIONAL: Spaces = Spaces {
    main: "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    rel: "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
};
const STRICT: Spaces =
    Spaces { main: "http://purl.oclc.org/ooxml/spreadsheetml/main", rel: "http://purl.oclc.org/ooxml/officeDocument/relationships" };

/// Elements of a workbook part that come after `definedNames` and `calcPr`, in schema order.
const AFTER_CALC: [&str; 9] = [
    "oleSize",
    "customWorkbookViews",
    "pivotCaches",
    "smartTagPr",
    "smartTagTypes",
    "webPublishing",
    "fileRecoveryPr",
    "webPublishObjects",
    "extLst",
];
const AFTER_NAMES: [&str; 10] = [
    "calcPr",
    "oleSize",
    "customWorkbookViews",
    "pivotCaches",
    "smartTagPr",
    "smartTagTypes",
    "webPublishing",
    "fileRecoveryPr",
    "webPublishObjects",
    "extLst",
];

/// Where an element goes that must come before every element named in `after`: before the first
/// of them, else before the root's end tag.
fn before_any(tags: &[Tag<'_>], after: &[&str], root_end: usize) -> usize {
    tags.iter().filter(|t| !t.closing && after.contains(&t.local())).map(|t| t.start).min().unwrap_or(root_end)
}

/// Whether a defined name's formula points at the Takeoff sheet.
fn on_takeoff(formula: &str) -> bool {
    let f = formula.trim().trim_start_matches('=').trim_start();
    let lower = f.to_lowercase();
    let sheet = SHEET.to_lowercase();
    lower.starts_with(&format!("{sheet}!")) || lower.starts_with(&format!("'{sheet}'!"))
}

/// The Takeoff sheet's part.
fn sheet_xml(t: &Takeoff, main: &str) -> Result<String, CreateError> {
    if t.rows.len() > MAX_ROWS {
        return Err(invalid("too many quantities to send"));
    }
    let cols = HEADINGS.len().saturating_add(t.columns.len().min(200));
    let col = |i: usize| col_to_letters(u32::try_from(i).unwrap_or(0));
    let mut out = String::with_capacity(256 + t.rows.len() * 200);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    out.push_str(&format!("<worksheet xmlns=\"{main}\">"));
    out.push_str("<sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews>");
    out.push_str("<cols>");
    for i in 0..cols {
        let width = match i {
            0 => 22,
            1..=3 => 16,
            4 => 7,
            5 => 12,
            6 => 9,
            _ => 14,
        };
        out.push_str(&format!("<col min=\"{n}\" max=\"{n}\" width=\"{width}\" customWidth=\"1\"/>", n = i + 1));
    }
    out.push_str("</cols><sheetData>");
    let text = |out: &mut String, cell: &str, s: &str| {
        if !s.is_empty() {
            out.push_str(&format!("<c r=\"{cell}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", escape(s)));
        }
    };
    let number = |out: &mut String, cell: &str, n: f64| {
        if n.is_finite() {
            out.push_str(&format!("<c r=\"{cell}\"><v>{n}</v></c>"));
        }
    };
    out.push_str("<row r=\"1\">");
    for (i, h) in HEADINGS.iter().copied().chain(t.columns.iter().take(200).map(String::as_str)).enumerate() {
        text(&mut out, &format!("{}1", col(i)), h);
    }
    out.push_str("</row>");
    for (i, r) in t.rows.iter().enumerate() {
        let n = i + 2;
        out.push_str(&format!("<row r=\"{n}\">"));
        let at = |c: usize| format!("{}{n}", col(c));
        text(&mut out, &at(0), &r.name);
        text(&mut out, &at(1), &r.kind);
        text(&mut out, &at(2), &r.label);
        text(&mut out, &at(3), &r.layer);
        if let Some(p) = r.page {
            number(&mut out, &at(4), p as f64);
        }
        number(&mut out, &at(5), r.quantity);
        text(&mut out, &at(6), &r.unit);
        text(&mut out, &at(7), &r.shown);
        for (j, f) in r.fields.iter().take(200).enumerate() {
            text(&mut out, &at(HEADINGS.len() + j), f);
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData></worksheet>");
    Ok(out)
}

/// The workbook `xlsx` with `t` written to its Takeoff sheet (made when it has none).
pub fn write(xlsx: &[u8], t: &Takeoff) -> Result<Sent, CreateError> {
    let mut pkg = Package::open(xlsx)?;
    let mut notes = Vec::new();

    // The workbook part, from the package's relationships.
    let root_rels = pkg.text("_rels/.rels")?;
    let root_tags = tags(&root_rels);
    let workbook = root_tags
        .iter()
        .filter(|t| !t.closing && t.local() == "Relationship")
        .find(|t| t.attr("Type").is_some_and(|ty| ty.ends_with("/officeDocument")))
        .and_then(|t| t.attr("Target"))
        .map(|target| resolve("", &target))
        .ok_or_else(|| invalid("this isn't an XLSX workbook (it has no workbook part)"))?;
    let folder = folder_of(&workbook).to_string();
    let wb_rels_name = rels_of(&workbook);
    let wb_xml = pkg.text(&workbook)?;
    let wb_rels = pkg.text(&wb_rels_name)?;
    let types = pkg.text("[Content_Types].xml")?;
    let ns = if wb_xml.contains(STRICT.main) { STRICT } else { TRANSITIONAL };

    let wb_tags = tags(&wb_xml);
    let rel_tags = tags(&wb_rels);
    let type_tags = tags(&types);
    let root = find(&wb_tags, "workbook").ok_or_else(|| invalid("the workbook part has no workbook element"))?;
    let p = if root.prefix().is_empty() { String::new() } else { format!("{}:", root.prefix()) };
    let root_end = closing(&wb_tags, root).ok_or_else(|| invalid("the workbook part isn't complete"))?.start;
    let rels_root = find(&rel_tags, "Relationships").ok_or_else(|| invalid("the workbook's relationships can't be read"))?;
    let rels_end = closing(&rel_tags, rels_root).ok_or_else(|| invalid("the workbook's relationships can't be read"))?.start;
    let rp = if rels_root.prefix().is_empty() { String::new() } else { format!("{}:", rels_root.prefix()) };
    let types_root = find(&type_tags, "Types").ok_or_else(|| invalid("the workbook's content types can't be read"))?;
    let types_end = closing(&type_tags, types_root).ok_or_else(|| invalid("the workbook's content types can't be read"))?.start;
    let tp = if types_root.prefix().is_empty() { String::new() } else { format!("{}:", types_root.prefix()) };

    let relationships: Vec<Tag<'_>> = rel_tags.iter().filter(|t| !t.closing && t.local() == "Relationship").copied().collect();
    let sheets: Vec<Tag<'_>> = wb_tags.iter().filter(|t| !t.closing && t.local() == "sheet").copied().collect();
    let existing = sheets.iter().find(|s| s.attr("name").is_some_and(|n| n.eq_ignore_ascii_case(SHEET))).copied();

    let mut wb_edits = Vec::new();
    let mut rel_edits = Vec::new();
    let mut type_edits = Vec::new();
    let mut added: Vec<(String, String)> = Vec::new();
    let mut replaced: Vec<(String, String)> = Vec::new();
    let mut dropped: HashSet<String> = HashSet::new();

    // The Takeoff sheet's part: the existing one, or a new one.
    let sheet_part = match existing {
        Some(s) => {
            let id = s.attr("id").ok_or_else(|| invalid("the Takeoff sheet has no relationship"))?;
            let rel = relationships
                .iter()
                .find(|r| r.attr("Id").as_deref() == Some(id.as_str()))
                .ok_or_else(|| invalid("the Takeoff sheet's part can't be found"))?;
            if rel.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External")) {
                return Err(invalid("the Takeoff sheet's part is outside the workbook"));
            }
            if !rel.attr("Type").is_some_and(|ty| ty.ends_with("/worksheet")) {
                return Err(invalid("the workbook's Takeoff sheet isn't a worksheet (rename it to send quantities)"));
            }
            resolve(&folder, &rel.attr("Target").unwrap_or_default())
        }
        None => {
            let taken_ids: HashSet<String> = relationships.iter().filter_map(|r| r.attr("Id")).collect();
            let id = (1..MAX_ENTRIES).map(|n| format!("rIdTakeoff{n}")).find(|i| !taken_ids.contains(i)).unwrap_or_else(|| "rIdTakeoff".into());
            let sheet_id = sheets.iter().filter_map(|s| s.attr("sheetId").and_then(|v| v.parse::<u64>().ok())).max().unwrap_or(0).saturating_add(1);
            let number = (1..MAX_ENTRIES)
                .find(|n| !pkg.has(&resolve(&folder, &format!("worksheets/sheet{n}.xml"))))
                .ok_or_else(|| invalid("the workbook has too many sheets"))?;
            let target = format!("worksheets/sheet{number}.xml");
            let part = resolve(&folder, &target);
            // The relationships namespace's prefix in the workbook part, else declared here.
            let declared = attrs(root.attrs)
                .into_iter()
                .find(|(n, v)| n.starts_with("xmlns:") && unescape(v) == ns.rel)
                .map(|(n, _)| n.trim_start_matches("xmlns:").to_string());
            let (r, decl) = match declared {
                Some(r) => (r, String::new()),
                None => ("rt".to_string(), format!(" xmlns:rt=\"{}\"", ns.rel)),
            };
            let sheets_open = find(&wb_tags, "sheets").ok_or_else(|| invalid("the workbook part has no sheets"))?;
            let sheets_close =
                closing(&wb_tags, sheets_open).filter(|c| c.closing).ok_or_else(|| invalid("the workbook part's sheets can't be read"))?;
            wb_edits.push(Splice {
                start: sheets_close.start,
                end: sheets_close.start,
                text: format!("<{p}sheet name=\"{SHEET}\" sheetId=\"{sheet_id}\" {r}:id=\"{id}\"{decl}/>"),
            });
            let rel_type = format!("{}/worksheet", ns.rel);
            rel_edits.push(Splice {
                start: rels_end,
                end: rels_end,
                text: format!("<{rp}Relationship Id=\"{id}\" Type=\"{rel_type}\" Target=\"{target}\"/>"),
            });
            type_edits.push(Splice {
                start: types_end,
                end: types_end,
                text: format!(
                    "<{tp}Override PartName=\"/{part}\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
                ),
            });
            added.push((part.clone(), String::new()));
            part
        }
    };
    let sheet = sheet_xml(t, ns.main)?;
    match added.iter_mut().find(|(n, _)| *n == sheet_part) {
        Some(a) => a.1 = sheet,
        None => replaced.push((sheet_part.clone(), sheet)),
    }

    // Defined names: those on the Takeoff sheet are replaced by the rows' names.
    let names_open = find(&wb_tags, "definedNames");
    let names_close = names_open.and_then(|o| closing(&wb_tags, o));
    let mut kept = String::new();
    let mut used: HashSet<String> = HashSet::new();
    if let (Some(open), Some(close)) = (names_open, names_close) {
        let entries: Vec<Tag<'_>> =
            wb_tags.iter().filter(|t| t.start > open.start && t.start < close.start && !t.closing && t.local() == "definedName").copied().collect();
        for e in entries {
            let end = closing(&wb_tags, e).map_or(e.end, |c| c.end);
            let formula = if e.empty {
                String::new()
            } else {
                unescape(wb_xml.get(e.end..closing(&wb_tags, e).map_or(e.end, |c| c.start)).unwrap_or_default())
            };
            if on_takeoff(&formula) {
                continue;
            }
            if e.attr("localSheetId").is_none()
                && let Some(n) = e.attr("name")
            {
                used.insert(n.to_lowercase());
            }
            kept.push_str(wb_xml.get(e.start..end).unwrap_or_default());
        }
    }
    let mut ours = String::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (i, r) in t.rows.iter().enumerate() {
        let lower = r.name.to_lowercase();
        if r.name.is_empty() || !seen.insert(lower.clone()) {
            continue;
        }
        if used.contains(&lower) {
            notes.push(format!("{} is already a name in the workbook, so it wasn't given to the Takeoff row", r.name));
            continue;
        }
        let row = i + 2;
        let col = col_to_letters(QUANTITY_COL - 1);
        ours.push_str(&format!("<{p}definedName name=\"{}\">{SHEET}!${col}${row}</{p}definedName>", escape(&r.name)));
    }
    let names_xml = if kept.is_empty() && ours.is_empty() { String::new() } else { format!("<{p}definedNames>{kept}{ours}</{p}definedNames>") };
    match (names_open, names_close) {
        (Some(open), Some(close)) => wb_edits.push(Splice { start: open.start, end: close.end, text: names_xml }),
        _ => {
            let at = before_any(&wb_tags, &AFTER_NAMES, root_end);
            wb_edits.push(Splice { start: at, end: at, text: names_xml });
        }
    }

    // Formulas are worked out when the workbook is next opened.
    match find(&wb_tags, "calcPr") {
        Some(calc) => {
            let kept_attrs: String =
                attrs(calc.attrs).into_iter().filter(|(n, _)| *n != "fullCalcOnLoad").map(|(n, v)| format!(" {n}=\"{v}\"")).collect();
            let tail = if calc.empty { "/>" } else { ">" };
            wb_edits.push(Splice { start: calc.start, end: calc.end, text: format!("<{}{kept_attrs} fullCalcOnLoad=\"1\"{tail}", calc.name) });
        }
        None => {
            let at = before_any(&wb_tags, &AFTER_CALC, root_end);
            wb_edits.push(Splice { start: at, end: at, text: format!("<{p}calcPr fullCalcOnLoad=\"1\"/>") });
        }
    }

    // The calculation chain lists formula cells; the spreadsheet rebuilds it.
    for r in relationships.iter().filter(|r| r.attr("Type").is_some_and(|ty| ty.ends_with("/calcChain"))) {
        let part = resolve(&folder, &r.attr("Target").unwrap_or_default());
        rel_edits.push(Splice { start: r.start, end: closing(&rel_tags, *r).map_or(r.end, |c| c.end), text: String::new() });
        if let Some(o) =
            type_tags.iter().find(|o| !o.closing && o.local() == "Override" && o.attr("PartName").is_some_and(|n| n.trim_start_matches('/') == part))
        {
            type_edits.push(Splice { start: o.start, end: closing(&type_tags, *o).map_or(o.end, |c| c.end), text: String::new() });
        }
        dropped.insert(part);
    }

    replaced.push((workbook.clone(), splice(&wb_xml, wb_edits)?));
    replaced.push((wb_rels_name.clone(), splice(&wb_rels, rel_edits)?));
    replaced.push(("[Content_Types].xml".into(), splice(&types, type_edits)?));

    // The package again: changed parts written, the rest copied as they were.
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let zip_err = |e: zip::result::ZipError| invalid(format!("the workbook couldn't be written: {e}"));
    for i in 0..pkg.archive.len() {
        let name = pkg.archive.by_index_raw(i).map_err(zip_err)?.name().to_string();
        if dropped.contains(&name) {
            continue;
        }
        if let Some((_, text)) = replaced.iter().find(|(n, _)| *n == name) {
            out.start_file(name.as_str(), options).map_err(zip_err)?;
            out.write_all(text.as_bytes()).map_err(|e| invalid(e.to_string()))?;
            continue;
        }
        let entry = pkg.archive.by_index_raw(i).map_err(zip_err)?;
        out.raw_copy_file(entry).map_err(zip_err)?;
    }
    for (name, text) in &added {
        out.start_file(name.as_str(), options).map_err(zip_err)?;
        out.write_all(text.as_bytes()).map_err(|e| invalid(e.to_string()))?;
    }
    let bytes = out.finish().map_err(zip_err)?.into_inner();
    Ok(Sent { bytes, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::open;

    fn takeoff(kitchen: f64) -> Takeoff {
        Takeoff {
            columns: vec!["Drop ID".into()],
            rows: vec![
                TakeoffRow {
                    name: "Kitchen_floor".into(),
                    kind: "Area".into(),
                    label: "Kitchen floor".into(),
                    layer: "Flooring".into(),
                    page: Some(1),
                    quantity: kitchen,
                    unit: "sq ft".into(),
                    shown: format!("{kitchen} sq ft"),
                    fields: vec![String::new()],
                },
                TakeoffRow {
                    name: "Cable_run_1".into(),
                    kind: "Cable run".into(),
                    page: Some(2),
                    quantity: 86.5,
                    unit: "ft".into(),
                    shown: "86'-6\"".into(),
                    fields: vec!["D-101 & D-102 <east>".into()],
                    ..TakeoffRow::default()
                },
            ],
        }
    }

    /// A budget written by GridCraft whose formulas use the takeoff's names.
    fn budget() -> Vec<u8> {
        let mut s = gridcraft_engine::Session::new();
        s.new_workbook();
        for (cell, input) in
            [("A1", "Flooring"), ("B1", "=Kitchen_floor*6.5"), ("A2", "Cable"), ("B2", "=Cable_run_1*0.8"), ("A3", "Total"), ("B3", "=B1+B2")]
        {
            s.execute("cell.set", serde_json::json!({ "cell": cell, "input": input })).unwrap();
        }
        gridcraft_engine::io::save_bytes(&s.doc().unwrap().wb, "budget.xlsx").unwrap()
    }

    fn part(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).ok()?;
        let mut f = z.by_name(name).ok()?;
        let mut out = Vec::new();
        f.read_to_end(&mut out).ok()?;
        Some(out)
    }

    fn number(book: &Book, sheet: usize, cell: &str) -> f64 {
        let c = gridcraft_engine::core::RangeRef::parse(cell).unwrap().start;
        match book.wb.sheets[sheet].value(c) {
            Value::Number(n) => n,
            v => panic!("{cell} is {v:?}"),
        }
    }

    #[test]
    fn a_budget_follows_the_takeoff_through_names() {
        let original = budget();
        let sent = write(&original, &takeoff(120.0)).unwrap();
        assert!(sent.notes.is_empty(), "{:?}", sent.notes);
        let book = open(&sent.bytes, "budget.xlsx").unwrap();
        assert_eq!(book.sheet_names().last().map(String::as_str), Some(SHEET));
        assert_eq!(number(&book, 0, "B1"), 780.0, "120 sq ft × 6.50");
        assert_eq!(number(&book, 0, "B3"), 780.0 + 86.5 * 0.8);
        assert!(book.has_takeoff(&takeoff(120.0)));
        assert!(!book.has_takeoff(&takeoff(140.0)), "a changed area is noticed");
        let grid = book.grid(1).unwrap();
        assert_eq!(grid.rows[0][..3], ["Name", "Type", "Label"]);
        assert_eq!(grid.rows[2][8], "D-101 & D-102 <east>", "text escaped and read back");
        // Every part but the three changed and the new sheet is copied byte for byte.
        let z = zip::ZipArchive::new(Cursor::new(original.clone())).unwrap();
        for name in z.file_names() {
            if !matches!(name, "xl/workbook.xml" | "xl/_rels/workbook.xml.rels" | "[Content_Types].xml" | "xl/calcChain.xml") {
                assert_eq!(part(&sent.bytes, name), part(&original, name), "{name} unchanged");
            }
        }

        // Sent again: the sheet and its names are replaced, not added twice.
        let mut fewer = takeoff(200.0);
        fewer.rows.truncate(1);
        let again = write(&sent.bytes, &fewer).unwrap();
        let book = open(&again.bytes, "budget.xlsx").unwrap();
        assert_eq!(book.sheet_names().iter().filter(|n| *n == SHEET).count(), 1);
        assert_eq!(number(&book, 0, "B1"), 1300.0);
        assert!(book.has_takeoff(&fewer));
        let wb = String::from_utf8(part(&again.bytes, "xl/workbook.xml").unwrap()).unwrap();
        assert_eq!(wb.matches("name=\"Kitchen_floor\"").count(), 1);
        assert!(!wb.contains("Cable_run_1"), "a measurement gone takes its name with it");
        assert_eq!(wb.matches("fullCalcOnLoad").count(), 1);
    }

    /// A package of hand-written parts.
    fn package(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in parts {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(text.as_bytes()).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    const TYPES: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/></Types>"#;
    const ROOT_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    const WB_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="/xl/worksheets/sheet1.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/></Relationships>"#;
    /// A prefixed workbook part with a name of its own, a print area and manual calculation.
    const WORKBOOK: &str = r#"<?xml version="1.0"?><x:workbook xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:rel="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><!-- <x:sheets> in a comment --><x:sheets><x:sheet name="Budget" sheetId="4" rel:id="rId1"/></x:sheets><x:definedNames><x:definedName name="Rate">Budget!$B$1</x:definedName><x:definedName name="_xlnm.Print_Area" localSheetId="0">Budget!$A$1:$C$9</x:definedName><x:definedName name="area_1">Budget!$C$1</x:definedName></x:definedNames><x:calcPr calcId="191029" calcMode="manual"/></x:workbook>"#;
    const SHEET1: &str = r#"<?xml version="1.0"?><x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheetData><x:row r="1"><x:c r="A1"><x:v>2</x:v></x:c></x:row></x:sheetData></x:worksheet>"#;

    #[test]
    fn a_hand_written_workbook_gains_the_sheet_and_keeps_its_own_names() {
        let original = package(&[
            ("[Content_Types].xml", TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", WB_RELS),
            ("xl/worksheets/sheet1.xml", SHEET1),
            ("xl/calcChain.xml", "<calcChain/>"),
        ]);
        let mut t = takeoff(50.0);
        t.rows.push(TakeoffRow { name: "Area_1".into(), kind: "Area".into(), quantity: 3.0, ..TakeoffRow::default() });
        let sent = write(&original, &t).unwrap();
        assert_eq!(sent.notes.len(), 1, "Area_1 is the workbook's own name (ignoring case): {:?}", sent.notes);
        let wb = String::from_utf8(part(&sent.bytes, "xl/workbook.xml").unwrap()).unwrap();
        assert!(wb.contains(r#"<x:sheet name="Takeoff" sheetId="5" rel:id="rIdTakeoff1"/></x:sheets>"#), "{wb}");
        assert!(wb.contains(r#"<x:definedName name="Rate">Budget!$B$1</x:definedName>"#));
        assert!(wb.contains("_xlnm.Print_Area"));
        assert!(wb.contains(r#"<x:definedName name="Kitchen_floor">Takeoff!$F$2</x:definedName>"#), "{wb}");
        assert!(wb.contains(r#"<x:calcPr calcId="191029" calcMode="manual" fullCalcOnLoad="1"/>"#), "{wb}");
        assert!(part(&sent.bytes, "xl/calcChain.xml").is_none(), "the calculation chain is dropped");
        let types = String::from_utf8(part(&sent.bytes, "[Content_Types].xml").unwrap()).unwrap();
        assert!(!types.contains("calcChain") && types.contains("/xl/worksheets/sheet2.xml"));
        let rels = String::from_utf8(part(&sent.bytes, "xl/_rels/workbook.xml.rels").unwrap()).unwrap();
        assert!(!rels.contains("calcChain") && rels.contains(r#"Target="worksheets/sheet2.xml""#), "{rels}");
        assert_eq!(part(&sent.bytes, "xl/worksheets/sheet1.xml").as_deref(), Some(SHEET1.as_bytes()));
    }

    #[test]
    fn a_new_workbook_has_a_budget_and_the_takeoff() {
        let sent = new_workbook(&takeoff(10.0)).unwrap();
        let book = open(&sent.bytes, "Takeoff.xlsx").unwrap();
        assert_eq!(book.sheet_names(), ["Budget", SHEET]);
        assert!(book.has_takeoff(&takeoff(10.0)));
    }

    #[test]
    fn what_isnt_a_workbook_is_refused() {
        assert!(write(b"not a zip", &takeoff(1.0)).is_err());
        assert!(write(&package(&[("readme.txt", "hello")]), &takeoff(1.0)).is_err());
        // A Takeoff that is a chart sheet isn't written over.
        let chart_rels =
            WB_RELS.replace("rId9", "rId2").replace("calcChain\" Target=\"calcChain.xml", "chartsheet\" Target=\"chartsheets/sheet1.xml");
        let wb = WORKBOOK.replace("</x:sheets>", r#"<x:sheet name="takeoff" sheetId="5" rel:id="rId2"/></x:sheets>"#);
        let pkg = package(&[
            ("[Content_Types].xml", TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("xl/workbook.xml", &wb),
            ("xl/_rels/workbook.xml.rels", &chart_rels),
        ]);
        assert!(write(&pkg, &takeoff(1.0)).is_err());
        assert!(can_send("Budget.XLSX") && can_send("b.xlsm") && !can_send("b.csv") && !can_send("b.ods"));
    }

    #[test]
    fn the_scanner_reads_what_it_must() {
        let xml = r#"<a x='1>2' y="&lt;&amp;&#65;&#x42;"><!-- <b/> --><![CDATA[<c/>]]><d:e f="g"/></a>"#;
        let t = tags(xml);
        let names: Vec<&str> = t.iter().map(|t| t.name).collect();
        assert_eq!(names, ["a", "d:e", "a"]);
        assert_eq!(t[0].attr("x").as_deref(), Some("1>2"));
        assert_eq!(t[0].attr("y").as_deref(), Some("<&AB"));
        assert_eq!(t[1].local(), "e");
        assert!(t[1].empty && t[2].closing);
        assert_eq!(resolve("xl", "../docProps/app.xml"), "docProps/app.xml");
        assert_eq!(resolve("xl", "/xl/worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(rels_of("xl/workbook.xml"), "xl/_rels/workbook.xml.rels");
        assert!(on_takeoff("Takeoff!$F$2") && on_takeoff("='Takeoff'!$F$2") && !on_takeoff("Budget!$A$1"));
        assert_eq!(escape("a\u{1}<b>"), "a&lt;b&gt;");
        // Unfinished or hostile text doesn't stop the scan badly.
        assert!(tags("<a b=\"unterminated").is_empty());
        assert_eq!(unescape("&bogus; & &#xFFFFFFFF;"), "&bogus; & &#xFFFFFFFF;");
    }
}
