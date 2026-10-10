//! Make text that a page draws through form XObjects editable.
//!
//! Text editing ([`crate::text_lines`]) rewrites a page's own content streams, so text inside a
//! form XObject (common in files from design tools, in flattened form fields, and in pages
//! re-saved by note-taking apps) can be read but not edited. [`unpack_text_forms`] draws such
//! forms into the page itself: each `/Name Do` of a form holding text becomes
//! `q <Matrix> cm <BBox> re W n …the form's content… Q` (ISO 32000-2 §8.10.1: what the `Do`
//! paints), with the form's resources added to the page's under new names. The page looks the
//! same; the form objects stay in the file for anything else that draws them.
//!
//! Forms that aren't plain content are left alone: transparency groups (`/Group`; drawing their
//! content directly could change how it blends) and optional content (`/OC`).

use pdfcraft_content::{Op, parse, serialize_ops};
use pdfcraft_cos::{Dict, Document, ObjRef, Object, Stream};

use crate::EditError;
use crate::text::{MAX_FORM_DEPTH, MAX_FORM_VISITS};

/// Resource categories whose names content refers to.
const CATEGORIES: [&[u8]; 7] = [b"Font", b"XObject", b"ExtGState", b"ColorSpace", b"Pattern", b"Shading", b"Properties"];

/// Whether `page` (0-based) draws text from inside form XObjects that [`unpack_text_forms`]
/// would make editable.
pub fn has_text_in_forms(doc: &Document, page: usize) -> bool {
    let Some(p) = pdfcraft_model::pages(doc).into_iter().nth(page) else { return false };
    let res = resolved_dict(doc, p.dict.get(b"Resources"));
    let mut budget = MAX_FORM_VISITS;
    content(doc, &p.dict).iter().any(|(_, data)| {
        parse(data).ops.iter().any(|op| {
            op.op == b"Do"
                && op.operands.first().and_then(Object::as_name).is_some_and(|n| form_with_text(doc, &res, n, 0, &mut budget, &[]).is_some())
        })
    })
}

/// Draw the text-holding form XObjects of `page` (0-based) into its content (module docs).
/// Returns how many forms were drawn in; 0 leaves the page as it was.
pub fn unpack_text_forms(doc: &mut Document, page: usize) -> Result<usize, EditError> {
    let p = pdfcraft_model::pages(doc).into_iter().nth(page).ok_or(EditError::NoSuchPage(page))?;
    let page_res = resolved_dict(doc, p.dict.get(b"Resources"));
    let mut u =
        Unpack { doc, added: CATEGORIES.iter().map(|c| (c.to_vec(), Dict::new())).collect(), next: 0, visits: 0, path: Vec::new(), unpacked: 0 };
    let page_scope = Scope { res: page_res.clone(), prefix: None };
    let streams = content(doc, &p.dict);
    let mut list = Vec::with_capacity(streams.len());
    let mut changed = false;
    for (obj, data) in &streams {
        // PdfCraft's own marks (headers, watermarks, wrappers) stay as they are.
        if crate::stream_tag(doc, obj).is_some() {
            list.push(obj.clone());
            continue;
        }
        let before = u.unpacked;
        let mut out = Vec::new();
        u.ops(parse(data).ops, &page_scope, &mut out);
        if u.unpacked == before {
            list.push(obj.clone());
            continue;
        }
        changed = true;
        let mut dict = match &*doc.resolve(obj) {
            Object::Stream(s) => s.dict.clone(),
            _ => Dict::new(),
        };
        dict.remove(b"Length");
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
        list.push(Object::Stream(Stream::flate(dict, &serialize_ops(&out))));
    }
    if !changed {
        return Ok(0);
    }
    let unpacked = u.unpacked;
    let added = u.added;
    // The page gets its own (possibly copied) resources, with the forms' added under new names.
    let mut res = page_res;
    for (cat, new) in added {
        if new.is_empty() {
            continue;
        }
        let mut d = resolved_dict(doc, res.get(&cat));
        for (k, v) in new.iter() {
            d.set(k.clone(), v.clone());
        }
        res.set(cat, Object::Dict(d));
    }
    let list: Vec<Object> = list.into_iter().map(|o| if let Object::Stream(_) = o { Object::Ref(doc.add(o)) } else { o }).collect();
    let contents = match <[Object; 1]>::try_from(list) {
        Ok([one]) => one,
        Err(many) => Object::Array(many),
    };
    doc.update_dict(p.obj, |d| {
        d.set(b"Resources".to_vec(), Object::Dict(res));
        d.set(b"Contents".to_vec(), contents);
    })?;
    Ok(unpacked)
}

fn resolved_dict(doc: &Document, o: Option<&Object>) -> Dict {
    o.map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default()
}

/// A page's content streams and their decoded bytes.
fn content(doc: &Document, page: &Dict) -> Vec<(Object, Vec<u8>)> {
    let list: Vec<Object> = match page.get(b"Contents") {
        None => Vec::new(),
        Some(c) => match &*doc.resolve(c) {
            Object::Array(a) => a.clone(),
            _ => vec![c.clone()],
        },
    };
    list.into_iter()
        .map(|o| {
            let data = match &*doc.resolve(&o) {
                Object::Stream(s) => s.decoded().unwrap_or_default(),
                _ => Vec::new(),
            };
            (o, data)
        })
        .collect()
}

/// A form XObject to draw in: its object, content, resources (`None`: it has none of its own and
/// uses the ones it was drawn with), matrix and bounding box.
struct Form {
    obj: Option<ObjRef>,
    data: Vec<u8>,
    resources: Option<Dict>,
    matrix: Option<[f64; 6]>,
    bbox: Option<[f64; 4]>,
}

fn nums<const N: usize>(doc: &Document, o: Option<&Object>) -> Option<[f64; N]> {
    let a = o.map(|o| doc.resolve(o))?;
    let v: Vec<f64> = a.as_array()?.iter().filter_map(|x| doc.resolve(x).as_f64()).collect();
    <[f64; N]>::try_from(v).ok().filter(|v| v.iter().all(|x| x.is_finite()))
}

/// The form XObject `name` in `res` if it is plain content (no transparency group or optional
/// content) and draws text, directly or through forms it draws. `budget` bounds the forms looked
/// into; `path` holds the forms being drawn (a form may refer to itself).
fn form_with_text(doc: &Document, res: &Dict, name: &[u8], depth: usize, budget: &mut usize, path: &[ObjRef]) -> Option<Form> {
    if depth >= MAX_FORM_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;
    let xobjects = resolved_dict(doc, res.get(b"XObject"));
    let o = xobjects.get(name)?;
    let obj = o.as_ref();
    if obj.is_some_and(|r| path.contains(&r)) {
        return None;
    }
    let Object::Stream(s) = &*doc.resolve(o) else { return None };
    if s.dict.name(b"Subtype") != Some(b"Form") || s.dict.contains(b"Group") || s.dict.contains(b"OC") {
        return None;
    }
    let data = s.decoded().ok()?;
    let own = s.dict.get(b"Resources").map(|r| doc.resolve(r)).and_then(|r| r.as_dict().cloned());
    let inner_res = own.clone().unwrap_or_else(|| res.clone());
    let mut inner_path = path.to_vec();
    inner_path.extend(obj);
    let ops = parse(&data).ops;
    let text = ops.iter().any(|op| op.op == b"BT")
        || ops.iter().any(|op| {
            op.op == b"Do"
                && op
                    .operands
                    .first()
                    .and_then(Object::as_name)
                    .is_some_and(|n| form_with_text(doc, &inner_res, n, depth + 1, budget, &inner_path).is_some())
        });
    text.then(|| Form { obj, data, resources: own, matrix: nums(doc, s.dict.get(b"Matrix")), bbox: nums(doc, s.dict.get(b"BBox")) })
}

/// Whether `data` (drawn with `res`) paints anything but text: a filled or stroked path, a
/// shading, an image, or a form that does. Fill and stroke operators with no path to paint
/// (exported field appearances carry them) paint nothing. Past the depth or visit budget it is
/// taken to paint, which keeps the clip.
fn paints(doc: &Document, data: &[u8], res: &Dict, depth: usize, budget: &mut usize) -> bool {
    if depth >= MAX_FORM_DEPTH || *budget == 0 {
        return true;
    }
    *budget -= 1;
    let mut path = false;
    for o in parse(data).ops {
        match o.op.as_slice() {
            b"m" | b"l" | b"c" | b"v" | b"y" | b"h" | b"re" => path = true,
            b"n" => path = false,
            b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"S" | b"s" => {
                if path {
                    return true;
                }
            }
            b"sh" | b"BI" => return true,
            b"Do" => {
                let Some(name) = o.operands.first().and_then(Object::as_name) else { continue };
                let xobjects = resolved_dict(doc, res.get(b"XObject"));
                let Some(x) = xobjects.get(name) else { continue };
                let Object::Stream(s) = &*doc.resolve(x) else { return true };
                if s.dict.name(b"Subtype") != Some(b"Form") {
                    return true;
                }
                let inner = s.dict.get(b"Resources").map(|r| doc.resolve(r)).and_then(|r| r.as_dict().cloned()).unwrap_or_else(|| res.clone());
                if paints(doc, &s.decoded().unwrap_or_default(), &inner, depth + 1, budget) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// One level of content: its resources, and the prefix its names get in the page (`None`: the
/// page's own content, whose names stay).
struct Scope {
    res: Dict,
    prefix: Option<String>,
}

struct Unpack<'a> {
    doc: &'a Document,
    /// Resources to add to the page, by category, under their new names.
    added: Vec<(Vec<u8>, Dict)>,
    next: usize,
    visits: usize,
    path: Vec<ObjRef>,
    unpacked: usize,
}

fn op(name: &[u8], operands: Vec<Object>) -> Op {
    Op { op: name.to_vec(), operands, inline: None, span: 0..0 }
}

impl Unpack<'_> {
    /// `name` of category `cat` as the page knows it: the page's own names stay; a form's are
    /// prefixed and its resource is added to the page. Names that aren't resources (device
    /// colour spaces) stay.
    fn rename(&mut self, scope: &Scope, cat: &[u8], name: &[u8]) -> Vec<u8> {
        let Some(prefix) = &scope.prefix else { return name.to_vec() };
        let dict = resolved_dict(self.doc, scope.res.get(cat));
        let Some(value) = dict.get(name) else { return name.to_vec() };
        let mut new = prefix.as_bytes().to_vec();
        new.extend_from_slice(name);
        if let Some((_, d)) = self.added.iter_mut().find(|(c, _)| c.as_slice() == cat) {
            d.set(new.clone(), value.clone());
        }
        new
    }

    fn rename_operand(&mut self, scope: &Scope, cat: &[u8], o: &mut Object) {
        if let Some(name) = o.as_name().map(<[u8]>::to_vec) {
            *o = Object::Name(self.rename(scope, cat, &name));
        }
    }

    /// Copy `ops` to `out`: forms with text are drawn in (recursively), names renamed for the page.
    fn ops(&mut self, ops: Vec<Op>, scope: &Scope, out: &mut Vec<Op>) {
        for mut o in ops {
            if o.op == b"Do"
                && let Some(name) = o.operands.first().and_then(Object::as_name).map(<[u8]>::to_vec)
                && self.visits < MAX_FORM_VISITS
                && self.path.len() < MAX_FORM_DEPTH
            {
                let mut budget = MAX_FORM_VISITS;
                if let Some(form) = form_with_text(self.doc, &scope.res, &name, self.path.len(), &mut budget, &self.path) {
                    self.visits += 1;
                    self.unpacked += 1;
                    self.draw_in(form, scope, out);
                    continue;
                }
            }
            match o.op.as_slice() {
                b"Tf" => {
                    if let Some(x) = o.operands.first_mut() {
                        self.rename_operand(scope, b"Font", x);
                    }
                }
                b"Do" => {
                    if let Some(x) = o.operands.first_mut() {
                        self.rename_operand(scope, b"XObject", x);
                    }
                }
                b"gs" => {
                    if let Some(x) = o.operands.first_mut() {
                        self.rename_operand(scope, b"ExtGState", x);
                    }
                }
                b"sh" => {
                    if let Some(x) = o.operands.first_mut() {
                        self.rename_operand(scope, b"Shading", x);
                    }
                }
                b"cs" | b"CS" => {
                    if let Some(x) = o.operands.first_mut() {
                        self.rename_operand(scope, b"ColorSpace", x);
                    }
                }
                b"scn" | b"SCN" => {
                    if let Some(x) = o.operands.last_mut() {
                        self.rename_operand(scope, b"Pattern", x);
                    }
                }
                b"BDC" | b"DP" => {
                    if let Some(x) = o.operands.get_mut(1) {
                        self.rename_operand(scope, b"Properties", x);
                    }
                }
                b"BI" => {
                    if let Some((dict, _)) = o.inline.as_mut() {
                        for key in [&b"CS"[..], b"ColorSpace"] {
                            if let Some(mut cs) = dict.get(key).cloned() {
                                self.rename_operand(scope, b"ColorSpace", &mut cs);
                                dict.set(key.to_vec(), cs);
                            }
                        }
                    }
                }
                _ => {}
            }
            out.push(o);
        }
    }

    /// What `Do` of `form` paints, as page content.
    fn draw_in(&mut self, form: Form, outer: &Scope, out: &mut Vec<Op>) {
        let scope = match form.resources {
            Some(res) => {
                self.next += 1;
                Scope { res, prefix: Some(format!("PCu{}_", self.next)) }
            }
            // No resources of its own: it uses the ones it was drawn with, under their names.
            None => Scope { res: outer.res.clone(), prefix: outer.prefix.clone() },
        };
        out.push(op(b"q", Vec::new()));
        if let Some(m) = form.matrix.filter(|m| *m != [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]) {
            out.push(op(b"cm", m.iter().map(|x| Object::Real(*x)).collect()));
        }
        // The form's bounding box clips what it draws. A form that draws nothing but text (a
        // flattened field's value, a label) is drawn in without it: the box is the field's, as
        // tall as one line, and text edited longer would rewrap below it and be cut off.
        let mut budget = MAX_FORM_VISITS;
        let paints_more_than_text = paints(self.doc, &form.data, &scope.res, self.path.len(), &mut budget);
        if let Some([x0, y0, x1, y1]) = form.bbox.filter(|_| paints_more_than_text) {
            let (x, y) = (x0.min(x1), y0.min(y1));
            out.push(op(b"re", [x, y, (x1 - x0).abs(), (y1 - y0).abs()].iter().map(|v| Object::Real(*v)).collect()));
            out.push(op(b"W", Vec::new()));
            out.push(op(b"n", Vec::new()));
        }
        if let Some(r) = form.obj {
            self.path.push(r);
        }
        self.ops(parse(&form.data).ops, &scope, out);
        if form.obj.is_some() {
            self.path.pop();
        }
        out.push(op(b"Q", Vec::new()));
    }
}
