//! RevuCraft: sheets linked to the DWG/DXF drawings they came from, as InDesign links its
//! placed files.
//!
//! A linked sheet draws its drawing through a form XObject, so the drawing can be replaced from
//! the file without touching the markups (annotations) on the sheet. The link is a record in the
//! catalog's `/PCLinks` array: the file's path, the layout plotted, the page, the form, and the
//! file's size, modification time and fingerprint when the sheet was last made from it, so a
//! reader can tell when the file has changed. A plotted copy of the drawing stays in the PDF, so
//! the sheet still shows (and prints) without the file.
//!
//! Optionally, an update keeps the previous drawing as a hidden layer ("Rev A · A-201"), drawn
//! under the current one.
//!
//! A link can also be a table: a range of a workbook (XLSX, CSV…) printed by GridCraft and placed
//! on a sheet (`/Kind /Table`, with `/Sheet` and `/Range`). It is listed, updated, moved and kept
//! as revisions the same way; an update keeps its top-left corner where it was.

use std::collections::{HashMap, HashSet};

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString, Stream};

use crate::{EditError, measure};

/// The catalog key holding the link records.
const LINKS: &[u8] = b"PCLinks";
/// Links kept at most, and earlier drawings kept per link.
const MAX_LINKS: usize = 512;
const MAX_REVISIONS: usize = 26;
/// Page content decoded at most when a sheet is first linked.
const MAX_CONTENT: usize = 256 << 20;

/// What a link draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkKind {
    /// A DWG or DXF drawing: a whole sheet, or placed on one.
    #[default]
    Drawing,
    /// A table: a range of a workbook, placed on a sheet.
    Table,
}

/// A linked drawing or table, as the Links pane shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct CadLink {
    /// A drawing or a table.
    pub kind: LinkKind,
    /// A table's range as asked for (`B2:F30`, a named range, or empty for the print area).
    pub range: String,
    /// Its position in the document's list of links.
    pub index: usize,
    /// The sheet (page index) it draws on.
    pub page: usize,
    pub path: String,
    /// Where the file is relative to the PDF (`/`-separated, such as `rooms/101.dwg`), when it was
    /// known as the link was made: found again on any device that has both in the same folders.
    pub relative: Option<String>,
    /// The layout plotted (`Model` for model space).
    pub layout: String,
    /// The file as it was when the sheet was last made from it.
    pub size: u64,
    pub mtime: i64,
    pub fingerprint: String,
    /// When the sheet was last made from the file (seconds since 1970).
    pub updated: i64,
    /// Earlier drawings kept as hidden layers.
    pub revisions: usize,
    /// Placed on a sheet (one of several drawings on it), not the whole sheet.
    pub placed: bool,
    /// Where a placed drawing is on its sheet (user space: x0, y0, x1, y1).
    pub rect: Option<[f64; 4]>,
    /// How much a placed table is enlarged (1 at the size it prints in its workbook).
    pub scale: f64,
}

/// The file a sheet is linked to, as read from disk by the app.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkedFile {
    pub path: String,
    /// The file's path relative to the PDF (`/`-separated), when the PDF has a place on disk.
    pub relative: Option<String>,
    pub size: u64,
    pub mtime: i64,
    /// Now (seconds since 1970), recorded as the update time.
    pub now: i64,
}

/// A short fingerprint of a file's bytes (64-bit FNV-1a, hex), to tell a real change from a
/// touched file.
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn invalid(msg: &str) -> EditError {
    EditError::Source(msg.to_string())
}

fn catalog(doc: &Document) -> Result<(ObjRef, Dict), EditError> {
    let root = doc.root().ok_or_else(|| invalid("the document has no catalog"))?;
    let d = doc.get(root).as_dict().cloned().ok_or_else(|| invalid("the document has no catalog"))?;
    Ok((root, d))
}

/// The link records, in order.
fn records(doc: &Document) -> Vec<Dict> {
    let Ok((_, cat)) = catalog(doc) else { return Vec::new() };
    let Some(list) = cat.get(LINKS).map(|o| doc.resolve(o)) else { return Vec::new() };
    list.as_array().map(|a| a.iter().take(MAX_LINKS).filter_map(|o| doc.resolve(o).as_dict().cloned()).collect()).unwrap_or_default()
}

fn write_records(doc: &mut Document, list: Vec<Dict>) -> Result<(), EditError> {
    let (root, _) = catalog(doc)?;
    doc.update_dict(root, |c| c.set(LINKS.to_vec(), Object::Array(list.into_iter().map(Object::Dict).collect()))).map_err(|e| invalid(&e.to_string()))
}

fn text(d: &Dict, key: &[u8]) -> String {
    d.get(key).and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default()
}

fn num(doc: &Document, d: &Dict, key: &[u8]) -> f64 {
    d.get(key).and_then(|o| doc.resolve(o).as_f64()).filter(|v| v.is_finite()).unwrap_or(0.0)
}

fn nums(doc: &Document, d: &Dict, key: &[u8]) -> Vec<f64> {
    d.get(key)
        .and_then(|o| doc.resolve(o).as_array().map(|a| a.iter().filter_map(|x| doc.resolve(x).as_f64()).filter(|v| v.is_finite()).collect()))
        .unwrap_or_default()
}

fn num_array(v: &[f64]) -> Object {
    Object::Array(v.iter().map(|x| Object::Real(*x)).collect())
}

fn placed(d: &Dict) -> bool {
    matches!(d.get(b"Placed"), Some(Object::Bool(true)))
}

/// A placed drawing's placement (a translation), identity otherwise.
fn matrix(doc: &Document, d: &Dict) -> [f64; 6] {
    match nums(doc, d, b"Matrix").as_slice() {
        [a, b, c, e, f, g] => [*a, *b, *c, *e, *f, *g],
        _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

/// Where a placed drawing or table is on its sheet (user space: x0, y0, x1, y1), at its scale.
fn placed_rect(doc: &Document, r: &Dict) -> [f64; 4] {
    let m = matrix(doc, r);
    let (w, h) = (num(doc, r, b"W") * m[0], num(doc, r, b"H") * m[3]);
    [m[4], m[5], m[4] + w, m[5] + h]
}

/// A name for a new link's form in its sheet's resources, used by no other link.
fn new_xname(list: &[Dict]) -> String {
    let taken: HashSet<Vec<u8>> = list.iter().filter_map(|r| r.name(b"XName").map(<[u8]>::to_vec)).collect();
    (list.len()..list.len().saturating_add(MAX_LINKS + 1))
        .map(|i| format!("PCLink{i}"))
        .find(|n| !taken.contains(n.as_bytes()))
        .unwrap_or_else(|| "PCLinkX".into())
}

fn cm(m: &[f64; 6]) -> String {
    format!("{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} cm ", m[0], m[1], m[2], m[3], m[4], m[5])
}

fn int(doc: &Document, d: &Dict, key: &[u8]) -> i64 {
    d.get(key).and_then(|o| doc.resolve(o).as_int()).unwrap_or(0)
}

/// The document's linked drawings.
pub fn list(doc: &Document) -> Vec<CadLink> {
    let pages: Vec<ObjRef> = pdfcraft_model::pages(doc).into_iter().map(|p| p.obj).collect();
    records(doc)
        .iter()
        .enumerate()
        .filter_map(|(index, r)| {
            let page = pages.iter().position(|p| Some(*p) == r.reference(b"Page"))?;
            Some(CadLink {
                kind: if r.name(b"Kind") == Some(&b"Table"[..]) { LinkKind::Table } else { LinkKind::Drawing },
                range: text(r, b"Range"),
                index,
                page,
                path: text(r, b"Path"),
                relative: Some(text(r, b"RelPath")).filter(|p| !p.is_empty()),
                layout: text(r, b"Layout"),
                size: u64::try_from(int(doc, r, b"Size")).unwrap_or(0),
                mtime: int(doc, r, b"MTime"),
                fingerprint: text(r, b"Hash"),
                updated: int(doc, r, b"Updated"),
                revisions: r.get(b"Revs").and_then(|o| doc.resolve(o).as_array().map(Vec::len)).unwrap_or(0),
                placed: placed(r),
                rect: placed(r).then(|| placed_rect(doc, r)),
                scale: matrix(doc, r)[0],
            })
        })
        .collect()
}

fn decoded_contents(doc: &Document, page: &Dict) -> Result<Vec<u8>, EditError> {
    let mut content = Vec::new();
    if let Some(c) = page.get(b"Contents") {
        let list = match &*doc.resolve(c) {
            Object::Array(a) => a.clone(),
            _ => vec![c.clone()],
        };
        for o in list {
            if let Object::Stream(s) = &*doc.resolve(&o) {
                let bytes = s.decoded_within(MAX_CONTENT).map_err(|e| invalid(&format!("the sheet's drawing: {e}")))?;
                if content.len().saturating_add(bytes.len()) > MAX_CONTENT {
                    return Err(invalid("the sheet's drawing is too large to link"));
                }
                content.extend(bytes);
                content.push(b'\n');
            }
        }
    }
    Ok(content)
}

fn media_box(doc: &Document, page: &Dict) -> [f64; 4] {
    let v: Vec<f64> = page
        .get(b"MediaBox")
        .and_then(|o| doc.resolve(o).as_array().map(|a| a.iter().filter_map(|x| doc.resolve(x).as_f64()).collect()))
        .unwrap_or_default();
    match v.as_slice() {
        [a, b, c, d] if v.iter().all(|x| x.is_finite()) => [a.min(*c), b.min(*d), a.max(*c), b.max(*d)],
        _ => [0.0, 0.0, 612.0, 792.0],
    }
}

/// Redraw a page from its link records: earlier drawings (each on its hidden layer), then the
/// current one. The page's own resources become the links' forms and layers.
fn redraw_page(doc: &mut Document, page: ObjRef) -> Result<(), EditError> {
    let mut content = String::new();
    let mut xobjects = Dict::new();
    let mut props = Dict::new();
    // What the sheet had before drawings were placed on it.
    if let Some(base) = doc.get(page).as_dict().and_then(|d| d.reference(b"PCBase")) {
        xobjects.set(b"PCBase".to_vec(), Object::Ref(base));
        content.push_str("q /PCBase Do Q\n");
    }
    for r in records(doc).iter().filter(|r| r.reference(b"Page") == Some(page)) {
        let name = r.name(b"XName").map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_else(|| "PCLink".into());
        let m = matrix(doc, r);
        if let Some(Object::Array(revs)) = r.get(b"Revs").map(|o| (*doc.resolve(o)).clone()) {
            for (k, rev) in revs.iter().enumerate() {
                let Some(rev) = doc.resolve(rev).as_dict().cloned() else { continue };
                let (Some(form), Some(ocg)) = (rev.reference(b"Form"), rev.reference(b"OCG")) else { continue };
                let (x, o) = (format!("{name}R{k}"), format!("{name}O{k}"));
                // An earlier drawing stays where it was drawn.
                let rm = if rev.contains(b"Matrix") { matrix(doc, &rev) } else { m };
                xobjects.set(x.clone().into_bytes(), Object::Ref(form));
                props.set(o.clone().into_bytes(), Object::Ref(ocg));
                content.push_str(&format!("/OC /{o} BDC q {}/{x} Do Q EMC\n", cm(&rm)));
            }
        }
        if let Some(form) = r.reference(b"Form") {
            xobjects.set(name.clone().into_bytes(), Object::Ref(form));
            // A placed drawing is on its own layer, named after its file.
            match r.reference(b"OCG").filter(|_| placed(r)) {
                Some(ocg) => {
                    let o = format!("{name}L");
                    props.set(o.clone().into_bytes(), Object::Ref(ocg));
                    content.push_str(&format!("/OC /{o} BDC q {}/{name} Do Q EMC\n", cm(&m)));
                }
                None => content.push_str(&format!("q {}/{name} Do Q\n", cm(&m))),
            }
        }
    }
    let mut res = Dict::new();
    res.set(b"XObject".to_vec(), Object::Dict(xobjects));
    if !props.is_empty() {
        res.set(b"Properties".to_vec(), Object::Dict(props));
    }
    let stream = doc.add(Object::Stream(Stream::flate(Dict::new(), content.as_bytes())));
    doc.update_dict(page, |d| {
        d.set(b"Contents".to_vec(), Object::Ref(stream));
        d.set(b"Resources".to_vec(), Object::Dict(res));
    })
    .map_err(|e| invalid(&e.to_string()))
}

/// Link page `page` (drawn from `layout` of `file`) to its drawing: its content becomes a form
/// the link can replace.
pub(crate) fn link_page(doc: &mut Document, page: usize, layout: &str, file: &LinkedFile, hash: &str) -> Result<(), EditError> {
    let pages = pdfcraft_model::pages(doc);
    let p = pages.get(page).ok_or_else(|| invalid("no such sheet"))?;
    let content = decoded_contents(doc, &p.dict)?;
    let resources = p.dict.get(b"Resources").map(|o| (*doc.resolve(o)).clone()).unwrap_or(Object::Dict(Dict::new()));
    let bbox = media_box(doc, &p.dict);
    let mut fd = Dict::new();
    fd.set(b"Type".to_vec(), Object::name("XObject"));
    fd.set(b"Subtype".to_vec(), Object::name("Form"));
    fd.set(b"BBox".to_vec(), Object::Array(bbox.iter().map(|v| Object::Real(*v)).collect()));
    fd.set(b"Resources".to_vec(), resources);
    let form = doc.add(Object::Stream(Stream::flate(fd, &content)));
    let mut list = records(doc);
    if list.len() >= MAX_LINKS {
        return Err(invalid("the document has too many linked drawings"));
    }
    let mut r = Dict::new();
    r.set(b"Path".to_vec(), PdfString::text(&file.path));
    set_relative(&mut r, file.relative.as_deref());
    r.set(b"Layout".to_vec(), PdfString::text(layout));
    r.set(b"Page".to_vec(), Object::Ref(p.obj));
    r.set(b"Form".to_vec(), Object::Ref(form));
    r.set(b"XName".to_vec(), Object::name(&format!("PCLink{}", list.len())));
    r.set(b"Size".to_vec(), Object::Int(i64::try_from(file.size).unwrap_or(i64::MAX)));
    r.set(b"MTime".to_vec(), Object::Int(file.mtime));
    r.set(b"Hash".to_vec(), PdfString::text(hash));
    r.set(b"Updated".to_vec(), Object::Int(file.now));
    list.push(r);
    write_records(doc, list)?;
    redraw_page(doc, p.obj)
}

/// The catalog's optional content properties, resolved: the dictionary, its default
/// configuration and their arrays inline, so they can be changed and written back whole.
fn ocproperties(doc: &Document) -> Option<Dict> {
    let (_, cat) = catalog(doc).ok()?;
    let mut p = doc.resolve(cat.get(b"OCProperties")?).as_dict()?.clone();
    let arrays = |d: &mut Dict, keys: &[&[u8]]| {
        for k in keys {
            if let Some(a) = d.get(k).map(|o| doc.resolve(o)).and_then(|o| o.as_array().cloned()) {
                d.set(k.to_vec(), Object::Array(a));
            }
        }
    };
    arrays(&mut p, &[b"OCGs"]);
    if let Some(mut config) = p.get(b"D").map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()) {
        arrays(&mut config, &[b"Order", b"ON", b"OFF"]);
        p.set(b"D".to_vec(), Object::Dict(config));
    }
    Some(p)
}

/// Write the optional content properties back where they were (inline or their own object).
fn write_ocproperties(doc: &mut Document, props: Dict) -> Result<(), EditError> {
    let (root, cat) = catalog(doc)?;
    match cat.get(b"OCProperties") {
        Some(Object::Ref(r)) => {
            doc.set(*r, Object::Dict(props));
            Ok(())
        }
        _ => doc.update_dict(root, |c| c.set(b"OCProperties".to_vec(), Object::Dict(props))).map_err(|e| invalid(&e.to_string())),
    }
}

fn retain_refs(d: &mut Dict, key: &[u8], keep: &dyn Fn(ObjRef) -> bool) {
    if let Some(a) = d.get(key).and_then(Object::as_array).cloned() {
        let kept: Vec<Object> = a.into_iter().filter(|o| o.as_ref().is_none_or(keep)).collect();
        d.set(key.to_vec(), Object::Array(kept));
    }
}

/// Copying a drawing again brings its CAD layers again: use the layers already there (by name)
/// and drop the copies, so an update doesn't double the layer list.
fn reuse_layers(doc: &mut Document, form: ObjRef, before: &HashMap<String, ObjRef>) -> Result<(), EditError> {
    let known: HashSet<ObjRef> = before.values().copied().collect();
    let mut same: HashMap<ObjRef, ObjRef> = HashMap::new();
    for l in pdfcraft_annot::layers::list(doc) {
        if !known.contains(&l.id)
            && let Some(old) = before.get(&l.name)
        {
            same.insert(l.id, *old);
        }
    }
    if same.is_empty() {
        return Ok(());
    }
    // The form's /Properties point at the layers already there.
    let form_obj = (*doc.get(form)).clone();
    let Object::Stream(mut s) = form_obj else { return Ok(()) };
    let res_obj = s.dict.get(b"Resources").cloned();
    let swap = |props: &mut Dict| {
        let keys: Vec<Vec<u8>> = props.iter().map(|(k, _)| k.clone()).collect();
        for k in keys {
            if let Some(Object::Ref(r)) = props.get(&k)
                && let Some(old) = same.get(r)
            {
                props.set(k, Object::Ref(*old));
            }
        }
    };
    match res_obj {
        Some(Object::Ref(rr)) => {
            let mut res = doc.get(rr).as_dict().cloned().unwrap_or_default();
            match res.get(b"Properties").cloned() {
                Some(Object::Ref(pr)) => {
                    let mut props = doc.get(pr).as_dict().cloned().unwrap_or_default();
                    swap(&mut props);
                    doc.set(pr, Object::Dict(props));
                }
                Some(Object::Dict(mut props)) => {
                    swap(&mut props);
                    res.set(b"Properties".to_vec(), Object::Dict(props));
                    doc.set(rr, Object::Dict(res));
                }
                _ => {}
            }
        }
        Some(Object::Dict(mut res)) => {
            match res.get(b"Properties").cloned() {
                Some(Object::Ref(pr)) => {
                    let mut props = doc.get(pr).as_dict().cloned().unwrap_or_default();
                    swap(&mut props);
                    doc.set(pr, Object::Dict(props));
                }
                Some(Object::Dict(mut props)) => {
                    swap(&mut props);
                    res.set(b"Properties".to_vec(), Object::Dict(props));
                }
                _ => {}
            }
            s.dict.set(b"Resources".to_vec(), Object::Dict(res));
            doc.set(form, Object::Stream(s));
        }
        _ => {}
    }
    // The copies leave the layer list.
    let dropped: HashSet<ObjRef> = same.keys().copied().collect();
    let Some(mut p) = ocproperties(doc) else { return Ok(()) };
    let keep = |r: ObjRef| !dropped.contains(&r);
    retain_refs(&mut p, b"OCGs", &keep);
    if let Some(mut config) = p.get(b"D").and_then(Object::as_dict).cloned() {
        for key in [&b"Order"[..], b"ON", b"OFF"] {
            retain_refs(&mut config, key, &keep);
        }
        p.set(b"D".to_vec(), Object::Dict(config));
    }
    write_ocproperties(doc, p)
}

/// Hide a layer in the default view (`/D /OFF`).
fn hide_layer(doc: &mut Document, id: ObjRef) -> Result<(), EditError> {
    let Some(mut p) = ocproperties(doc) else { return Ok(()) };
    let mut config = p.get(b"D").and_then(Object::as_dict).cloned().unwrap_or_default();
    let mut off = config.get(b"OFF").and_then(Object::as_array).cloned().unwrap_or_default();
    if !off.iter().any(|o| o.as_ref() == Some(id)) {
        off.push(Object::Ref(id));
    }
    config.set(b"OFF".to_vec(), Object::Array(off));
    p.set(b"D".to_vec(), Object::Dict(config));
    write_ocproperties(doc, p)
}

/// The scale of a drawing viewport, written as the drawing writes it.
pub(crate) fn viewport_scale(vp: &pdfcraft_create::cad::CadViewport) -> Option<measure::Scale> {
    let feet = vp.unit == "ft";
    let scale = measure::Scale::new(vp.real_per_point, vp.unit, if feet { 2 } else { 0 });
    let scale = if feet { scale.and_then(|s| s.with_feet_inches(16)) } else { scale };
    let mut scale = scale.ok()?;
    scale.ratio = vp.ratio.clone();
    Some(scale)
}

/// Put a drawing's viewport scales on a page, marked as the link's so an update replaces them.
pub(crate) fn set_viewports(doc: &mut Document, page: usize, viewports: &[&pdfcraft_create::cad::CadViewport]) -> Result<(), EditError> {
    let pages = pdfcraft_model::pages(doc);
    let p = pages.get(page).ok_or_else(|| invalid("no such sheet"))?.obj;
    // The link's earlier viewports go; any others (set by hand) stay.
    let own = doc.get(p).as_dict().cloned().unwrap_or_default();
    if let Some(list) = own.get(b"VP").map(|o| (*doc.resolve(o)).clone()).and_then(|o| o.as_array().cloned()) {
        let kept: Vec<Object> = list
            .into_iter()
            .filter(|o| !matches!(doc.resolve(o).as_dict().and_then(|d| d.get(b"PCLink").cloned()), Some(Object::Bool(true))))
            .collect();
        doc.update_dict(p, |d| d.set(b"VP".to_vec(), Object::Array(kept))).map_err(|e| invalid(&e.to_string()))?;
    }
    for vp in viewports {
        let Some(scale) = viewport_scale(vp) else { continue };
        // A viewport that can't be written is left to calibrate by hand.
        if measure::set_scale(doc, page, vp.bbox, &vp.name, &scale).is_err() {
            continue;
        }
        let own = doc.get(p).as_dict().cloned().unwrap_or_default();
        if let Some(mut list) = own.get(b"VP").map(|o| (*doc.resolve(o)).clone()).and_then(|o| o.as_array().cloned())
            && let Some(Object::Dict(last)) = list.last_mut()
        {
            last.set(b"PCLink".to_vec(), Object::Bool(true));
            doc.update_dict(p, |d| d.set(b"VP".to_vec(), Object::Array(list))).map_err(|e| invalid(&e.to_string()))?;
        }
    }
    Ok(())
}

/// Redraw link `index` from `bytes` (the file `name`, as it is now), keeping the markups. With
/// `keep_revision`, the drawing it replaces stays as a hidden layer under the new one.
pub(crate) fn update(doc: &mut Document, index: usize, name: &str, bytes: &[u8], file: &LinkedFile, keep_revision: bool) -> Result<(), EditError> {
    let mut list = records(doc);
    let rec = list.get(index).cloned().ok_or_else(|| invalid("that link doesn't exist"))?;
    let page_obj = rec.reference(b"Page").ok_or_else(|| invalid("that link has no sheet"))?;
    let page = pdfcraft_model::pages(doc).iter().position(|p| p.obj == page_obj).ok_or_else(|| invalid("that link's sheet is gone"))?;
    let layout = text(&rec, b"Layout");
    let is_placed = placed(&rec);
    if rec.name(b"Kind") == Some(&b"Table"[..]) {
        return update_table(doc, list, index, rec, page_obj, name, bytes, file, keep_revision);
    }
    // A placed drawing is plotted again at its sheet's scale; a sheet, its layout.
    let (pdf, sheets, plot) = if is_placed {
        let plot = pdfcraft_create::cad::plot_model_to_scale(bytes, name, num(doc, &rec, b"RIP"))?;
        (plot.pdf.clone(), None, Some(plot))
    } else {
        let sheets = pdfcraft_create::cad::to_pdf_layout(bytes, name, Some(&layout))?;
        (sheets.pdf.clone(), Some(sheets), None)
    };
    let plotted = Document::open(std::sync::Arc::new(pdf)).map_err(|e| invalid(&format!("{name}: {e}")))?;
    let before: HashMap<String, ObjRef> = pdfcraft_annot::layers::list(doc).into_iter().map(|l| (l.name, l.id)).collect();
    let (form, _) = pdfcraft_organize::page_as_form(doc, &plotted, 0)?;
    reuse_layers(doc, form, &before)?;
    let mut rec = rec;
    if keep_revision && let Some(old) = rec.reference(b"Form") {
        let mut revs = rec.get(b"Revs").map(|o| (*doc.resolve(o)).clone()).and_then(|o| o.as_array().cloned()).unwrap_or_default();
        if revs.len() < MAX_REVISIONS {
            let letter = char::from(b'A'.saturating_add(u8::try_from(revs.len()).unwrap_or(25)));
            let ocg = pdfcraft_annot::layers::add(doc, &format!("Rev {letter} · {layout}"), [0.55, 0.55, 0.55])?;
            hide_layer(doc, ocg)?;
            let mut rev = Dict::new();
            rev.set(b"Form".to_vec(), Object::Ref(old));
            rev.set(b"Matrix".to_vec(), num_array(&matrix(doc, &rec)));
            rev.set(b"OCG".to_vec(), Object::Ref(ocg));
            revs.push(Object::Dict(rev));
            rec.set(b"Revs".to_vec(), Object::Array(revs));
        }
    }
    // A placed drawing keeps its place: the same drawing point lands on the same sheet point,
    // however its extents changed.
    if let Some(plot) = &plot {
        let m = matrix(doc, &rec);
        let old_min = nums(doc, &rec, b"ModelMin");
        let old_ppu = num(doc, &rec, b"PPU");
        if let [mx, my] = old_min.as_slice() {
            let origin = [m[4] - mx * old_ppu, m[5] - my * old_ppu];
            let ppu = plot.points_per_unit;
            rec.set(b"Matrix".to_vec(), num_array(&[1.0, 0.0, 0.0, 1.0, origin[0] + plot.model_min[0] * ppu, origin[1] + plot.model_min[1] * ppu]));
        }
        rec.set(b"ModelMin".to_vec(), num_array(&plot.model_min));
        rec.set(b"PPU".to_vec(), Object::Real(plot.points_per_unit));
        rec.set(b"W".to_vec(), Object::Real(plot.size[0]));
        rec.set(b"H".to_vec(), Object::Real(plot.size[1]));
    }
    rec.set(b"Form".to_vec(), Object::Ref(form));
    rec.set(b"Size".to_vec(), Object::Int(i64::try_from(file.size).unwrap_or(i64::MAX)));
    rec.set(b"MTime".to_vec(), Object::Int(file.mtime));
    rec.set(b"Hash".to_vec(), PdfString::text(&fingerprint(bytes)));
    rec.set(b"Updated".to_vec(), Object::Int(file.now));
    if let Some(slot) = list.get_mut(index) {
        *slot = rec;
    }
    write_records(doc, list)?;
    redraw_page(doc, page_obj)?;
    match &sheets {
        Some(sheets) => {
            let vps: Vec<&pdfcraft_create::cad::CadViewport> = sheets.viewports.iter().filter(|v| v.page == 0).collect();
            set_viewports(doc, page, &vps)
        }
        // A placed drawing is at the sheet's scale already.
        None => Ok(()),
    }
}

/// Keep the form `rec` draws now as a hidden "Rev" layer under the new one.
fn keep_revision_of(doc: &mut Document, rec: &mut Dict, label: &str) -> Result<(), EditError> {
    let Some(old) = rec.reference(b"Form") else { return Ok(()) };
    let mut revs = rec.get(b"Revs").map(|o| (*doc.resolve(o)).clone()).and_then(|o| o.as_array().cloned()).unwrap_or_default();
    if revs.len() >= MAX_REVISIONS {
        return Ok(());
    }
    let letter = char::from(b'A'.saturating_add(u8::try_from(revs.len()).unwrap_or(25)));
    let ocg = pdfcraft_annot::layers::add(doc, &format!("Rev {letter} · {label}"), [0.55, 0.55, 0.55])?;
    hide_layer(doc, ocg)?;
    let mut rev = Dict::new();
    rev.set(b"Form".to_vec(), Object::Ref(old));
    rev.set(b"Matrix".to_vec(), num_array(&matrix(doc, rec)));
    rev.set(b"OCG".to_vec(), Object::Ref(ocg));
    revs.push(Object::Dict(rev));
    rec.set(b"Revs".to_vec(), Object::Array(revs));
    Ok(())
}

/// [`update`] for a table: printed again from the workbook, its top-left corner where it was.
#[allow(clippy::too_many_arguments)]
fn update_table(
    doc: &mut Document,
    mut list: Vec<Dict>,
    index: usize,
    mut rec: Dict,
    page_obj: ObjRef,
    name: &str,
    bytes: &[u8],
    file: &LinkedFile,
    keep_revision: bool,
) -> Result<(), EditError> {
    let (sheet, range) = (text(&rec, b"Sheet"), text(&rec, b"Range"));
    let plot = pdfcraft_create::table::plot_table(bytes, name, &sheet, &range)?;
    let plotted = Document::open(std::sync::Arc::new(plot.pdf.clone())).map_err(|e| invalid(&format!("{name}: {e}")))?;
    let (form, _) = pdfcraft_organize::page_as_form(doc, &plotted, 0)?;
    if keep_revision {
        let label = if sheet.is_empty() { name.to_string() } else { sheet.clone() };
        keep_revision_of(doc, &mut rec, &label)?;
    }
    // The table keeps its size (scale) and its top-left corner.
    let m = matrix(doc, &rec);
    let s = m[0];
    let top = m[5] + num(doc, &rec, b"H") * m[3];
    rec.set(b"Matrix".to_vec(), num_array(&[s, 0.0, 0.0, s, m[4], top - plot.size[1] * s]));
    rec.set(b"W".to_vec(), Object::Real(plot.size[0]));
    rec.set(b"H".to_vec(), Object::Real(plot.size[1]));
    rec.set(b"Form".to_vec(), Object::Ref(form));
    rec.set(b"Size".to_vec(), Object::Int(i64::try_from(file.size).unwrap_or(i64::MAX)));
    rec.set(b"MTime".to_vec(), Object::Int(file.mtime));
    rec.set(b"Hash".to_vec(), PdfString::text(&fingerprint(bytes)));
    rec.set(b"Updated".to_vec(), Object::Int(file.now));
    set_relative(&mut rec, file.relative.as_deref());
    if let Some(slot) = list.get_mut(index) {
        *slot = rec;
    }
    write_records(doc, list)?;
    redraw_page(doc, page_obj)
}

/// Place range `range` of sheet `sheet` of workbook `name` (`bytes`, from `file`) on sheet
/// `page` as a table, linked to the file, on its own layer: at the sheet's top left, or beside
/// what is placed there already. Returns the new link's index.
pub(crate) fn place_table(
    doc: &mut Document,
    page: usize,
    name: &str,
    bytes: &[u8],
    file: &LinkedFile,
    sheet: &str,
    range: &str,
) -> Result<usize, EditError> {
    let pages = pdfcraft_model::pages(doc);
    let p = pages.get(page).ok_or_else(|| invalid("no such sheet"))?;
    let (page_obj, page_dict) = (p.obj, p.dict.clone());
    let mbox = media_box(doc, &page_dict);
    let plot = pdfcraft_create::table::plot_table(bytes, name, sheet, range)?;
    let plotted = Document::open(std::sync::Arc::new(plot.pdf.clone())).map_err(|e| invalid(&format!("{name}: {e}")))?;
    let mut list = records(doc);
    if list.len() >= MAX_LINKS {
        return Err(invalid("the document has too many links"));
    }
    ensure_base(doc, page_obj, &page_dict)?;
    let (form, _) = pdfcraft_organize::page_as_form(doc, &plotted, 0)?;
    let file_name: String = file.path.rsplit(['/', '\\']).next().unwrap_or(name).chars().filter(|c| !c.is_control()).take(100).collect();
    let ocg = pdfcraft_annot::layers::add(doc, if file_name.trim().is_empty() { "Table" } else { &file_name }, [0.25, 0.25, 0.25])?;
    let (w, h) = (plot.size[0], plot.size[1]);
    let margin = 36.0;
    let mut at = [mbox[0] + margin, mbox[3] - margin - h];
    let others: Vec<[f64; 4]> = list.iter().filter(|r| r.reference(b"Page") == Some(page_obj) && placed(r)).map(|r| placed_rect(doc, r)).collect();
    let rect = [at[0], at[1], at[0] + w, at[1] + h];
    if others.iter().any(|o| overlap(o, &rect) > 0.0) {
        // Beside the others, along their top edge.
        let right = others.iter().map(|o| o[2]).fold(f64::MIN, f64::max);
        let top = others.iter().map(|o| o[3]).fold(f64::MIN, f64::max);
        at = [right + 18.0, top - h];
    }
    let index = list.len();
    let mut r = Dict::new();
    r.set(b"Kind".to_vec(), Object::name("Table"));
    r.set(b"Path".to_vec(), PdfString::text(&file.path));
    set_relative(&mut r, file.relative.as_deref());
    r.set(b"Layout".to_vec(), PdfString::text(sheet));
    r.set(b"Sheet".to_vec(), PdfString::text(sheet));
    r.set(b"Range".to_vec(), PdfString::text(range.trim()));
    r.set(b"Page".to_vec(), Object::Ref(page_obj));
    r.set(b"Form".to_vec(), Object::Ref(form));
    r.set(b"XName".to_vec(), Object::name(&new_xname(&list)));
    r.set(b"Size".to_vec(), Object::Int(i64::try_from(file.size).unwrap_or(i64::MAX)));
    r.set(b"MTime".to_vec(), Object::Int(file.mtime));
    r.set(b"Hash".to_vec(), PdfString::text(&fingerprint(bytes)));
    r.set(b"Updated".to_vec(), Object::Int(file.now));
    r.set(b"Placed".to_vec(), Object::Bool(true));
    r.set(b"OCG".to_vec(), Object::Ref(ocg));
    r.set(b"W".to_vec(), Object::Real(w));
    r.set(b"H".to_vec(), Object::Real(h));
    r.set(b"Matrix".to_vec(), num_array(&[1.0, 0.0, 0.0, 1.0, at[0], at[1]]));
    list.push(r);
    write_records(doc, list)?;
    redraw_page(doc, page_obj)?;
    Ok(index)
}

/// Keep what a sheet showed before drawings were placed on it, as a form drawn first.
fn ensure_base(doc: &mut Document, page: ObjRef, page_dict: &Dict) -> Result<(), EditError> {
    let own = doc.get(page).as_dict().cloned().unwrap_or_default();
    if own.contains(b"PCBase") || records(doc).iter().any(|r| r.reference(b"Page") == Some(page)) {
        return Ok(());
    }
    let content = decoded_contents(doc, page_dict)?;
    let resources = page_dict.get(b"Resources").map(|o| (*doc.resolve(o)).clone()).unwrap_or(Object::Dict(Dict::new()));
    let bbox = media_box(doc, page_dict);
    let mut fd = Dict::new();
    fd.set(b"Type".to_vec(), Object::name("XObject"));
    fd.set(b"Subtype".to_vec(), Object::name("Form"));
    fd.set(b"BBox".to_vec(), num_array(&bbox));
    fd.set(b"Resources".to_vec(), resources);
    let form = doc.add(Object::Stream(Stream::flate(fd, &content)));
    doc.update_dict(page, |d| d.set(b"PCBase".to_vec(), Object::Ref(form))).map_err(|e| invalid(&e.to_string()))
}

fn overlap(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    w * h
}

/// Place the model space of drawing `name` (`bytes`, from `file`) on sheet `page`, at the sheet's
/// scale (1/8" = 1'-0" when the sheet has none), on its own layer. Drawings placed on one sheet
/// share its drawing coordinates, so rooms drawn where they are in the building land in place;
/// one that would cover another goes beside them instead. Returns the new link's index.
pub(crate) fn place(doc: &mut Document, page: usize, name: &str, bytes: &[u8], file: &LinkedFile) -> Result<usize, EditError> {
    let pages = pdfcraft_model::pages(doc);
    let p = pages.get(page).ok_or_else(|| invalid("no such sheet"))?;
    let (page_obj, page_dict) = (p.obj, p.dict.clone());
    let mbox = media_box(doc, &page_dict);
    let centre = [(mbox[0] + mbox[2]) / 2.0, (mbox[1] + mbox[3]) / 2.0];
    let sheet_scale = measure::viewport_scale_at(doc, page, centre).ok().flatten();
    let rip = sheet_scale.as_ref().and_then(|s| s.points_for_inches(1.0)).filter(|p| p.is_finite() && *p > 0.0).map_or(96.0 / 72.0, |p| 1.0 / p);
    let plot = pdfcraft_create::cad::plot_model_to_scale(bytes, name, rip)?;
    let plotted = Document::open(std::sync::Arc::new(plot.pdf.clone())).map_err(|e| invalid(&format!("{name}: {e}")))?;
    let mut list = records(doc);
    if list.len() >= MAX_LINKS {
        return Err(invalid("the document has too many linked drawings"));
    }
    ensure_base(doc, page_obj, &page_dict)?;
    let before: HashMap<String, ObjRef> = pdfcraft_annot::layers::list(doc).into_iter().map(|l| (l.name, l.id)).collect();
    let (form, _) = pdfcraft_organize::page_as_form(doc, &plotted, 0)?;
    reuse_layers(doc, form, &before)?;
    let layer_name: String = std::path::Path::new(&file.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string())
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect();
    let ocg = pdfcraft_annot::layers::add(doc, if layer_name.trim().is_empty() { "Drawing" } else { &layer_name }, [0.25, 0.25, 0.25])?;
    // Where drawing (0, 0) is on this sheet: shared by every drawing placed on it.
    let ppu = plot.points_per_unit;
    let own = doc.get(page_obj).as_dict().cloned().unwrap_or_default();
    let origin = match nums(doc, &own, b"PCOrigin").as_slice() {
        [x, y] => [*x, *y],
        _ => {
            let o = [centre[0] - plot.size[0] / 2.0 - plot.model_min[0] * ppu, centre[1] - plot.size[1] / 2.0 - plot.model_min[1] * ppu];
            doc.update_dict(page_obj, |d| d.set(b"PCOrigin".to_vec(), num_array(&o))).map_err(|e| invalid(&e.to_string()))?;
            o
        }
    };
    let (w, h) = (plot.size[0], plot.size[1]);
    let mut at = [origin[0] + plot.model_min[0] * ppu, origin[1] + plot.model_min[1] * ppu];
    let others: Vec<[f64; 4]> = list.iter().filter(|r| r.reference(b"Page") == Some(page_obj) && placed(r)).map(|r| placed_rect(doc, r)).collect();
    let rect = [at[0], at[1], at[0] + w, at[1] + h];
    if others.iter().any(|o| overlap(o, &rect) > 0.5 * (w * h).min((o[2] - o[0]) * (o[3] - o[1]))) {
        // Drawn around their own origins: beside the others, along their bottom edge.
        let right = others.iter().map(|o| o[2]).fold(f64::MIN, f64::max);
        let bottom = others.iter().map(|o| o[1]).fold(f64::MAX, f64::min);
        at = [right + 18.0, bottom];
    }
    let index = list.len();
    let mut r = Dict::new();
    r.set(b"Path".to_vec(), PdfString::text(&file.path));
    set_relative(&mut r, file.relative.as_deref());
    r.set(b"Layout".to_vec(), PdfString::text("Model"));
    r.set(b"Page".to_vec(), Object::Ref(page_obj));
    r.set(b"Form".to_vec(), Object::Ref(form));
    r.set(b"XName".to_vec(), Object::name(&new_xname(&list)));
    r.set(b"Size".to_vec(), Object::Int(i64::try_from(file.size).unwrap_or(i64::MAX)));
    r.set(b"MTime".to_vec(), Object::Int(file.mtime));
    r.set(b"Hash".to_vec(), PdfString::text(&fingerprint(bytes)));
    r.set(b"Updated".to_vec(), Object::Int(file.now));
    r.set(b"Placed".to_vec(), Object::Bool(true));
    r.set(b"OCG".to_vec(), Object::Ref(ocg));
    r.set(b"RIP".to_vec(), Object::Real(rip));
    r.set(b"PPU".to_vec(), Object::Real(ppu));
    r.set(b"ModelMin".to_vec(), num_array(&plot.model_min));
    r.set(b"W".to_vec(), Object::Real(w));
    r.set(b"H".to_vec(), Object::Real(h));
    r.set(b"Matrix".to_vec(), num_array(&[1.0, 0.0, 0.0, 1.0, at[0], at[1]]));
    list.push(r);
    write_records(doc, list)?;
    redraw_page(doc, page_obj)?;
    // A sheet without a scale takes the drawings' scale, so it measures.
    if sheet_scale.is_none() {
        let feet = rip / 12.0;
        let vp = pdfcraft_create::cad::CadViewport {
            page,
            bbox: mbox,
            name: "Drawings".into(),
            real_per_point: feet,
            unit: "ft",
            ratio: pdfcraft_create::cad::ratio_label(feet, "ft"),
        };
        set_viewports(doc, page, &[&vp])?;
    }
    Ok(index)
}

/// Move placed drawing `index` by (`dx`, `dy`) points on its sheet.
pub(crate) fn move_link(doc: &mut Document, index: usize, dx: f64, dy: f64) -> Result<(), EditError> {
    if !(dx.is_finite() && dy.is_finite()) || dx.abs() > 1e5 || dy.abs() > 1e5 {
        return Err(invalid("that move is out of range"));
    }
    let mut list = records(doc);
    let rec = list.get(index).cloned().ok_or_else(|| invalid("that link doesn't exist"))?;
    if !placed(&rec) {
        return Err(invalid("only a drawing placed on a sheet can move"));
    }
    let page = rec.reference(b"Page").ok_or_else(|| invalid("that link has no sheet"))?;
    let m = matrix(doc, &rec);
    let moved = [m[0], m[1], m[2], m[3], m[4] + dx, m[5] + dy];
    if let Some(r) = list.get_mut(index) {
        r.set(b"Matrix".to_vec(), num_array(&moved));
    }
    write_records(doc, list)?;
    redraw_page(doc, page)
}

/// The smallest and largest a placed table is shown, against its size in its workbook.
pub const MIN_TABLE_SCALE: f64 = 0.1;
pub const MAX_TABLE_SCALE: f64 = 10.0;

/// Show placed table `index` at `scale` times its size in its workbook, its top-left corner where
/// it is.
pub(crate) fn resize_link(doc: &mut Document, index: usize, scale: f64) -> Result<(), EditError> {
    if !scale.is_finite() || !(MIN_TABLE_SCALE..=MAX_TABLE_SCALE).contains(&scale) {
        return Err(invalid("that size is out of range"));
    }
    let mut list = records(doc);
    let rec = list.get(index).cloned().ok_or_else(|| invalid("that link doesn't exist"))?;
    if !placed(&rec) || rec.name(b"Kind") != Some(&b"Table"[..]) {
        return Err(invalid("only a table placed on a sheet can be resized (a drawing keeps its sheet's scale)"));
    }
    let page = rec.reference(b"Page").ok_or_else(|| invalid("that link has no sheet"))?;
    let m = matrix(doc, &rec);
    let h = num(doc, &rec, b"H");
    let top = m[5] + h * m[3];
    if let Some(r) = list.get_mut(index) {
        r.set(b"Matrix".to_vec(), num_array(&[scale, 0.0, 0.0, scale, m[4], top - h * scale]));
    }
    write_records(doc, list)?;
    redraw_page(doc, page)
}

/// Take placed drawing or table `index` off its sheet: its link, what it draws and its layers
/// (its own and its earlier versions') go; the sheet and the other links stay as they are.
pub(crate) fn remove_link(doc: &mut Document, index: usize) -> Result<(), EditError> {
    let mut list = records(doc);
    let rec = list.get(index).cloned().ok_or_else(|| invalid("that link doesn't exist"))?;
    if !placed(&rec) {
        return Err(invalid("only a drawing or table placed on a sheet can be removed (a whole linked sheet is deleted as a page)"));
    }
    let page = rec.reference(b"Page").ok_or_else(|| invalid("that link has no sheet"))?;
    list.remove(index);
    // Its layers, unless another link draws on them.
    let layers_of = |r: &Dict| -> Vec<ObjRef> {
        let mut out: Vec<ObjRef> = r.reference(b"OCG").into_iter().collect();
        if let Some(Object::Array(revs)) = r.get(b"Revs").map(|o| (*doc.resolve(o)).clone()) {
            out.extend(revs.iter().filter_map(|v| doc.resolve(v).as_dict().and_then(|d| d.reference(b"OCG"))));
        }
        out
    };
    let used: HashSet<ObjRef> = list.iter().flat_map(&layers_of).collect();
    let gone: HashSet<ObjRef> = layers_of(&rec).into_iter().filter(|l| !used.contains(l)).collect();
    write_records(doc, list)?;
    redraw_page(doc, page)?;
    if !gone.is_empty()
        && let Some(mut props) = ocproperties(doc)
    {
        let keep = |r: ObjRef| !gone.contains(&r);
        retain_refs(&mut props, b"OCGs", &keep);
        if let Some(mut config) = props.get(b"D").and_then(Object::as_dict).cloned() {
            for k in [&b"Order"[..], b"ON", b"OFF"] {
                retain_refs(&mut config, k, &keep);
            }
            props.set(b"D".to_vec(), Object::Dict(config));
        }
        write_ocproperties(doc, props)?;
    }
    Ok(())
}

/// Record (or, `None`, clear) a link's path relative to the PDF. One that couldn't be a relative
/// path (empty, absolute, too long, control characters) isn't recorded.
fn set_relative(r: &mut Dict, relative: Option<&str>) {
    match relative.map(str::trim).filter(|p| valid_relative(p)) {
        Some(p) => r.set(b"RelPath".to_vec(), PdfString::text(p)),
        None => {
            r.remove(b"RelPath");
        }
    }
}

fn valid_relative(p: &str) -> bool {
    !p.is_empty() && p.len() <= 4096 && !p.chars().any(char::is_control) && !p.starts_with(['/', '\\']) && !p.contains(':')
}

/// Point link `index` at a file somewhere else (and, when known, where that is from the PDF).
pub(crate) fn relink(doc: &mut Document, index: usize, path: &str, relative: Option<&str>) -> Result<(), EditError> {
    let path = path.trim();
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(invalid("that isn't a file path"));
    }
    let mut list = records(doc);
    let rec = list.get_mut(index).ok_or_else(|| invalid("that link doesn't exist"))?;
    rec.set(b"Path".to_vec(), PdfString::text(path));
    set_relative(rec, relative);
    write_records(doc, list)
}
