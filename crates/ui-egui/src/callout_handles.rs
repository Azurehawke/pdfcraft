//! RevuCraft: the two handles of a selected photo callout. One, on the photo's inner bottom
//! corner, scales the photo (its proportions kept); the other, on the text column's outer edge,
//! sets how wide the description wraps. The box's top edge and photo-side edge stay where they
//! are, and so does the dot; the change is one edit ([`Edit::SetCalloutParts`]) on letting go.

use egui::{Color32, Pos2, Rect, Sense, Stroke, vec2};
use pdfcraft_engine::photo_callout::{PHOTO_HEIGHTS, Parts, TEXT_WIDTHS, TextSide};
use pdfcraft_engine::{Document, Edit};

use crate::canvas::DocView;
use crate::comments::PageCx;

/// A handle's size on screen.
const HANDLE: f32 = 9.0;

/// The photo's corner the handle sits on, and the corner that stays put (user space).
fn photo_corners(p: &Parts) -> ([f64; 2], [f64; 2]) {
    let [x0, y0, x1, y1] = p.photo;
    match p.side {
        // Text on the right: the box's left and top edges stay, so the photo's top-left does.
        TextSide::Right => ([x1, y0], [x0, y1]),
        TextSide::Left => ([x0, y0], [x1, y1]),
    }
}

/// The middle of the text column's outer edge (user space).
fn text_edge(p: &Parts) -> [f64; 2] {
    let [x0, y0, x1, y1] = p.text;
    let x = if p.side == TextSide::Right { x1 } else { x0 };
    [x, (y0 + y1) / 2.0]
}

/// The photo's new height when its handle is dragged from `corner` to `to`, `anchor` fixed: the
/// drag measured along the photo's diagonal.
pub(crate) fn photo_height(p: &Parts, to: [f64; 2]) -> Option<f64> {
    let (corner, anchor) = photo_corners(p);
    let d = [corner[0] - anchor[0], corner[1] - anchor[1]];
    let len = d[0] * d[0] + d[1] * d[1];
    if !len.is_finite() || len < 1e-9 {
        return None;
    }
    let s = ((to[0] - anchor[0]) * d[0] + (to[1] - anchor[1]) * d[1]) / len;
    let h = (p.photo[3] - p.photo[1]) * s;
    h.is_finite().then(|| h.clamp(*PHOTO_HEIGHTS.start(), *PHOTO_HEIGHTS.end()))
}

/// The text column's new width when its edge is dragged to `to`.
pub(crate) fn text_width(p: &Parts, to: [f64; 2]) -> Option<f64> {
    let edge = text_edge(p);
    let dx = if p.side == TextSide::Right { to[0] - edge[0] } else { edge[0] - to[0] };
    let w = p.text[2] - p.text[0] + dx;
    w.is_finite().then(|| w.clamp(*TEXT_WIDTHS.start(), *TEXT_WIDTHS.end()))
}

/// Draws the handles of the selected photo callout on this page and takes their drags. Returns
/// `true` while one is being dragged (the page's other tools leave the pointer alone).
pub(crate) fn input(ui: &egui::Ui, doc: &Document, view: &mut DocView, cx: &PageCx<'_>, accent: Color32) -> bool {
    let Some((page, index)) = view.comments.selected.filter(|(p, _)| *p == cx.page) else { return false };
    if view.comments.group().len() > 1 || !cx.allowed {
        return false;
    }
    let Some(parts) = doc.callout_parts(page, index) else { return false };
    let photo_at = cx.to_screen(photo_corners(&parts).0);
    let text_at = cx.to_screen(text_edge(&parts));
    let mut busy = false;
    let painter = ui.painter().clone();
    for (photo, at) in [(true, photo_at), (false, text_at)] {
        let r = Rect::from_center_size(at, vec2(HANDLE, HANDLE));
        let id = ui.id().with(("callout-handle", page, index, photo));
        let tip = if photo { tl!("Drag to size the photo") } else { tl!("Drag to set how wide the text wraps") };
        let resp = ui.interact(r.expand(3.0), id, Sense::drag()).on_hover_text(tip);
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if photo { egui::CursorIcon::ResizeNwSe } else { egui::CursorIcon::ResizeHorizontal });
        }
        painter.rect(r, 1.0, Color32::WHITE, Stroke::new(1.5, accent), egui::StrokeKind::Middle);
        let to = ui.input(|i| i.pointer.latest_pos()).map(|p| cx.to_user(p));
        if resp.dragged() {
            busy = true;
            if let Some(to) = to {
                preview(&painter, cx, &parts, photo, to, accent);
            }
        }
        if resp.drag_stopped()
            && let Some(to) = to
        {
            let (photo_height, text_width) = if photo { (photo_height(&parts, to), None) } else { (None, text_width(&parts, to)) };
            if photo_height.is_some() || text_width.is_some() {
                view.pending_edit = Some(Edit::SetCalloutParts { page, index, photo_height, text_width });
            }
        }
    }
    busy
}

/// The photo or the text edge as it would be, while dragging.
fn preview(painter: &egui::Painter, cx: &PageCx<'_>, parts: &Parts, photo: bool, to: [f64; 2], accent: Color32) {
    let stroke = Stroke::new(1.5, accent);
    if photo {
        let Some(h) = photo_height(parts, to) else { return };
        let (corner, anchor) = photo_corners(parts);
        let s = h / (parts.photo[3] - parts.photo[1]).max(1e-6);
        let c = [anchor[0] + (corner[0] - anchor[0]) * s, anchor[1] + (corner[1] - anchor[1]) * s];
        painter.rect_stroke(Rect::from_two_pos(cx.to_screen(anchor), cx.to_screen(c)), 0.0, stroke, egui::StrokeKind::Middle);
    } else {
        let Some(w) = text_width(parts, to) else { return };
        let x = if parts.side == TextSide::Right { parts.text[0] + w } else { parts.text[2] - w };
        let (a, b): (Pos2, Pos2) = (cx.to_screen([x, parts.text[1]]), cx.to_screen([x, parts.text[3]]));
        painter.line_segment([a, b], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(side: TextSide) -> Parts {
        // A 96 × 72 photo, the text 150 wide.
        match side {
            TextSide::Right => Parts { photo: [6.0, 22.0, 102.0, 94.0], text: [110.0, 6.0, 260.0, 94.0], side },
            TextSide::Left => Parts { photo: [164.0, 22.0, 260.0, 94.0], text: [6.0, 6.0, 156.0, 94.0], side },
        }
    }

    #[test]
    fn handles_measure_along_the_photo_and_across_the_text() {
        let p = parts(TextSide::Right);
        // Dragging the inner bottom corner twice as far from the fixed top-left doubles the photo.
        assert!((photo_height(&p, [198.0, -50.0]).unwrap() - 144.0).abs() < 1e-9);
        // The text edge 40 further out is 40 wider; in past the minimum stops there.
        assert!((text_width(&p, [300.0, 50.0]).unwrap() - 190.0).abs() < 1e-9);
        assert_eq!(text_width(&p, [0.0, 50.0]), Some(*TEXT_WIDTHS.start()));
        // Text on the left: dragging its edge left widens it.
        let l = parts(TextSide::Left);
        assert!((text_width(&l, [-34.0, 50.0]).unwrap() - 190.0).abs() < 1e-9);
        assert!((photo_height(&l, [68.0, -50.0]).unwrap() - 144.0).abs() < 1e-9);
        // Nonsense in, a bounded answer or none out.
        assert!(photo_height(&p, [f64::NAN, 0.0]).is_none());
        assert_eq!(photo_height(&p, [1e9, -1e9]), Some(*PHOTO_HEIGHTS.end()));
    }
}
