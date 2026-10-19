//! RevuCraft: DWG and DXF drawings opened as PDF sheets, through CADCraft's reader and plotter
//! (Azurehawke/cadcraft, MIT OR Apache-2.0; DWG through the `acadrust` crate, MPL-2.0, used
//! unmodified as a dependency).
//!
//! Each layout (a title-block sheet) becomes a page, plotted at 1:1 on its paper, in tab order;
//! a drawing without layouts plots model space fitted to a sheet. CAD layers become PDF layers.
//! Every viewport on a sheet brings its scale with it, as a measurement viewport over its
//! rectangle, so the sheet can be measured without calibrating.

use cadcraft_doc::{Drawing, EntityKind, Space};

use crate::CreateError;

/// File extensions opened this way.
pub const EXTENSIONS: [&str; 2] = ["dwg", "dxf"];
/// The largest drawing file read.
const MAX_BYTES: usize = 512 << 20;
/// PDF points per millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// Measurement viewports taken from one sheet at most.
const MAX_VIEWPORTS: usize = 64;

/// A drawing viewport's scale on a page of the plotted PDF.
#[derive(Clone, Debug, PartialEq)]
pub struct CadViewport {
    pub page: usize,
    /// The viewport's rectangle in PDF user space (points; x0, y0, x1, y1).
    pub bbox: [f64; 4],
    pub name: String,
    /// Real length per PDF point, in `unit`.
    pub real_per_point: f64,
    /// `"ft"` (written in feet and inches) or `"mm"`.
    pub unit: &'static str,
    /// The scale as drawings write it (`1/8" = 1'-0"`, `1:100`).
    pub ratio: String,
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

/// The scale as drawings write it: an architectural scale (`1/4" = 1'-0"`), an engineering one
/// (`1" = 20'`), or a ratio (`1:100`) for metric drawings.
pub fn ratio_label(real_per_point: f64, unit: &str) -> String {
    let close = |v: f64| v >= 0.5 && ((v - v.round()) / v).abs() < 1e-4;
    if unit == "ft" {
        // Inches on paper for one real foot.
        let per_foot = 1.0 / (real_per_point * 72.0);
        if per_foot.is_finite() {
            for den in [1u64, 2, 4, 8, 16, 32, 64, 128] {
                let num = per_foot * den as f64;
                if close(num) && num < 1e6 {
                    let n = num.round() as u64;
                    let g = gcd(n, den);
                    let (n, d) = (n / g, den / g);
                    let paper = match (n / d, n % d) {
                        (whole, 0) => format!("{whole}"),
                        (0, rest) => format!("{rest}/{d}"),
                        (whole, rest) => format!("{whole} {rest}/{d}"),
                    };
                    return format!("{paper}\" = 1'-0\"");
                }
            }
        }
        // Feet for one inch on paper.
        let feet = real_per_point * 72.0;
        return if close(feet) { format!("1\" = {}'", feet.round()) } else { format!("1\" = {feet:.2}'") };
    }
    let n = real_per_point * PT_PER_MM;
    if close(n) { format!("1:{}", n.round()) } else { format!("1:{n:.2}") }
}

/// A drawing plotted to PDF, with the scales of its viewports.
#[derive(Clone, Debug)]
pub struct CadSheets {
    pub pdf: Vec<u8>,
    pub viewports: Vec<CadViewport>,
    /// Each page's layout name (`"Model"` for model space), in page order.
    pub layouts: Vec<String>,
    /// What reading the drawing could not keep (from the DWG conversion).
    pub notes: Vec<String>,
}

/// Whether `name` or `bytes` look like a DWG or DXF drawing.
pub fn is_cad(name: &str, bytes: &[u8]) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    EXTENSIONS.contains(&ext.as_str()) || bytes.starts_with(b"AC10")
}

/// Inches in one drawing unit (`$INSUNITS`; unitless drawings follow `$MEASUREMENT`).
fn unit_inches(d: &Drawing) -> f64 {
    match d.header.i64("INSUNITS", 0) {
        1 => 1.0,
        2 => 12.0,
        3 => 63_360.0,
        4 => 1.0 / 25.4,
        5 => 10.0 / 25.4,
        6 => 1000.0 / 25.4,
        7 => 1_000_000.0 / 25.4,
        10 => 36.0,
        _ if d.header.i64("MEASUREMENT", 0) == 1 => 1.0 / 25.4,
        _ => 1.0,
    }
}

/// Plot the drawing in `bytes` (named `name`) to PDF.
pub fn to_pdf(bytes: &[u8], name: &str) -> Result<CadSheets, CreateError> {
    to_pdf_layout(bytes, name, None)
}

/// Plot one layout of the drawing (`"Model"`: model space), or, with `None`, its sheets: the
/// layouts that have something on them, else model space.
pub fn to_pdf_layout(bytes: &[u8], name: &str, only: Option<&str>) -> Result<CadSheets, CreateError> {
    if bytes.len() > MAX_BYTES {
        return Err(CreateError::Invalid(format!("{name}: the drawing is larger than 512 MB")));
    }
    let d = cadcraft_io::read(bytes, name).map_err(|e| CreateError::Invalid(format!("{name}: {e}")))?;
    // Title-block sheets first: the layouts that have something on them, in tab order.
    let mut layouts: Vec<&cadcraft_doc::Layout> =
        d.layouts.iter().filter(|l| !l.entities.is_empty() && !l.name.eq_ignore_ascii_case("model")).collect();
    layouts.sort_by_key(|l| l.tab_order);
    layouts.truncate(cadcraft_io::pdf::MAX_PAGES);
    let spaces: Vec<Space> = match only {
        Some(m) if m.eq_ignore_ascii_case("model") => vec![Space::Model],
        Some(l) => match d.layouts.iter().find(|x| x.name == l) {
            Some(x) => vec![Space::Paper(x.name.clone())],
            None => return Err(CreateError::Invalid(format!("{name}: the drawing has no layout named {l}"))),
        },
        None if layouts.is_empty() => vec![Space::Model],
        None => layouts.iter().map(|l| Space::Paper(l.name.clone())).collect(),
    };
    let layout_names: Vec<String> = spaces
        .iter()
        .map(|s| match s {
            Space::Paper(n) => n.clone(),
            Space::Model => "Model".to_string(),
        })
        .collect();
    let title = name.rsplit_once('.').map_or(name, |(s, _)| s).to_string();
    let opts = cadcraft_io::PdfOptions { compress: true, title, ..cadcraft_io::PdfOptions::default() };
    let pdf = cadcraft_io::pdf::pdf_sheets(&d, &spaces, &opts).map_err(|e| CreateError::Invalid(format!("{name}: {e}")))?;

    // Points per paper unit, and how real lengths are written.
    let unit_mm = cadcraft_render::paper::paper_unit_mm(&d);
    let k = unit_mm * PT_PER_MM;
    let inches = unit_inches(&d);
    let metric = d.header.i64("MEASUREMENT", 0) == 1;
    let (unit, per_inch) = if metric { ("mm", 25.4) } else { ("ft", 1.0 / 12.0) };
    // A length of `model` drawing units per point, in the unit written.
    let real = |model_per_point: f64| {
        let v = model_per_point * inches * per_inch;
        (v.is_finite() && v > 0.0).then_some(v)
    };
    let mut viewports = Vec::new();
    for (page, space) in spaces.iter().enumerate() {
        match space {
            Space::Paper(_) => {
                let Some(store) = d.space(space) else { continue };
                let mut n = 0usize;
                for e in store.iter() {
                    let EntityKind::Viewport(vp) = &e.kind else { continue };
                    // The first viewport is the sheet itself; viewports that are off show no drawing.
                    if vp.id == 1 || !vp.on || n >= MAX_VIEWPORTS {
                        continue;
                    }
                    if !(vp.width.is_finite() && vp.height.is_finite() && vp.view_height.is_finite())
                        || vp.width <= 0.0
                        || vp.height <= 0.0
                        || vp.view_height <= 0.0
                    {
                        continue;
                    }
                    // Model units per paper unit, then per point.
                    let Some(real_per_point) = real(vp.view_height / vp.height / k) else { continue };
                    let (cx, cy) = (vp.center.x, vp.center.y);
                    let bbox = [(cx - vp.width / 2.0) * k, (cy - vp.height / 2.0) * k, (cx + vp.width / 2.0) * k, (cy + vp.height / 2.0) * k];
                    if bbox.iter().all(|v| v.is_finite()) && bbox[2] > bbox[0] && bbox[3] > bbox[1] {
                        n += 1;
                        let ratio = ratio_label(real_per_point, unit);
                        viewports.push(CadViewport { page, bbox, name: format!("Viewport {n}"), real_per_point, unit, ratio });
                    }
                }
            }
            Space::Model => {
                // Model space is fitted to the printable area of its sheet, as the plotter does.
                let Ok(setup) = cadcraft_io::pdf::page_for(&d, space, &opts) else { continue };
                let sheet = cadcraft_render::Sheet::from_page(&setup, unit_mm);
                let b = d.extents(space);
                // A drawing one line thick (or tall) fits by its other side, as the plotter does.
                if b.is_empty() || (b.width() <= 0.0 && b.height() <= 0.0) {
                    continue;
                }
                let s = (sheet.printable.width() / b.width().max(1e-12)).min(sheet.printable.height() / b.height().max(1e-12));
                let Some(real_per_point) = real(1.0 / (s * k)) else { continue };
                let size = sheet.size * k;
                let ratio = ratio_label(real_per_point, unit);
                viewports.push(CadViewport { page, bbox: [0.0, 0.0, size.x, size.y], name: "Model".into(), real_per_point, unit, ratio });
            }
        }
    }
    Ok(CadSheets { pdf, viewports, layouts: layout_names, notes: d.read_notes.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal ASCII DXF: model space with one line, inch units.
    const LINE_DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n120.0\n21\n0.0\n31\n0.0\n0\nENDSEC\n0\nEOF\n";

    #[test]
    fn a_dxf_plots_to_a_pdf_with_its_scale() {
        assert!(is_cad("Plan.DWG", b"") && is_cad("x", b"AC1032...") && !is_cad("plan.pdf", b"%PDF-1.7"));
        let sheets = to_pdf(LINE_DXF.as_bytes(), "plan.dxf").expect("the DXF plots");
        assert!(sheets.pdf.starts_with(b"%PDF-"), "a PDF comes out");
        let vp = sheets.viewports.first().expect("model space brings its fitted scale");
        assert_eq!((vp.page, vp.unit), (0, "ft"));
        assert!(vp.real_per_point > 0.0 && vp.real_per_point.is_finite());
    }

    #[test]
    fn scales_read_as_drawings_write_them() {
        // 1/8" = 1'-0": one point is 96/72 inches, a ninth of a foot... in feet per point.
        assert_eq!(ratio_label(96.0 / 72.0 / 12.0, "ft"), "1/8\" = 1'-0\"");
        assert_eq!(ratio_label(48.0 / 72.0 / 12.0, "ft"), "1/4\" = 1'-0\"");
        assert_eq!(ratio_label(8.0 / 72.0 / 12.0, "ft"), "1 1/2\" = 1'-0\"");
        assert_eq!(ratio_label(20.0 / 72.0, "ft"), "1\" = 20'");
        assert_eq!(ratio_label(100.0 / PT_PER_MM, "mm"), "1:100");
    }

    #[test]
    fn hostile_drawings_are_errors_not_crashes() {
        for bytes in [&b""[..], b"0\nSECTION\n", b"AC1032\0\0\0\0garbage", &[0xFF; 64]] {
            let _ = to_pdf(bytes, "bad.dxf");
            let _ = to_pdf(bytes, "bad.dwg");
        }
    }
}
