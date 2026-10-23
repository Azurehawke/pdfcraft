//! RevuCraft's photo callouts: a callout (a FreeText annotation with a leader line, §12.5.6.6)
//! whose box shows a photo scaled down to fit and a description beside it, on the side the user
//! chooses. The photo is a JPEG image XObject (`/PCPhotoImage`), drawn by the appearance, so the
//! callout prints with the drawing in any viewer.
//!
//! Keys: `/PCPhotoImage` (the image), `/PCPhotoAspect` (its width over its height),
//! `/PCPhotoH` (the photo's height in points when placed), `/PCBoxW` (the box's width then, so a
//! wider box shows a larger photo), `/PCFrame` (the frame and leader colour) and `/PCTextSide`
//! (`/Left` or `/Right`: where the description goes).

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString, Stream};

use crate::{
    AnnotError, LineEnding, Meta, NewAnnotation, Rgb, Shape, Style, add_annotation, annot_dict, annot_ref, finite, num_array, set_appearance, touch,
};

/// Padding inside the box, the gap between photo and text, the text size and line height, and
/// the description's width when placed (points).
pub const PAD: f64 = 6.0;
pub const GAP: f64 = 8.0;
pub const SIZE: f64 = 9.0;
pub const LINE: f64 = SIZE * 1.25;
pub const TEXT_W: f64 = 150.0;
/// The largest photo accepted (bytes).
pub const MAX_PHOTO: usize = 40 * 1024 * 1024;

/// Which side of the photo the description goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSide {
    Left,
    Right,
}

impl TextSide {
    pub fn name(self) -> &'static str {
        match self {
            TextSide::Left => "Left",
            TextSide::Right => "Right",
        }
    }
}

/// A photo callout to place.
#[derive(Clone, Debug, PartialEq)]
pub struct NewPhotoCallout {
    pub page: usize,
    /// The spot the leader points at.
    pub point: [f64; 2],
    /// The box's centre.
    pub center: [f64; 2],
    /// The photo as a baseline RGB JPEG, and its size in pixels.
    pub jpeg: Vec<u8>,
    pub pixels: (u32, u32),
    /// The photo's height on the sheet, points.
    pub photo_height: f64,
    pub text: String,
    pub side: TextSide,
    pub color: Rgb,
    pub author: String,
}

/// Whether the annotation dictionary is a photo callout.
pub fn is_photo_callout(d: &Dict) -> bool {
    d.name(b"Subtype") == Some(b"FreeText") && d.get(b"PCPhotoImage").is_some()
}

/// The description's title (its first line) and the rest.
fn split_text(text: &str) -> (&str, &str) {
    match text.split_once('\n') {
        Some((title, rest)) => (title.trim(), rest.trim()),
        None => (text.trim(), ""),
    }
}

/// How many lines the description takes at `width`.
fn line_count(text: &str, width: f64) -> usize {
    let (title, rest) = split_text(text);
    let title_lines = if title.is_empty() { 0 } else { crate::appearance::wrap(title, SIZE * 1.06, width).len() };
    let rest_lines = if rest.is_empty() { 0 } else { crate::appearance::wrap(rest, SIZE, width).len() };
    title_lines + rest_lines
}

/// The box (x0, y0, x1, y1) for a photo `ph` points high with `aspect`, and `text`, centred on
/// `center`.
fn box_for(center: [f64; 2], ph: f64, aspect: f64, text: &str) -> [f64; 4] {
    let pw = ph * aspect;
    let text_h = line_count(text, TEXT_W) as f64 * LINE;
    let w = PAD + pw + GAP + TEXT_W + PAD;
    let h = ph.max(text_h) + 2.0 * PAD;
    [center[0] - w / 2.0, center[1] - h / 2.0, center[0] + w / 2.0, center[1] + h / 2.0]
}

/// The point on the box's outline nearest `p` (where the leader meets it).
pub(crate) fn nearest_on_box(b: [f64; 4], p: [f64; 2]) -> [f64; 2] {
    let x = p[0].clamp(b[0], b[2]);
    let y = p[1].clamp(b[1], b[3]);
    if x > b[0] && x < b[2] && y > b[1] && y < b[3] {
        // Inside: the nearest edge.
        let d = [x - b[0], b[2] - x, y - b[1], b[3] - y];
        let m = d.iter().copied().fold(f64::MAX, f64::min);
        return if m == d[0] {
            [b[0], y]
        } else if m == d[1] {
            [b[2], y]
        } else if m == d[2] {
            [x, b[1]]
        } else {
            [x, b[3]]
        };
    }
    [x, y]
}

/// The photo as an image XObject.
fn image(jpeg: &[u8], pixels: (u32, u32)) -> Stream {
    let mut d = Dict::new();
    d.set(b"Type".to_vec(), Object::name("XObject"));
    d.set(b"Subtype".to_vec(), Object::name("Image"));
    d.set(b"Width".to_vec(), Object::Int(i64::from(pixels.0)));
    d.set(b"Height".to_vec(), Object::Int(i64::from(pixels.1)));
    d.set(b"ColorSpace".to_vec(), Object::name("DeviceRGB"));
    d.set(b"BitsPerComponent".to_vec(), Object::Int(8));
    d.set(b"Filter".to_vec(), Object::name("DCTDecode"));
    Stream::from_raw(d, jpeg.to_vec())
}

/// Place a photo callout. Returns its index in the page's annotations.
pub fn add(doc: &mut Document, new: &NewPhotoCallout, meta: &Meta) -> Result<usize, AnnotError> {
    if new.jpeg.is_empty() || new.jpeg.len() > MAX_PHOTO {
        return Err(AnnotError::Invalid("the photo is empty or too large".into()));
    }
    let (pw, ph) = new.pixels;
    if pw == 0 || ph == 0 || pw > 30_000 || ph > 30_000 {
        return Err(AnnotError::Invalid("the photo's size can't be read".into()));
    }
    if !finite(&new.point) || !finite(&new.center) || !new.photo_height.is_finite() || !(12.0..=2000.0).contains(&new.photo_height) {
        return Err(AnnotError::Invalid("invalid photo callout placement".into()));
    }
    if new.text.chars().count() > 4000 {
        return Err(AnnotError::Invalid("the description is too long".into()));
    }
    let aspect = (f64::from(pw) / f64::from(ph)).clamp(0.25, 4.0);
    let tb = box_for(new.center, new.photo_height, aspect, &new.text);
    let knee = nearest_on_box(tb, new.point);
    let shape = Shape::Callout { rect: tb, knee, point: new.point, font_size: SIZE, ending: LineEnding::None };
    let style = Style { color: new.color, opacity: 1.0, width: 1.5, fill: Some([1.0, 1.0, 1.0]) };
    let index = add_annotation(doc, &NewAnnotation { page: new.page, shape, style, contents: new.text.clone(), author: new.author.clone() }, meta)?;
    let picture = doc.add(Object::Stream(image(&new.jpeg, new.pixels)));
    let (_, r) = annot_ref(doc, new.page, index)?;
    doc.update_dict(r, |d| {
        d.set(b"PCPhotoImage".to_vec(), Object::Ref(picture));
        d.set(b"PCPhotoAspect".to_vec(), Object::Real(aspect));
        d.set(b"PCPhotoH".to_vec(), Object::Real(new.photo_height));
        d.set(b"PCBoxW".to_vec(), Object::Real(tb[2] - tb[0]));
        d.set(b"PCFrame".to_vec(), num_array(&new.color.map(|c| c.clamp(0.0, 1.0))));
        d.set(b"PCTextSide".to_vec(), Object::name(new.side.name()));
        d.set(b"Subj".to_vec(), PdfString::text("Photo callout"));
        d.remove(b"RC");
    })?;
    set_appearance(doc, r)?;
    Ok(index)
}

/// Put the description on the other side (or a given side) of the photo.
pub fn set_side(doc: &mut Document, page: usize, index: usize, side: TextSide, meta: &Meta) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    if !is_photo_callout(&annot_dict(doc, r)) {
        return Err(AnnotError::Invalid("not a photo callout".into()));
    }
    doc.update_dict(r, |d| {
        d.set(b"PCTextSide".to_vec(), Object::name(side.name()));
        touch(d, meta);
    })?;
    set_appearance(doc, r)
}

/// The side the description is on.
pub fn side_of(d: &Dict) -> TextSide {
    if d.name(b"PCTextSide") == Some(b"Left") { TextSide::Left } else { TextSide::Right }
}

/// The annotation dictionary at `(page, index)`, if there is one.
pub fn dict_at(doc: &Document, page: usize, index: usize) -> Option<Dict> {
    let p = crate::page_refs(doc).ok()?.get(page).copied()?;
    let entry = crate::annots(doc, p).get(index).cloned()?;
    doc.resolve(&entry).as_dict().cloned()
}

/// The JPEG of the photo callout at `(page, index)`, for showing it full size.
pub fn photo_jpeg(doc: &Document, page: usize, index: usize) -> Option<Vec<u8>> {
    let d = dict_at(doc, page, index)?;
    let r: ObjRef = d.get(b"PCPhotoImage")?.as_ref()?;
    let obj = doc.get(r);
    match &*obj {
        Object::Stream(s) if s.dict.name(b"Filter") == Some(b"DCTDecode") => Some(s.raw.to_vec()),
        _ => None,
    }
}

/// The file held by the file attachment (a photo pin) at `(page, index)`.
pub fn attachment_bytes(doc: &Document, page: usize, index: usize) -> Option<Vec<u8>> {
    let d = dict_at(doc, page, index)?;
    let fs = doc.resolve(d.get(b"FS")?).as_dict()?.clone();
    let ef = doc.resolve(fs.get(b"EF")?).as_dict()?.clone();
    let file = doc.resolve(ef.get(b"F").or_else(|| ef.get(b"UF"))?);
    match &*file {
        Object::Stream(s) => s.decoded_within(MAX_PHOTO).ok(),
        _ => None,
    }
}

/// The text box a photo callout needs for `text`: the width and top edge stay; the height fits
/// the photo and the wrapped description.
pub(crate) fn fitted(d: &Dict, tb: [f64; 4], text: &str) -> Option<[f64; 4]> {
    let (aspect, ph) = (num(d, b"PCPhotoAspect")?, num(d, b"PCPhotoH")?);
    let w = tb[2] - tb[0];
    let scale = num(d, b"PCBoxW").filter(|v| *v > 1.0).map_or(1.0, |b| (w / b).clamp(0.25, 8.0));
    let ph = ph * scale;
    let text_w = (w - 2.0 * PAD - GAP - ph * aspect).max(40.0);
    let h = ph.max(line_count(text, text_w) as f64 * LINE) + 2.0 * PAD;
    let top = tb[1].max(tb[3]);
    Some([tb[0], top - h, tb[2], top])
}

fn num(d: &Dict, k: &[u8]) -> Option<f64> {
    d.get(k).and_then(Object::as_f64).filter(|v| v.is_finite() && *v > 0.0)
}

/// Where the photo and the description go in the box `tb`: the photo's rectangle, and the text
/// column's left edge and width.
pub(crate) fn layout(d: &Dict, tb: [f64; 4]) -> Option<([f64; 4], f64, f64)> {
    let aspect = num(d, b"PCPhotoAspect")?;
    let (w, h) = (tb[2] - tb[0], tb[3] - tb[1]);
    let scale = num(d, b"PCBoxW").filter(|v| *v > 1.0).map_or(1.0, |b| (w / b).clamp(0.25, 8.0));
    let mut ph = num(d, b"PCPhotoH").map_or(h - 2.0 * PAD, |p| p * scale).min(h - 2.0 * PAD);
    let mut pw = ph * aspect;
    // The photo takes at most two thirds of the box.
    if pw > (w - 2.0 * PAD) * 0.66 {
        pw = (w - 2.0 * PAD) * 0.66;
        ph = pw / aspect;
    }
    if pw < 1.0 || ph < 1.0 {
        return None;
    }
    let top = tb[3] - PAD;
    let (photo_x, text_x) = match side_of(d) {
        TextSide::Right => (tb[0] + PAD, tb[0] + PAD + pw + GAP),
        TextSide::Left => (tb[2] - PAD - pw, tb[0] + PAD),
    };
    let text_w = (w - 2.0 * PAD - pw - GAP).max(1.0);
    Some(([photo_x, top - ph, photo_x + pw, top], text_x, text_w))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn blank() -> Document {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
        Document::open(Arc::new(pdf.to_vec())).unwrap()
    }

    #[test]
    fn photo_callouts_hold_the_photo_and_flip_sides() {
        let mut doc = blank();
        let new = NewPhotoCallout {
            page: 0,
            point: [100.0, 100.0],
            center: [300.0, 250.0],
            jpeg: vec![0xFF, 0xD8, 0xFF, 0xD9],
            pixels: (400, 300),
            photo_height: 72.0,
            text: "Rack A\n24-port switch".into(),
            side: TextSide::Right,
            color: [0.18, 0.44, 0.69],
            author: "Steve".into(),
        };
        let i = add(&mut doc, &new, &Meta::default()).unwrap();
        assert_eq!(photo_jpeg(&doc, 0, i), Some(vec![0xFF, 0xD8, 0xFF, 0xD9]));
        let p = crate::page_refs(&doc).unwrap()[0];
        let d = doc.resolve(&crate::annots(&doc, p)[i]).as_dict().unwrap().clone();
        assert!(is_photo_callout(&d));
        let ap = doc.resolve(d.get(b"AP").unwrap());
        let n = doc.resolve(ap.as_dict().unwrap().get(b"N").unwrap());
        let Object::Stream(s) = n.as_ref() else { panic!("no appearance") };
        let text = String::from_utf8_lossy(&s.decoded().unwrap()).into_owned();
        assert!(text.contains("/Ph Do") && text.contains("(Rack A) Tj"), "{text}");
        // The photo is 96 × 72 on the left; the text starts after it.
        let tb = [300.0 - (PAD + 96.0 + GAP + TEXT_W + PAD) / 2.0, 0.0, 300.0 + (PAD + 96.0 + GAP + TEXT_W + PAD) / 2.0, 100.0];
        let (photo, text_x, _) = layout(&d, tb).unwrap();
        assert!((photo[0] - (tb[0] + PAD)).abs() < 1e-6 && text_x > photo[2]);
        set_side(&mut doc, 0, i, TextSide::Left, &Meta::default()).unwrap();
        let d = doc.resolve(&crate::annots(&doc, p)[i]).as_dict().unwrap().clone();
        let (photo, text_x, _) = layout(&d, tb).unwrap();
        assert!(photo[0] > text_x, "the description moved to the left");
        assert!(add(&mut doc, &NewPhotoCallout { pixels: (0, 0), ..new.clone() }, &Meta::default()).is_err());
        assert!(add(&mut doc, &NewPhotoCallout { photo_height: f64::NAN, ..new }, &Meta::default()).is_err());
    }

    #[test]
    fn moving_a_photo_callout_keeps_its_dot_on_the_drawing() {
        let mut doc = blank();
        let new = NewPhotoCallout {
            page: 0,
            point: [100.0, 100.0],
            center: [300.0, 250.0],
            jpeg: vec![0xFF, 0xD8, 0xFF, 0xD9],
            pixels: (400, 300),
            photo_height: 72.0,
            text: "Rack A".into(),
            side: TextSide::Right,
            color: [0.18, 0.44, 0.69],
            author: "Steve".into(),
        };
        let i = add(&mut doc, &new, &Meta::default()).unwrap();
        crate::move_annotation(&mut doc, 0, i, 60.0, -40.0, &Meta::default()).unwrap();
        let p = crate::page_refs(&doc).unwrap()[0];
        let d = doc.resolve(&crate::annots(&doc, p)[i]).as_dict().unwrap().clone();
        let cl: Vec<f64> = d.get(b"CL").unwrap().as_array().unwrap().iter().map(|o| o.as_f64().unwrap()).collect();
        assert_eq!(&cl[..2], &[100.0, 100.0], "the dot stayed");
        // The box moved, and the leader meets it at its nearest point.
        let rect: Vec<f64> = d.get(b"Rect").unwrap().as_array().unwrap().iter().map(|o| o.as_f64().unwrap()).collect();
        let rd: Vec<f64> = d.get(b"RD").unwrap().as_array().unwrap().iter().map(|o| o.as_f64().unwrap()).collect();
        let tb = [rect[0] + rd[0], rect[1] + rd[1], rect[2] - rd[2], rect[3] - rd[3]];
        let w = PAD + 96.0 + GAP + TEXT_W + PAD;
        assert!(tb[0] > 300.0 - w / 2.0 + 30.0, "the box moved right: {tb:?}");
        assert_eq!([cl[2], cl[3]], nearest_on_box(tb, [100.0, 100.0]));
        assert!(rect[0] <= 100.0 && rect[1] <= 100.0, "the rectangle still holds the dot");
    }
}
