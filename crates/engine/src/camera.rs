//! Scan & OCR ▸ Enhance ▸ Camera image: straighten a photographed document. Four corners on the
//! page (PDF user space) are warped to a rectangle, and the page's content becomes that picture
//! (one undoable step, through Replace Pages, so the page's comments, links and bookmarks stay).

use std::sync::Arc;

use pdfcraft_geom::perspective::{Homography, Point, order_quad};
use pdfcraft_render::{RenderConfig, RenderRequest};

use crate::{DocId, Edit, EditError, ImageResolution, Session};

/// The densest the page is read at: 600 dpi, as for OCR.
const MAX_SCALE: f32 = 600.0 / 72.0;
/// The resolution a page without a picture is read at.
const DEFAULT_SCALE: f32 = 300.0 / 72.0;
/// The most pixels the page is rendered with, and the straightened picture has.
const MAX_PIXELS: f64 = 40_000_000.0;
/// JPEG quality of the straightened picture.
const QUALITY: u8 = 90;

/// A straightened picture: RGB8, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Warped {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Warp the quadrilateral `quad` (raster pixels, any corner order) of a premultiplied RGBA
/// raster to an upright rectangle as large as the quadrilateral's longer sides, composited over
/// white. Errors say what was wrong with the corners or the raster.
pub fn warp(rgba: &[u8], width: u32, height: u32, quad: [Point; 4]) -> Result<Warped, String> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || w.checked_mul(h).and_then(|n| n.checked_mul(4)) != Some(rgba.len()) {
        return Err("the page could not be read as a picture".into());
    }
    let [tl, tr, br, bl] = order_quad(quad).ok_or("the four corners must enclose an area (a convex quadrilateral)")?;
    let dist = |a: Point, b: Point| (a[0] - b[0]).hypot(a[1] - b[1]);
    let mut ow = dist(tl, tr).max(dist(bl, br));
    let mut oh = dist(tl, bl).max(dist(tr, br));
    let k = (MAX_PIXELS / (ow * oh).max(1.0)).sqrt().min(1.0);
    ow = (ow * k).floor().max(1.0);
    oh = (oh * k).floor().max(1.0);
    if !(ow.is_finite() && oh.is_finite()) {
        return Err("the corners are out of range".into());
    }
    // Both are at least 1 and their product is at most MAX_PIXELS, so the casts are exact.
    let (ow, oh) = (ow as u32, oh as u32);
    let rect = [[0.0, 0.0], [ow as f64, 0.0], [ow as f64, oh as f64], [0.0, oh as f64]];
    let to_src = Homography::from_points(rect, [tl, tr, br, bl]).ok_or("the four corners must not be in a line")?;

    let px = |x: usize, y: usize| -> [f64; 4] {
        let i = (y * w + x) * 4;
        match rgba.get(i..i + 4) {
            Some(p) => [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64],
            None => [0.0; 4],
        }
    };
    let mut rgb = Vec::with_capacity(ow as usize * oh as usize * 3);
    for y in 0..oh {
        for x in 0..ow {
            // Sample at the pixel's centre; outside the page is white.
            let mut c = [0.0f64; 4];
            if let Some([sx, sy]) = to_src.apply([x as f64 + 0.5, y as f64 + 0.5]) {
                let (fx, fy) = (sx - 0.5, sy - 0.5);
                if fx > -1.0 && fy > -1.0 && fx < w as f64 && fy < h as f64 {
                    let (x0, y0) = (fx.floor(), fy.floor());
                    let (ax, ay) = (fx - x0, fy - y0);
                    // Clamped to the raster, so every index below is in range.
                    let clamp = |v: f64, n: usize| v.clamp(0.0, (n - 1) as f64) as usize;
                    let (xa, xb) = (clamp(x0, w), clamp(x0 + 1.0, w));
                    let (ya, yb) = (clamp(y0, h), clamp(y0 + 1.0, h));
                    let (p00, p10, p01, p11) = (px(xa, ya), px(xb, ya), px(xa, yb), px(xb, yb));
                    for (i, v) in c.iter_mut().enumerate() {
                        let top = p00[i] * (1.0 - ax) + p10[i] * ax;
                        let bottom = p01[i] * (1.0 - ax) + p11[i] * ax;
                        *v = top * (1.0 - ay) + bottom * ay;
                    }
                }
            }
            // Premultiplied over white: colour + (1 - alpha) × white.
            for v in &c[..3] {
                rgb.push((v + 255.0 - c[3]).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    Ok(Warped { width: ow, height: oh, rgb })
}

/// The long side, in pixels, a page is rendered at to look for the document's edges.
const DETECT_SIDE: f32 = 1200.0;

/// What straightening one page needs, without the session: a page of [`CameraBatch`], or the
/// one [`Session::enhance_camera_page_at`] enhances.
#[derive(Clone, Debug)]
struct PageSource {
    page: usize,
    info: pdfcraft_render::PageInfo,
    /// The page's densest picture, in pixels per point, when it has one.
    native: Option<f32>,
}

/// A page straightened and cleaned up, ready to replace the page it came from
/// ([`Session::finish_camera_batch`]).
#[derive(Clone, Debug)]
pub struct CameraPage {
    pub page: usize,
    name: &'static str,
    encoded: Vec<u8>,
    dpi: f64,
    /// Whether the document's edges were found; when not, the whole page was cleaned up as it is.
    pub detected: bool,
}

/// Enhance ▸ Camera image ▸ Enhance all pages: every page of a document straightened at the
/// edges found on it (the whole page where none are) and cleaned up alike. The pixel work
/// ([`CameraBatch::page`]) needs no session, so it can run on a worker thread a page at a time;
/// [`Session::finish_camera_batch`] then replaces the pages in one undoable step.
#[derive(Clone, Debug)]
pub struct CameraBatch {
    pub id: DocId,
    bytes: Arc<Vec<u8>>,
    password: Option<Arc<str>>,
    pages: Vec<PageSource>,
    look: crate::camera_clean::Look,
    level: u8,
}

impl CameraBatch {
    /// How many pages it enhances.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Straighten and clean up the `index`th page (0-based among [`Self::len`]).
    pub fn page(&self, index: usize) -> Result<CameraPage, String> {
        let src = self.pages.get(index).ok_or("no such page")?;
        let corners = detect(&self.bytes, self.password.clone(), src).map(|q| (q, true)).unwrap_or_else(|| (whole_page(&src.info), false));
        let mut out = straighten(&self.bytes, self.password.clone(), src, corners.0, self.look, self.level)?;
        out.detected = corners.1;
        Ok(out)
    }
}

/// The page's own corners, in user space: what a page without a detected document keeps.
fn whole_page(info: &pdfcraft_render::PageInfo) -> [Point; 4] {
    [[0.0, 0.0], [info.width, 0.0], [info.width, info.height], [0.0, info.height]].map(|[x, y]| {
        let [u, v] = info.view_to_user(x, y);
        [u as f64, v as f64]
    })
}

/// The photographed document's corners on a page, in user space (see
/// [`Session::detect_camera_corners`]).
fn detect(bytes: &Arc<Vec<u8>>, password: Option<Arc<str>>, src: &PageSource) -> Option<[Point; 4]> {
    let info = &src.info;
    let scale = (DETECT_SIDE / info.width.max(info.height).max(1.0)).clamp(0.05, MAX_SCALE);
    let config = RenderConfig { password, ..Default::default() };
    let shot = pdfcraft_render::PageRenderer::new(bytes.clone(), config).render(RenderRequest { page: src.page, scale, ..Default::default() });
    if shot.error.is_some() {
        return None;
    }
    let quad = crate::camera_edges::detect_document(&shot.rgba, shot.width, shot.height)?;
    let k = shot.width as f64 / (info.width as f64).max(1e-3);
    Some(quad.map(|[x, y]| {
        let [u, v] = info.view_to_user((x / k) as f32, (y / k) as f32);
        [u as f64, v as f64]
    }))
}

/// Render a page, warp what `corners` enclose upright, clean it up and encode it.
fn straighten(
    bytes: &Arc<Vec<u8>>,
    password: Option<Arc<str>>,
    src: &PageSource,
    corners: [Point; 4],
    look: crate::camera_clean::Look,
    level: u8,
) -> Result<CameraPage, String> {
    let (page, info) = (src.page, &src.info);
    let area = (info.width.max(1.0) as f64) * (info.height.max(1.0) as f64);
    let cap = (MAX_PIXELS / area).sqrt() as f32;
    let scale = src.native.unwrap_or(DEFAULT_SCALE).clamp(1.0, MAX_SCALE).min(cap).max(0.01);
    let config = RenderConfig { password, ..Default::default() };
    let mut r = pdfcraft_render::PageRenderer::new(bytes.clone(), config);
    let shot = r.render(RenderRequest { page, scale, ..Default::default() });
    if let Some(e) = shot.error {
        return Err(format!("page {} could not be rendered: {e}", page + 1));
    }
    let k = shot.width as f64 / (info.width as f64).max(1e-3);
    let quad = corners.map(|[x, y]| {
        let [vx, vy] = info.user_to_view(x as f32, y as f32);
        [vx as f64 * k, vy as f64 * k]
    });
    let out = warp(&shot.rgba, shot.width, shot.height, quad)?;
    let pixels = crate::camera_clean::apply(&out.rgb, out.width, out.height, look, level).ok_or("the straightened page could not be cleaned up")?;
    // Black and white is lossless (PNG): JPEG would smear grey around every letter.
    let mut encoded = Vec::new();
    let jpeg = |encoded: &mut Vec<u8>, px: &[u8], color| {
        image::ImageEncoder::write_image(image::codecs::jpeg::JpegEncoder::new_with_quality(encoded, QUALITY), px, out.width, out.height, color)
    };
    let (name, written) = match &pixels {
        crate::camera_clean::Pixels::Rgb(rgb) => ("camera.jpg", jpeg(&mut encoded, rgb, image::ExtendedColorType::Rgb8)),
        crate::camera_clean::Pixels::Grey(grey) if look == crate::camera_clean::Look::BlackWhite => (
            "camera.png",
            image::ImageEncoder::write_image(
                image::codecs::png::PngEncoder::new(&mut encoded),
                grey,
                out.width,
                out.height,
                image::ExtendedColorType::L8,
            ),
        ),
        crate::camera_clean::Pixels::Grey(grey) => ("camera.jpg", jpeg(&mut encoded, grey, image::ExtendedColorType::L8)),
    };
    written.map_err(|e| format!("the straightened page could not be encoded: {e}"))?;
    // One device pixel per `k` points keeps the page's physical size.
    let dpi = (k * 72.0).clamp(1.0, 1200.0);
    Ok(CameraPage { page, name, encoded, dpi, detected: true })
}

impl Session {
    /// What straightening page `page` of `id` needs, refused for a read-only document.
    fn page_source(&self, id: DocId, page: usize) -> Result<(Arc<Vec<u8>>, Option<Arc<str>>, PageSource), EditError> {
        let doc = self.get(id).ok_or(EditError::NoDocument)?;
        if let Some(why) = &doc.read_only_reason {
            return Err(EditError::ReadOnly(why.clone()));
        }
        let info = doc.info.pages.get(page).ok_or_else(|| EditError::Invalid(format!("there is no page {}", page.saturating_add(1))))?.clone();
        let native = doc
            .page_images(page)
            .iter()
            .filter(|i| i.width > 1 && i.height > 1)
            .map(|i| {
                let m = i.matrix;
                let (w, h) = (m[0].hypot(m[1]), m[2].hypot(m[3]));
                (i.width as f64 / w.max(1e-6)).max(i.height as f64 / h.max(1e-6)) as f32
            })
            .filter(|s| s.is_finite())
            .reduce(f32::max);
        Ok((doc.bytes.clone(), doc.password.as_deref().map(Arc::from), PageSource { page, info, native }))
    }

    /// Enhance ▸ Camera image: the corners of the photographed document on page `page`
    /// (0-based) of document `id`, in PDF user space (top-left, top-right, bottom-right,
    /// bottom-left as displayed), or `None` when its edges can't be found with confidence.
    pub fn detect_camera_corners(&self, id: DocId, page: usize) -> Option<[Point; 4]> {
        let doc = self.get(id)?;
        let info = doc.info.pages.get(page)?.clone();
        detect(&doc.bytes, doc.password.as_deref().map(Arc::from), &PageSource { page, info, native: None })
    }

    /// Enhance ▸ Camera image: replace page `page` (0-based) of document `id` with the part of it
    /// inside `corners` (PDF user space, any order), straightened to a rectangle. The page is
    /// read at its picture's own resolution (at most 600 dpi) and keeps that physical size.
    pub fn enhance_camera_page(&mut self, id: DocId, page: usize, corners: [Point; 4]) -> Result<(), EditError> {
        self.enhance_camera_page_at(id, page, corners, crate::camera_clean::Look::Original, 0)
    }

    /// [`Session::enhance_camera_page`], then give the straightened photo a `look` at `level`
    /// (0 = as photographed … 100 = white paper, strong contrast, no specks; for black and white
    /// the threshold; see [`crate::camera_clean`]). Still one undoable step.
    pub fn enhance_camera_page_at(
        &mut self,
        id: DocId,
        page: usize,
        corners: [Point; 4],
        look: crate::camera_clean::Look,
        level: u8,
    ) -> Result<(), EditError> {
        let (bytes, password, src) = self.page_source(id, page)?;
        let out = straighten(&bytes, password, &src, corners, look, level).map_err(EditError::Invalid)?;
        self.finish_camera_pages(id, vec![out], "Enhance camera image")
    }

    /// Enhance ▸ Camera image ▸ Enhance all pages: the work for every page of `id`, to run with
    /// [`CameraBatch::page`] and finish with [`Session::finish_camera_batch`].
    /// `skip` is a page left as it is (one already enhanced).
    pub fn camera_batch(&self, id: DocId, look: crate::camera_clean::Look, level: u8, skip: Option<usize>) -> Result<CameraBatch, EditError> {
        let n = self.get(id).ok_or(EditError::NoDocument)?.info.pages.len();
        let mut pages = Vec::with_capacity(n);
        let mut first = None;
        for page in (0..n).filter(|p| Some(*p) != skip) {
            let (bytes, password, src) = self.page_source(id, page)?;
            first.get_or_insert((bytes, password));
            pages.push(src);
        }
        let Some((bytes, password)) = first else {
            let doc = self.get(id).ok_or(EditError::NoDocument)?;
            return Ok(CameraBatch { id, bytes: doc.bytes.clone(), password: None, pages, look, level });
        };
        Ok(CameraBatch { id, bytes, password, pages, look, level })
    }

    /// Replace each enhanced page with its picture, in one undoable step ("Enhance all pages").
    /// The document must not have changed since [`Session::camera_batch`] (the caller checks).
    pub fn finish_camera_batch(&mut self, id: DocId, pages: Vec<CameraPage>) -> Result<(), EditError> {
        self.finish_camera_pages(id, pages, "Enhance all pages")
    }

    fn finish_camera_pages(&mut self, id: DocId, pages: Vec<CameraPage>, label: &str) -> Result<(), EditError> {
        if pages.is_empty() {
            return Ok(());
        }
        let mut edits = Vec::with_capacity(pages.len());
        for p in pages {
            let bytes = self.create_from_images_with_resolution(&[(p.name.into(), p.encoded)], ImageResolution::Dpi(p.dpi))?;
            edits.push(Edit::ReplacePages { pages: vec![p.page], name: "camera.pdf".into(), bytes, src_pages: vec![0] });
        }
        self.apply(id, Edit::Batch { label: label.into(), edits })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An opaque premultiplied raster filled by `f(x, y) -> [r, g, b]`.
    fn raster(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let [r, g, b] = f(x, y);
                [r, g, b, 255]
            })
            .collect()
    }

    #[test]
    fn a_tilted_quad_comes_out_upright() {
        // A dark quadrilateral on white; warping exactly its corners leaves a dark picture.
        let quad = [[20.0, 30.0], [170.0, 10.0], [180.0, 150.0], [10.0, 160.0]];
        let inside = |x: f64, y: f64| {
            let q = order_quad(quad).unwrap();
            (0..4).all(|i| {
                let (a, b) = (q[i], q[(i + 1) % 4]);
                (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]) >= 0.0
            })
        };
        let src = raster(200, 180, |x, y| if inside(x as f64 + 0.5, y as f64 + 0.5) { [20, 20, 20] } else { [255, 255, 255] });
        let out = warp(&src, 200, 180, quad).unwrap();
        // The longer of each pair of opposite sides: 170 × 140.
        assert_eq!((out.width, out.height), (170, 140));
        // Away from the anti-aliased edges, everything is the dark fill.
        let at = |x: u32, y: u32| out.rgb[((y * out.width + x) * 3) as usize];
        for (x, y) in [(5, 5), (85, 70), (164, 134), (5, 134), (164, 5)] {
            assert!(at(x, y) < 60, "({x}, {y}) = {}", at(x, y));
        }
    }

    #[test]
    fn the_corner_order_does_not_matter() {
        let src = raster(64, 48, |x, y| [(x * 4) as u8, (y * 5) as u8, 128]);
        let q = [[4.0, 3.0], [60.0, 6.0], [58.0, 44.0], [2.0, 40.0]];
        let a = warp(&src, 64, 48, q).unwrap();
        let b = warp(&src, 64, 48, [q[2], q[0], q[3], q[1]]).unwrap();
        assert_eq!(a, b);
        // Left to right the red channel grows, top to bottom the green one does.
        let px = |x: u32, y: u32| &a.rgb[((y * a.width + x) * 3) as usize..][..3];
        assert!(px(a.width - 3, 10)[0] > px(2, 10)[0] + 100);
        assert!(px(10, a.height - 3)[1] > px(10, 2)[1] + 100);
    }

    #[test]
    fn transparency_becomes_white() {
        let src = vec![0u8; 8 * 8 * 4];
        let out = warp(&src, 8, 8, [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]]).unwrap();
        assert!(out.rgb.iter().all(|&v| v == 255));
    }

    #[test]
    fn bad_corners_and_rasters_are_errors_not_panics() {
        let src = raster(10, 10, |_, _| [0, 0, 0]);
        let line = [[0.0, 0.0], [5.0, 5.0], [10.0, 10.0], [2.0, 2.0]];
        assert!(warp(&src, 10, 10, line).is_err());
        assert!(warp(&src, 10, 10, [[f64::NAN, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]).is_err());
        let ok = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!(warp(&src, 11, 10, ok).is_err(), "size doesn't match the buffer");
        assert!(warp(&[], 0, 0, ok).is_err());
        // Corners far outside the raster: a capped, white picture, not a huge allocation.
        let far = [[-1e9, -1e9], [1e9, -1e9], [1e9, 1e9], [-1e9, 1e9]];
        let out = warp(&src, 10, 10, far).unwrap();
        assert!((out.width as f64) * (out.height as f64) <= MAX_PIXELS);
    }
}
