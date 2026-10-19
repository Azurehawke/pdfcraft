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

/// A linked drawing, as the Links pane shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct CadLink {
    /// Its position in the document's list of links.
    pub index: usize,
    /// The sheet (page index) it draws on.
    pub page: usize,
    pub path: String,
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
}

/// The file a sheet is linked to, as read from disk by the app.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkedFile {
    pub path: String,
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
                index,
                page,
                path: text(r, b"Path"),
                layout: text(r, b"Layout"),
                size: u64::try_from(int(doc, r, b"Size")).unwrap_or(0),
                mtime: int(doc, r, b"MTime"),
                fingerprint: text(r, b"Hash"),
                updated: int(doc, r, b"Updated"),
                revisions: r.get(b"Revs").and_then(|o| doc.resolve(o).as_array().map(Vec::len)).unwrap_or(0),
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
    for r in records(doc).iter().filter(|r| r.reference(b"Page") == Some(page)) {
        let name = r.name(b"XName").map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_else(|| "PCLink".into());
        if let Some(Object::Array(revs)) = r.get(b"Revs").map(|o| (*doc.resolve(o)).clone()) {
            for (k, rev) in revs.iter().enumerate() {
                let Some(rev) = doc.resolve(rev).as_dict().cloned() else { continue };
                let (Some(form), Some(ocg)) = (rev.reference(b"Form"), rev.reference(b"OCG")) else { continue };
                let (x, o) = (format!("{name}R{k}"), format!("{name}O{k}"));
                xobjects.set(x.clone().into_bytes(), Object::Ref(form));
                props.set(o.clone().into_bytes(), Object::Ref(ocg));
                content.push_str(&format!("/OC /{o} BDC q /{x} Do Q EMC\n"));
            }
        }
        if let Some(form) = r.reference(b"Form") {
            xobjects.set(name.clone().into_bytes(), Object::Ref(form));
            content.push_str(&format!("q /{name} Do Q\n"));
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
    let sheets = pdfcraft_create::cad::to_pdf_layout(bytes, name, Some(&layout))?;
    let plotted = Document::open(std::sync::Arc::new(sheets.pdf.clone())).map_err(|e| invalid(&format!("{name}: {e}")))?;
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
            rev.set(b"OCG".to_vec(), Object::Ref(ocg));
            revs.push(Object::Dict(rev));
            rec.set(b"Revs".to_vec(), Object::Array(revs));
        }
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
    let vps: Vec<&pdfcraft_create::cad::CadViewport> = sheets.viewports.iter().filter(|v| v.page == 0).collect();
    set_viewports(doc, page, &vps)
}

/// Point link `index` at a file somewhere else.
pub(crate) fn relink(doc: &mut Document, index: usize, path: &str) -> Result<(), EditError> {
    let path = path.trim();
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(invalid("that isn't a file path"));
    }
    let mut list = records(doc);
    let rec = list.get_mut(index).ok_or_else(|| invalid("that link doesn't exist"))?;
    rec.set(b"Path".to_vec(), PdfString::text(path));
    write_records(doc, list)
}
