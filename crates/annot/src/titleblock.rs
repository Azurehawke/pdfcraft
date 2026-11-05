//! The title block: a corner of a sheet with what it is (project, client, address, author, format,
//! scale, date and the page's number out of the document's). It is drawn as ordinary drafted
//! comments on a layer of its own, moved to the back of the page's list so that every other mark
//! draws over it. The layer can be hidden at will. A page has one.

use pdfcraft_cos::{Document, ObjRef, Object};

use super::layers;
use super::{AnnotError, LineEnding, NewAnnotation, Shape, Style, annots, set_annots};

/// The key a title block's comments carry: `/PCTitleBlock true`.
const MARK: &[u8] = b"PCTitleBlock";

/// The role of the title block's layer.
pub const ROLE: &str = "titleblock";

/// The block's band from the page edges, in points (half an inch).
const MARGIN: f64 = 36.0;
/// Its size in points: five and a half by one and three quarter inches, at most the page less
/// the margins.
const WIDTH: f64 = 396.0;
const HEIGHT: f64 = 126.0;
/// The share of the width the left column takes.
const LEFT: f64 = 0.6;
/// Text size in points.
const FONT: f64 = 8.0;
/// Space between a cell's edge and its text, in points.
const INSET: f64 = 4.0;

/// What a title block says. Empty entries take no room.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields {
    pub project: String,
    pub client: String,
    pub address: String,
    pub author: String,
    pub scale: String,
}

/// The block's entries from the document's properties: the project is the document's Title, the
/// author its Author, and the client, address and scale are custom properties with the same names
/// (Document Properties ▸ Custom). `info` returns a property's text; blank entries stay empty.
pub fn fields_from_properties(info: impl Fn(&str) -> Option<String>) -> Fields {
    let get = |key: &str| info(key).map(|v| v.trim().to_owned()).unwrap_or_default();
    Fields { project: get("Title"), client: get("Client"), address: get("Address"), author: get("Author"), scale: get("Scale") }
}

/// The paper's name for a sheet of `width` × `height` points: a common sheet by name, else its
/// size in inches.
pub fn format_name(width: f64, height: f64) -> String {
    let (short, long) = (width.min(height) / 72.0, width.max(height) / 72.0);
    const SHEETS: [(&str, f64, f64); 5] =
        [("Letter", 8.5, 11.0), ("Legal", 8.5, 14.0), ("Tabloid", 11.0, 17.0), ("A4", 8.27, 11.69), ("A3", 11.69, 16.54)];
    SHEETS
        .iter()
        .find(|(_, w, h)| (short - w).abs() < 0.05 && (long - h).abs() < 0.05)
        .map(|(name, _, _)| name.to_string())
        .unwrap_or_else(|| format!("{short:.2} × {long:.2} in"))
}

/// The comments that draw a title block on `page` of a sheet whose media box is `media`
/// (`[x0, y0, x1, y1]`, points), for a document of `pages` pages. Nothing is written here.
pub fn layout(media: [f64; 4], page: usize, pages: usize, fields: &Fields, format: &str, date: &str) -> Result<Vec<NewAnnotation>, AnnotError> {
    let [x0, y0, x1, y1] = media;
    let width = WIDTH.min((x1 - x0) - 2.0 * MARGIN);
    let height = HEIGHT.min((y1 - y0) - 2.0 * MARGIN);
    if !(width > 100.0 && height > 40.0) {
        return Err(AnnotError::Invalid("the sheet is too small for a title block".into()));
    }
    let left = x1 - MARGIN - width;
    let bottom = y0 + MARGIN;
    let top = bottom + height;
    let split = left + width * LEFT;
    let row = height / 4.0;
    let sheet = format!("Page {} of {}", page + 1, pages);
    let cells = [
        ("Project", fields.project.as_str(), "Format", format),
        ("Client", fields.client.as_str(), "Scale", fields.scale.as_str()),
        ("Address", fields.address.as_str(), "Date", date),
        ("Author", fields.author.as_str(), "Sheet", sheet.as_str()),
    ];
    let line_style = Style { color: [0.0; 3], opacity: 1.0, width: 0.75, fill: None };
    let mut out = Vec::new();
    let mut add = |shape: Shape, style: Style, contents: String| {
        out.push(NewAnnotation { page, shape, style, contents, author: fields.author.clone() });
    };
    add(Shape::Rectangle { rect: [left, bottom, left + width, top] }, line_style.clone(), String::new());
    for i in 1..4 {
        let y = bottom + row * f64::from(i);
        add(
            Shape::Line { from: [left, y], to: [left + width, y], start: LineEnding::None, end: LineEnding::None },
            line_style.clone(),
            String::new(),
        );
    }
    add(Shape::Line { from: [split, bottom], to: [split, top], start: LineEnding::None, end: LineEnding::None }, line_style.clone(), String::new());
    let text_style = Style { color: [0.0; 3], opacity: 1.0, width: 0.0, fill: None };
    for (r, (l_label, l_value, r_label, r_value)) in cells.iter().enumerate() {
        // The top row is the first of the four.
        let cell_top = top - row * r as f64;
        let cell_bottom = cell_top - row;
        for (col, (label, value)) in [(0, (*l_label, *l_value)), (1, (*r_label, *r_value))] {
            let (cx0, cx1) = if col == 0 { (left, split) } else { (split, left + width) };
            let rect = [cx0 + INSET, cell_bottom + INSET, cx1 - INSET, cell_top - INSET];
            let contents = if value.is_empty() { label.to_string() } else { format!("{label}\n{value}") };
            add(Shape::TextBox { rect, font_size: FONT }, text_style.clone(), contents);
        }
    }
    Ok(out)
}

/// The title block's layer, made the first time one is drawn.
pub fn ensure_layer(doc: &mut Document) -> Result<ObjRef, AnnotError> {
    if let Some(found) = layers::list(doc).into_iter().find(|l| l.role.as_deref() == Some(ROLE)) {
        return Ok(found.id);
    }
    let id = layers::add(doc, "Title block", [0.0; 3])?;
    layers::set_role(doc, id, Some(ROLE))?;
    Ok(id)
}

/// Whether `page` already has a title block (one of its comments carries the mark).
pub fn on_page(doc: &Document, page: usize) -> Result<bool, AnnotError> {
    let p = super::page_ref(doc, page)?;
    Ok(annots(doc, p).iter().any(|entry| {
        let dict = match entry {
            Object::Ref(r) => doc.get(*r).as_dict().cloned(),
            Object::Dict(d) => Some(d.clone()),
            _ => None,
        };
        matches!(dict.as_ref().and_then(|d| d.get(MARK)), Some(Object::Bool(true)))
    }))
}

/// Marks the comment at `index` of `page` as part of a title block.
pub fn mark(doc: &mut Document, page: usize, index: usize) -> Result<(), AnnotError> {
    let (_, r) = super::annot_ref(doc, page, index)?;
    doc.update_dict(r, |d| d.set(MARK.to_vec(), Object::Bool(true)))?;
    Ok(())
}

/// Draws the comments at `indices` of `page` before every other comment on the page, keeping
/// their order among themselves. Comments are drawn in the order of the page's list.
pub fn send_to_back(doc: &mut Document, page: usize, indices: &[usize]) -> Result<(), AnnotError> {
    let p = super::page_ref(doc, page)?;
    let list = annots(doc, p);
    let front: Vec<Object> =
        indices.iter().map(|i| list.get(*i).cloned().ok_or(AnnotError::NoSuchAnnotation { page, index: *i })).collect::<Result<_, _>>()?;
    let rest = list.iter().enumerate().filter(|(i, _)| !indices.contains(i)).map(|(_, o)| o.clone());
    let reordered: Vec<Object> = front.into_iter().chain(rest).collect();
    set_annots(doc, p, reordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Fields {
        Fields {
            project: "Harbour House".into(),
            client: "A. Client".into(),
            address: "1 Dock Rd".into(),
            author: "S. Architect".into(),
            scale: "1:50".into(),
        }
    }

    #[test]
    fn the_block_sits_in_the_bottom_right_corner_inside_the_margins() {
        let media = [0.0, 0.0, 612.0, 792.0];
        let pieces = layout(media, 1, 2, &fields(), "Letter", "2026-11-04").expect("lays out");
        for p in &pieces {
            let rect = match &p.shape {
                Shape::Rectangle { rect } | Shape::TextBox { rect, .. } => *rect,
                Shape::Line { from, to, .. } => [from[0], from[1], to[0], to[1]],
                other => panic!("unexpected piece {other:?}"),
            };
            assert!(rect[0] >= 612.0 - MARGIN - WIDTH - 0.01 && rect[2] <= 612.0 - MARGIN + 0.01, "x out of the corner: {rect:?}");
            assert!(rect[1] >= MARGIN - 0.01 && rect[3] <= MARGIN + HEIGHT + 0.01, "y out of the corner: {rect:?}");
        }
    }

    #[test]
    fn the_sheet_number_counts_the_document() {
        let pieces = layout([0.0, 0.0, 612.0, 792.0], 1, 3, &fields(), "Letter", "2026-11-04").expect("lays out");
        assert!(
            pieces.iter().any(|p| p.contents.contains("Page 2 of 3")),
            "no page 2 of 3 in {:?}",
            pieces.iter().map(|p| &p.contents).collect::<Vec<_>>()
        );
    }

    #[test]
    fn empty_entries_take_no_text() {
        let pieces = layout([0.0, 0.0, 612.0, 792.0], 0, 1, &Fields::default(), "Letter", "2026-11-04").expect("lays out");
        assert!(pieces.iter().any(|p| p.contents == "Client"), "an empty client should show only its label");
        assert!(!pieces.iter().any(|p| p.contents.contains("Client\n")), "an empty client should have no value line");
    }

    #[test]
    fn a_small_sheet_is_refused() {
        assert!(layout([0.0, 0.0, 120.0, 120.0], 0, 1, &fields(), "Letter", "x").is_err());
    }

    #[test]
    fn format_names_common_sheets() {
        assert_eq!(format_name(612.0, 792.0), "Letter");
        assert_eq!(format_name(792.0, 612.0), "Letter");
        assert_eq!(format_name(300.0, 400.0), "4.17 × 5.56 in");
    }

    #[test]
    fn the_fields_come_from_the_properties_and_blanks_stay_empty() {
        let props = |key: &str| match key {
            "Title" => Some("  Harbour House ".to_owned()),
            "Client" => Some("Lee Family".to_owned()),
            "Scale" => Some("1:50".to_owned()),
            _ => None,
        };
        let f = fields_from_properties(props);
        assert_eq!(f.project, "Harbour House");
        assert_eq!(f.client, "Lee Family");
        assert_eq!(f.scale, "1:50");
        assert_eq!(f.address, "");
        assert_eq!(f.author, "");
    }
}
