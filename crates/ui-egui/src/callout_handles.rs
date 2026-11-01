//! RevuCraft: the handle on a selected callout's arrow tip, and the two handles of a selected
//! photo callout. One, on the photo's inner bottom
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
    // A selected space: its name, dragged to another spot inside or beside it.
    let mut busy = space_label_handle(ui, doc, view, cx, accent, page, index);
    // A selected callout: its connection points, and its arrow's end to glue to one of them.
    busy |= glue_handles(ui, doc, view, cx, accent, page, index);
    // A selected arrow: each end, dragged onto a box's connection point, glues there.
    busy |= arrow_end_handles(ui, doc, view, cx, accent, page, index);
    // Any callout: its arrow's tip, dragged to point it somewhere else (the box stays).
    busy |= tip_handle(ui, doc, view, cx, accent, page, index);
    let Some(parts) = doc.callout_parts(page, index) else { return busy };
    let photo_at = cx.to_screen(photo_corners(&parts).0);
    let text_at = cx.to_screen(text_edge(&parts));
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

/// How close (points, on screen) a dragged arrow end must come to a connection point to glue.
const SNAP_PX: f32 = 12.0;

/// The connection point of a callout's box nearest `p` (on screen) within [`SNAP_PX`].
fn snap_site(sites: &[(u32, [f64; 2])], cx: &PageCx<'_>, p: Pos2) -> Option<u32> {
    sites.iter().map(|(s, q)| (*s, cx.to_screen(*q).distance(p))).filter(|(_, d)| *d <= SNAP_PX).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(s, _)| s)
}

/// A selected callout's connection points (dots on its box) and its arrow's end on the box (a
/// square). Dragged onto a point, the arrow glues there on letting go; dropped anywhere else, or
/// with Alt held, it is freed and meets the box on the side facing its knee ([`Edit::SetCalloutGlue`]).
/// Returns `true` while the end is being dragged.
fn glue_handles(ui: &egui::Ui, doc: &Document, view: &mut DocView, cx: &PageCx<'_>, accent: Color32, page: usize, index: usize) -> bool {
    let Some(g) = doc.callout_glue(page, index) else { return false };
    let sites: Vec<(u32, [f64; 2])> = (0..pdfcraft_engine::callout_glue::SITES as u32)
        .filter_map(|s| pdfcraft_engine::callout_glue::site_point(g.rect, s).map(|p| (s, p)))
        .collect();
    let painter = ui.painter().clone();
    for (s, q) in &sites {
        let at = cx.to_screen(*q);
        let fill = if g.site == Some(*s) { accent } else { Color32::WHITE };
        painter.circle(at, 3.5, fill, Stroke::new(1.25, accent));
    }
    let attach_at = cx.to_screen(g.attach);
    let r = Rect::from_center_size(attach_at, vec2(HANDLE, HANDLE));
    let id = ui.id().with(("callout-glue", page, index));
    let resp = ui
        .interact(r.expand(3.0), id, Sense::drag())
        .on_hover_text(tl!("Drag onto a connection point to glue the arrow; drop it elsewhere, or hold Alt, to free it"));
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    painter.rect(r, 1.0, Color32::WHITE, Stroke::new(1.5, accent), egui::StrokeKind::Middle);
    let pointer = ui.input(|i| i.pointer.latest_pos());
    if resp.dragged()
        && let Some(p) = pointer
    {
        painter.line_segment([attach_at, p], Stroke::new(1.5, accent.gamma_multiply(0.7)));
        if let Some(hit) = snap_site(&sites, cx, p).and_then(|s| sites.iter().find(|(i, _)| *i == s)) {
            painter.circle_filled(cx.to_screen(hit.1), 4.5, accent);
        }
        return true;
    }
    if resp.drag_stopped()
        && let Some(p) = pointer
    {
        let alt = ui.input(|i| i.modifiers.alt);
        let site = if alt { None } else { snap_site(&sites, cx, p) };
        if site != g.site {
            view.pending_edit = Some(Edit::SetCalloutGlue { page, index, site });
        }
    }
    false
}

/// The connection point of any box on the page nearest `p` (on screen) within [`SNAP_PX`], as the
/// box's name and the site.
fn snap_target(targets: &[(String, [f64; 4])], cx: &PageCx<'_>, p: Pos2) -> Option<(String, u32)> {
    let mut best: Option<(&str, u32, f32)> = None;
    for (name, bx) in targets {
        for site in 0..pdfcraft_engine::callout_glue::SITES as u32 {
            let Some(q) = pdfcraft_engine::callout_glue::site_point(*bx, site) else { continue };
            let d = cx.to_screen(q).distance(p);
            if d <= SNAP_PX && best.is_none_or(|b| d < b.2) {
                best = Some((name.as_str(), site, d));
            }
        }
    }
    best.map(|(name, site, _)| (name.to_string(), site))
}

/// A selected arrow's two ends. Dragged onto a box's connection point, an end glues there on
/// letting go; dropped elsewhere, or with Alt held, it is freed and sits where it was dropped
/// ([`Edit::SetArrowGlue`] with a move). Returns `true` while an end is being dragged.
fn arrow_end_handles(ui: &egui::Ui, doc: &Document, view: &mut DocView, cx: &PageCx<'_>, accent: Color32, page: usize, index: usize) -> bool {
    let Some(a) = doc.info.annotations.iter().find(|a| a.page == page && a.index == index) else { return false };
    if a.subtype != "Line" || a.points.len() != 2 {
        return false;
    }
    let ends: Vec<[f64; 2]> = a.points.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect();
    let (Some(start), Some(end)) = (ends.first().copied(), ends.get(1).copied()) else { return false };
    let targets = doc.glue_targets(page);
    let painter = ui.painter().clone();
    let mut busy = false;
    for (is_start, at_user) in [(true, start), (false, end)] {
        let at = cx.to_screen(at_user);
        let r = Rect::from_center_size(at, vec2(HANDLE, HANDLE));
        let id = ui.id().with(("arrow-end", page, index, is_start));
        let resp = ui
            .interact(r.expand(3.0), id, Sense::drag())
            .on_hover_text(tl!("Drag onto a connection point to glue this end; drop it elsewhere, or hold Alt, to leave it free"));
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        painter.circle(at, HANDLE / 2.0, Color32::WHITE, Stroke::new(1.5, accent));
        let pointer = ui.input(|i| i.pointer.latest_pos());
        if resp.dragged() {
            busy = true;
            if let Some(p) = pointer {
                painter.line_segment([at, p], Stroke::new(1.5, accent.gamma_multiply(0.7)));
            }
            continue;
        }
        if resp.drag_stopped()
            && let Some(p) = pointer
        {
            let alt = ui.input(|i| i.modifiers.alt);
            let hit = if alt { None } else { snap_target(&targets, cx, p) };
            // A drop on a point glues the end there. Elsewhere a glued end is freed and moved as one
            // step; a free end is left to the line's own drag, as before.
            if let Some(target) = hit {
                view.pending_edit = Some(Edit::SetArrowGlue { page, index, start: is_start, target: Some(target) });
            } else if doc.arrow_end_glued(page, index, is_start) {
                let mut points = ends.clone();
                let slot = if is_start { 0 } else { 1 };
                if let Some(v) = points.get_mut(slot) {
                    *v = cx.to_user(p);
                }
                view.pending_edit = Some(Edit::Batch {
                    label: tl!("Move the arrow's end").to_string(),
                    edits: vec![Edit::SetArrowGlue { page, index, start: is_start, target: None }, Edit::ReshapeAnnotation { page, index, points }],
                });
            }
        }
    }
    busy
}

/// The handle on a selected space's name: dragged, the name moves there on letting go
/// ([`Edit::SetSpaceLabel`]); double-clicked, it goes back inside the space. Returns `true` while
/// it is being dragged.
fn space_label_handle(ui: &egui::Ui, doc: &Document, view: &mut DocView, cx: &PageCx<'_>, accent: Color32, page: usize, index: usize) -> bool {
    let Some(at) = doc.space_label(page, index) else { return false };
    let screen = cx.to_screen(at);
    let r = Rect::from_center_size(screen, vec2(HANDLE, HANDLE));
    let id = ui.id().with(("space-label", page, index));
    let resp =
        ui.interact(r.expand(3.0), id, Sense::click_and_drag()).on_hover_text(tl!("Drag to move the space's name; double-click to put it back"));
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    if resp.double_clicked() {
        view.pending_edit = Some(Edit::SetSpaceLabel { page, index, at: None });
        return false;
    }
    let painter = ui.painter();
    painter.rect(r, 1.0, Color32::WHITE, Stroke::new(1.5, accent), egui::StrokeKind::Middle);
    let to = ui.input(|i| i.pointer.latest_pos());
    if resp.dragged()
        && let Some(p) = to
    {
        painter.line_segment([screen, p], Stroke::new(1.5, accent.gamma_multiply(0.7)));
        painter.circle_filled(p, 3.0, accent);
        return true;
    }
    if resp.drag_stopped()
        && let Some(p) = to
        && p.distance(screen) >= 2.0
    {
        view.pending_edit = Some(Edit::SetSpaceLabel { page, index, at: Some(cx.to_user(p)) });
    }
    false
}

/// The handle on a callout's arrow tip: dragged, the tip moves and the leader follows it on letting
/// go ([`Edit::SetCalloutTip`]). Returns `true` while it is being dragged.
fn tip_handle(ui: &egui::Ui, doc: &Document, view: &mut DocView, cx: &PageCx<'_>, accent: Color32, page: usize, index: usize) -> bool {
    let Some(tip) = doc.callout_tip(page, index) else { return false };
    let at = cx.to_screen(tip);
    let r = Rect::from_center_size(at, vec2(HANDLE, HANDLE));
    let id = ui.id().with(("callout-tip", page, index));
    let resp = ui.interact(r.expand(3.0), id, Sense::drag()).on_hover_text(tl!("Drag to point the arrow somewhere else"));
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let painter = ui.painter();
    painter.circle(at, HANDLE / 2.0, Color32::WHITE, Stroke::new(1.5, accent));
    let to = ui.input(|i| i.pointer.latest_pos());
    if resp.dragged()
        && let Some(p) = to
    {
        painter.line_segment([at, p], Stroke::new(1.5, accent.gamma_multiply(0.7)));
        painter.circle_filled(p, 3.0, accent);
        return true;
    }
    if resp.drag_stopped()
        && let Some(p) = to
        && p.distance(at) >= 2.0
    {
        view.pending_edit = Some(Edit::SetCalloutTip { page, index, tip: cx.to_user(p) });
    }
    false
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
