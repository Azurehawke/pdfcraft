//! RevuCraft's Format copy, as Illustrator's eyedropper: click a markup, a drafted shape or a
//! line of the PDF (or of a placed drawing) and its look becomes the current style of every
//! markup and drafting tool. With markups selected first, the click restyles them to match
//! instead, in one undo. Shift-click also copies the layer.

use pdfcraft_engine::stroke_align::StrokeAlign;
use pdfcraft_engine::{Document, Edit, LineEnding};

use crate::comments::{self, CommentPrefs};
use crate::draft::{DraftSettings, LineWidth};

type Rgb = [f64; 3];

/// A look picked up: what the source has (a PDF line has only a width and a colour).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Look {
    pub color: Option<Rgb>,
    /// The line width in points; `Some(0.0)` is a hairline (drafting only: markups keep theirs).
    pub width: Option<f64>,
    pub opacity: Option<f64>,
    /// `Some(None)`: the source has no fill.
    pub fill: Option<Option<Rgb>>,
    pub endings: Option<Vec<LineEnding>>,
    pub align: Option<StrokeAlign>,
    pub font_size: Option<f64>,
    /// The source's layer (`Some(None)`: on no layer).
    pub layer: Option<Option<(u32, u16)>>,
}

/// The look of the markup at (`page`, `index`).
pub(crate) fn from_markup(doc: &Document, page: usize, index: usize) -> Option<Look> {
    let p = doc.comment_props(page, index)?;
    let layer = doc.info.annotations.iter().find(|a| a.page == page && a.index == index).map(|a| a.layer);
    Some(Look {
        color: p.color,
        width: p.width.filter(|w| w.is_finite() && *w >= 0.0),
        opacity: Some(p.opacity).filter(|o| o.is_finite()),
        fill: p.fillable.then_some(p.fill),
        endings: p.endings,
        align: doc.stroke_align(page, index),
        font_size: p.font_size,
        layer,
    })
}

/// The look of a line of the PDF: its width and colour.
pub(crate) fn from_line(width: f64, color: Rgb) -> Look {
    Look { color: Some(color), width: Some(width).filter(|w| w.is_finite() && *w >= 0.0), ..Look::default() }
}

/// Edits restyling the markups `targets` to `look` (`with_layer`: onto its layer too); `None`
/// when nothing would change.
pub(crate) fn restyle(doc: &Document, targets: &[(usize, usize)], look: &Look, with_layer: bool) -> Option<Edit> {
    let mut edits = Vec::new();
    for &(page, index) in targets {
        let Some(p) = doc.comment_props(page, index) else { continue };
        if p.locked {
            continue;
        }
        if p.restylable {
            // Endings only between markups with as many ends (a line's two, a callout's one).
            let endings = look.endings.clone().filter(|e| p.endings.as_ref().is_some_and(|own| own.len() == e.len()));
            // A hairline goes only to lines and shapes made of lines; others keep their width.
            let hairline = look.width == Some(0.0);
            let lines = matches!(p.subtype.as_str(), "Line" | "PolyLine" | "Polygon");
            let width = look.width.filter(|_| p.width.is_some() && !hairline);
            if look.color.is_some() || look.opacity.is_some() || width.is_some() || endings.is_some() {
                edits.push(Edit::StyleAnnotation { page, index, color: look.color, opacity: look.opacity, width, endings });
            }
            if hairline && lines && p.width != Some(0.0) {
                edits.push(Edit::SetLineWidth { page, index, width: 0.0, real_inches: None });
            }
            if p.fillable
                && let Some(fill) = look.fill
                && fill != p.fill
            {
                edits.push(Edit::FillAnnotation { page, index, fill });
            }
            if let Some(size) = look.font_size.filter(|_| p.font_size.is_some())
                && p.font_size != Some(size)
            {
                edits.push(Edit::FontSizeAnnotation { page, index, size });
            }
        }
        if let Some(align) = look.align
            && doc.stroke_align(page, index).is_some_and(|own| own != align)
        {
            edits.push(Edit::SetStrokeAlign { page, index, align });
        }
        if with_layer && let Some(layer) = look.layer {
            edits.push(Edit::SetAnnotationLayer { page, index, layer });
        }
    }
    match edits.len() {
        0 => None,
        1 => edits.pop(),
        _ => Some(Edit::Batch { label: "Format copy".into(), edits }),
    }
}

/// Make `look` the current style: every markup tool's colour, width, opacity and fill, and the
/// drafting tools' colour, width and stroke alignment.
pub(crate) fn load(prefs: &mut CommentPrefs, draft: &mut DraftSettings, look: &Look, layer: Option<(u32, u16)>) {
    let color = look.color.filter(|c| c.iter().all(|v| v.is_finite())).map(|c| c.map(|v| v.clamp(0.0, 1.0)));
    for tool in comments::ALL {
        if let Some(c) = color {
            prefs.set_color(tool, c);
        }
        // Markup tools keep a real width: a hairline gives them only its colour.
        if let Some(w) = look.width.filter(|w| *w > 0.0) {
            prefs.set_width(tool, w);
        }
        if let Some(o) = look.opacity {
            prefs.set_opacity(tool, o);
        }
        if let Some(fill) = look.fill
            && tool.has_fill()
        {
            prefs.set_fill(tool, fill);
        }
    }
    if let Some(c) = color {
        draft.color = Some(c);
        draft.color_layer = layer;
    }
    if let Some(w) = look.width {
        draft.line_width = if w == 0.0 { LineWidth::Hairline } else { LineWidth::Paper(w.clamp(0.1, 72.0)) };
        draft.by_layer = false;
    }
    if let Some(a) = look.align {
        draft.stroke_align = a;
    }
}

/// A Format copy click: the sheet, the markup there, the point, the tolerance and Shift.
pub(crate) type Click = (usize, Option<usize>, [f64; 2], f64, bool);

/// A Format copy click on sheet `page`: the markup `source` there, else the line of the PDF at
/// `at` (within `tolerance`). With markups selected they take its look; otherwise it becomes the
/// current style. `shift`: its layer too.
pub(crate) fn pick(app: &mut crate::PdfCraftApp, index: usize, page: usize, source: Option<usize>, at: [f64; 2], tolerance: f64, shift: bool) {
    let Some(view) = app.views.get(index) else { return };
    let id = view.id;
    let Some(doc) = app.session.get(id) else { return };
    let look = match source {
        Some(i) => from_markup(doc, page, i),
        None => doc.measurement_paths(page).ok().and_then(|g| g.stroke_at(at, tolerance)).map(|(s, _)| from_line(s.width, s.color)),
    };
    let Some(look) = look else {
        app.notify(tl!("Nothing to copy there: click a markup or a line"));
        return;
    };
    let targets: Vec<(usize, usize)> = view.comments.group().into_iter().filter(|t| source.is_none_or(|i| *t != (page, i))).collect();
    if !targets.is_empty() {
        let edit = restyle(doc, &targets, &look, shift);
        match edit {
            Some(edit) => {
                app.apply_edit(edit);
            }
            None => app.notify(tl!("The selected markups already look like that")),
        }
        return;
    }
    if shift && let Some(layer) = look.layer {
        let _ = app.session.set_current_layer(id, layer);
    }
    // The colour holds on the layer current now; another layer brings back its own colour.
    let current = app.session.get(id).and_then(|d| d.current_layer);
    load(&mut app.comment_prefs, &mut app.draft, &look, current);
    app.quick_tool = crate::QuickTool::Select;
    app.notify(copied_text(&look));
}

/// What a pick copied, in words.
fn copied_text(look: &Look) -> String {
    match look.width {
        Some(w) if w == 0.0 => tl!("Copied a hairline: drafting lines are hairlines now; markups take its colour and keep their width").to_string(),
        Some(w) => crate::i18n::fmt(
            tl!("Copied its look ({w} pt): new markups and drafted lines take it"),
            &[("w", &format!("{}", (w * 100.0).round() / 100.0))],
        ),
        None => tl!("Copied its colour: new markups and drafted lines take it").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_look_becomes_every_tools_style() {
        let mut prefs = CommentPrefs::default();
        let mut draft = DraftSettings::default();
        let look = Look { color: Some([1.0, 0.0, 0.0]), width: Some(2.5), opacity: Some(0.5), fill: Some(None), ..Look::default() };
        load(&mut prefs, &mut draft, &look, Some((7, 0)));
        let s = prefs.style(comments::CommentTool::Rectangle);
        assert_eq!((s.color, s.width, s.opacity, s.fill), ([1.0, 0.0, 0.0], 2.5, 0.5, None));
        assert_eq!((draft.color, draft.color_layer), (Some([1.0, 0.0, 0.0]), Some((7, 0))));
        assert_eq!(draft.line_width, LineWidth::Paper(2.5));
        assert!(!draft.by_layer, "a picked width wins over the layer's");
        // A PDF hairline: drafting becomes hairlines; markups take only its colour.
        let line = from_line(0.0, [0.0, 0.0, 1.0]);
        assert_eq!((line.width, line.color, line.fill), (Some(0.0), Some([0.0, 0.0, 1.0]), None));
        let mut draft = DraftSettings::default();
        load(&mut prefs, &mut draft, &line, None);
        assert_eq!(draft.line_width, LineWidth::Hairline);
        let s = prefs.style(comments::CommentTool::Rectangle);
        assert_eq!((s.color, s.width), ([0.0, 0.0, 1.0], 2.5), "markups keep their width");
        assert!(copied_text(&line).contains("hairline"));
        // Garbage colours don't panic.
        load(&mut prefs, &mut draft, &Look { color: Some([f64::NAN, 2.0, -1.0]), ..Look::default() }, None);
    }
}
