//! RevuCraft: the pen tool, which draws Bézier curves as VectorCraft's pen (Illustrator's Pen
//! tool) does, and the Direct selection arrow, which edits their points and handles
//! ([`pdfcraft_engine::bezier`]).
//!
//! Pen: a click places a corner; pressing and dragging pulls the new point's handles out (mirrored
//! either side), bending the curve into it as the pointer moves. Enter, Escape or a double-click
//! ends the curve; a click on its first point closes it.
//!
//! Direct selection: a click picks a curve; its points show. Drag a point (its handles go along)
//! or a handle (the other side mirrors; with Alt only this one moves). Alt-click a point to turn it
//! between corner and smooth; double-click the curve to add a point there; Delete removes the
//! picked point.

use egui::{Color32, Pos2, Stroke};
use pdfcraft_engine::Edit;
use pdfcraft_engine::bezier::{self, Node};

use crate::QuickTool;
use crate::canvas::DocView;
use crate::comments::{CommentTool, PageCx};

type Pt = [f64; 2];

/// How near the pointer must be to pick a point or handle (screen points).
const PICK: f32 = 8.0;
/// How far a press must drag to pull handles out rather than place a corner (screen points).
const PULL: f32 = 3.0;

/// What the pen and the Direct selection arrow remember between frames.
#[derive(Clone, Debug, Default)]
pub struct BezierState {
    /// The curve being drawn: its page and points so far.
    draft: Option<(usize, Vec<Node>)>,
    /// The handles of the point just placed are being pulled out.
    pulling: bool,
    /// Direct selection: the curve (page, index) and its point picked.
    picked: Option<(usize, usize, usize)>,
    /// Direct selection: a point or handle being dragged.
    drag: Option<Drag>,
}

#[derive(Clone, Debug)]
struct Drag {
    page: usize,
    index: usize,
    node: usize,
    /// `None`: the point itself; `Some(true)`: its handle after; `Some(false)`: before.
    handle: Option<bool>,
    nodes: Vec<Node>,
    closed: bool,
    from: Pt,
}

impl BezierState {
    /// Whether a curve is being drawn or edited (the page's other tools leave the pointer alone).
    pub fn busy(&self) -> bool {
        self.draft.is_some() || self.drag.is_some()
    }
}

fn accent() -> Color32 {
    Color32::from_rgb(242, 163, 58)
}

/// The pen's and the Direct selection arrow's input and drawing on one page.
pub(crate) fn page_input(ui: &egui::Ui, resp: &egui::Response, doc: &pdfcraft_engine::Document, view: &mut DocView, cx: &PageCx<'_>) {
    if !cx.allowed {
        return;
    }
    match cx.tool {
        QuickTool::Bezier => pen(ui, resp, view, cx),
        QuickTool::DirectSelect => direct(ui, resp, doc, view, cx),
        _ => {}
    }
}

fn pen(ui: &egui::Ui, resp: &egui::Response, view: &mut DocView, cx: &PageCx<'_>) {
    let page = cx.page;
    let (pointer, pressed, down, released) = ui.input(|i| {
        (i.pointer.interact_pos().or(i.pointer.hover_pos()), i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released())
    });
    let (enter, escape) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    let st = &mut view.bezier;
    // A curve on another page ends where it is when the pen moves on.
    if st.draft.as_ref().is_some_and(|(p, _)| *p != page) {
        return;
    }
    let mut commit: Option<bool> = None;
    if let Some(pos) = pointer.filter(|p| cx.zone.contains(*p)) {
        let at = cx.to_user(pos);
        if pressed && resp.hovered() {
            let closing = st.draft.as_ref().is_some_and(|(_, n)| n.len() >= 2 && n.first().is_some_and(|f| cx.to_screen(f.at).distance(pos) <= PICK));
            if closing {
                commit = Some(true);
            } else {
                st.draft.get_or_insert_with(|| (page, Vec::new())).1.push(Node::corner(at));
                st.pulling = true;
            }
        } else if st.pulling
            && down
            && let Some((_, nodes)) = st.draft.as_mut()
            && let Some(last) = nodes.last_mut()
        {
            // Pulling the handles out: the curve into the point bends as the pointer moves.
            *last = if cx.to_screen(last.at).distance(pos) > PULL { Node::smooth(last.at, at) } else { Node::corner(last.at) };
        }
    }
    if released {
        st.pulling = false;
    }
    if resp.double_clicked()
        && let Some((_, nodes)) = st.draft.as_mut()
    {
        // The double-click's second press placed a point on top of the last: drop it.
        if let (Some(a), Some(b)) = (nodes.last(), nodes.iter().rev().nth(1))
            && cx.to_screen(a.at).distance(cx.to_screen(b.at)) <= PICK
        {
            nodes.pop();
        }
        commit = Some(false);
    }
    if enter || escape {
        commit = Some(false);
    }
    if let Some(closed) = commit
        && let Some((page, nodes)) = st.draft.take()
    {
        st.pulling = false;
        if nodes.len() >= 2 {
            view.pending_edit =
                Some(Edit::AddPath { page, nodes, closed, style: cx.prefs.style(CommentTool::Line), author: cx.prefs.author.clone() });
        }
        return;
    }
    // The curve so far, the point's handles, and the next segment to the pointer.
    let Some((_, nodes)) = st.draft.as_ref() else { return };
    let painter = ui.painter();
    let stroke = Stroke::new(1.5, accent());
    paint_curve(painter, cx, nodes, false, stroke);
    if let (Some(last), Some(pos)) = (nodes.last(), pointer)
        && !st.pulling
    {
        let to = cx.to_user(pos);
        paint_segment(painter, cx, &[last.at, last.after, to, to], Stroke::new(1.0, accent().gamma_multiply(0.6)));
    }
    for n in nodes {
        paint_anchor(painter, cx, n, false);
    }
    if let Some(last) = nodes.last() {
        paint_handles(painter, cx, last);
    }
}

fn direct(ui: &egui::Ui, resp: &egui::Response, doc: &pdfcraft_engine::Document, view: &mut DocView, cx: &PageCx<'_>) {
    let page = cx.page;
    let curves = doc.paths(page);
    let (pointer, pressed, down, alt, delete) = ui.input(|i| {
        (
            i.pointer.interact_pos().or(i.pointer.hover_pos()),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.alt,
            i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
        )
    });
    let selected = view.comments.selected.filter(|(p, _)| *p == page).and_then(|(_, i)| curves.iter().find(|(ci, ..)| *ci == i).cloned());
    // A drag under way.
    if let Some(mut d) = view.bezier.drag.take().filter(|d| d.page == page) {
        if let Some(pos) = pointer {
            let at = cx.to_user(pos);
            let moved = edited(&d, at, alt);
            if down {
                paint_curve(ui.painter(), cx, &moved, d.closed, Stroke::new(1.5, accent()));
                if let Some(n) = moved.get(d.node) {
                    paint_handles(ui.painter(), cx, n);
                }
                view.bezier.drag = Some(d);
                return;
            }
            if (at[0] - d.from[0]).hypot(at[1] - d.from[1]) > 1e-6 {
                d.nodes = moved;
                view.pending_edit = Some(Edit::SetPath { page, index: d.index, nodes: d.nodes, closed: d.closed });
            }
        }
        return;
    }
    if let Some((index, nodes, closed)) = &selected {
        let picked = view.bezier.picked.filter(|(p, i, n)| *p == page && i == index && *n < nodes.len()).map(|(_, _, n)| n);
        // Delete the picked point (a curve keeps two).
        if delete
            && let Some(k) = picked
            && nodes.len() > 2
        {
            let mut left = nodes.clone();
            left.remove(k);
            view.bezier.picked = None;
            view.pending_edit = Some(Edit::SetPath { page, index: *index, nodes: left, closed: *closed });
            return;
        }
        if let Some(pos) = pointer.filter(|p| cx.zone.contains(*p)) {
            let at = cx.to_user(pos);
            if pressed && resp.hovered() {
                // The picked point's handles, then any point.
                let handle = picked.and_then(|k| {
                    let n = nodes.get(k)?;
                    [(true, n.after), (false, n.before)]
                        .into_iter()
                        .find(|(_, h)| !n.is_corner() && cx.to_screen(*h).distance(pos) <= PICK)
                        .map(|(after, _)| (k, Some(after)))
                });
                let point = || nodes.iter().position(|n| cx.to_screen(n.at).distance(pos) <= PICK).map(|k| (k, None));
                if let Some((k, h)) = handle.or_else(point) {
                    if alt && h.is_none() {
                        // Alt-click: corner ↔ smooth.
                        let turned = toggle(nodes, *closed, k);
                        view.pending_edit = Some(Edit::SetPath { page, index: *index, nodes: turned, closed: *closed });
                    } else {
                        view.bezier.picked = Some((page, *index, k));
                        view.bezier.drag = Some(Drag { page, index: *index, node: k, handle: h, nodes: nodes.clone(), closed: *closed, from: at });
                    }
                    return;
                }
            }
            // Double-click on the curve: a new point there.
            if resp.double_clicked()
                && let Some((seg, t, d)) = bezier::nearest(nodes, *closed, at)
                && d * f64::from(px_per_unit(cx)) <= f64::from(PICK)
            {
                let more = bezier::split(nodes, *closed, seg, t);
                view.bezier.picked = Some((page, *index, seg + 1));
                view.pending_edit = Some(Edit::SetPath { page, index: *index, nodes: more, closed: *closed });
                return;
            }
        }
        paint_curve(ui.painter(), cx, nodes, *closed, Stroke::new(1.0, accent()));
        for (k, n) in nodes.iter().enumerate() {
            paint_anchor(ui.painter(), cx, n, picked == Some(k));
        }
        if let Some(n) = picked.and_then(|k| nodes.get(k)) {
            paint_handles(ui.painter(), cx, n);
        }
    }
    // A click picks the curve under the pointer (or lets go of the one picked).
    if resp.clicked()
        && let Some(pos) = pointer.filter(|p| cx.zone.contains(*p))
    {
        let at = cx.to_user(pos);
        let tolerance = f64::from(PICK) / f64::from(px_per_unit(cx));
        let hit = curves
            .iter()
            .filter_map(|(i, n, c)| bezier::nearest(n, *c, at).map(|(_, _, d)| (*i, d)))
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match hit {
            Some((i, _)) => {
                if selected.as_ref().is_none_or(|(s, ..)| *s != i) {
                    view.bezier.picked = None;
                }
                view.comments.select_one(page, i);
            }
            None if selected.is_some() && view.bezier.drag.is_none() => {
                view.bezier.picked = None;
                view.comments.set_group(page, &[]);
            }
            None => {}
        }
    }
}

/// Screen points per user unit on this page.
fn px_per_unit(cx: &PageCx<'_>) -> f32 {
    (cx.xf.rect.width() / cx.xf.pw).max(1e-6)
}

/// The curve as it would be with the dragged point or handle at `at`.
fn edited(d: &Drag, at: Pt, alt: bool) -> Vec<Node> {
    let mut nodes = d.nodes.clone();
    let Some(n) = nodes.get_mut(d.node) else { return nodes };
    match d.handle {
        None => *n = n.moved([at[0] - n.at[0], at[1] - n.at[1]]),
        Some(after) => {
            let mirror = [2.0 * n.at[0] - at[0], 2.0 * n.at[1] - at[1]];
            if after {
                n.after = at;
                if !alt {
                    n.before = mirror;
                }
            } else {
                n.before = at;
                if !alt {
                    n.after = mirror;
                }
            }
        }
    }
    nodes
}

/// The curve with point `k` turned from corner to smooth (handles along its neighbours), or back.
fn toggle(nodes: &[Node], closed: bool, k: usize) -> Vec<Node> {
    let mut out = nodes.to_vec();
    let count = nodes.len();
    let prev = if k > 0 {
        nodes.get(k - 1)
    } else if closed {
        nodes.last()
    } else {
        None
    };
    let next = if k + 1 < count {
        nodes.get(k + 1)
    } else if closed {
        nodes.first()
    } else {
        None
    };
    let Some(n) = out.get_mut(k) else { return out };
    if n.is_corner() {
        let a = prev.map_or(n.at, |p| p.at);
        let b = next.map_or(n.at, |p| p.at);
        let dir = [(b[0] - a[0]) / 6.0, (b[1] - a[1]) / 6.0];
        if dir[0].hypot(dir[1]) > 1e-9 {
            *n = Node::smooth(n.at, [n.at[0] + dir[0], n.at[1] + dir[1]]);
        }
    } else {
        *n = Node::corner(n.at);
    }
    out
}

fn paint_segment(painter: &egui::Painter, cx: &PageCx<'_>, s: &[Pt; 4], stroke: Stroke) {
    let pts: Vec<Pos2> = (0..=24).map(|i| cx.to_screen(bezier::point_at(s, f64::from(i) / 24.0))).collect();
    painter.add(egui::Shape::line(pts, stroke));
}

fn paint_curve(painter: &egui::Painter, cx: &PageCx<'_>, nodes: &[Node], closed: bool, stroke: Stroke) {
    for s in bezier::segments(nodes, closed) {
        paint_segment(painter, cx, &s, stroke);
    }
}

fn paint_anchor(painter: &egui::Painter, cx: &PageCx<'_>, n: &Node, picked: bool) {
    let r = egui::Rect::from_center_size(cx.to_screen(n.at), egui::Vec2::splat(7.0));
    painter.rect(r, 0.0, if picked { accent() } else { Color32::WHITE }, Stroke::new(1.0, accent()), egui::StrokeKind::Middle);
}

fn paint_handles(painter: &egui::Painter, cx: &PageCx<'_>, n: &Node) {
    if n.is_corner() {
        return;
    }
    let at = cx.to_screen(n.at);
    for h in [n.before, n.after] {
        let p = cx.to_screen(h);
        painter.line_segment([at, p], Stroke::new(1.0, accent()));
        painter.circle(p, 3.5, Color32::WHITE, Stroke::new(1.0, accent()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_mirror_unless_alt_breaks_them() {
        let d = Drag {
            page: 0,
            index: 0,
            node: 0,
            handle: Some(true),
            nodes: vec![Node::smooth([0.0, 0.0], [10.0, 0.0]), Node::corner([50.0, 0.0])],
            closed: false,
            from: [10.0, 0.0],
        };
        let n = edited(&d, [0.0, 10.0], false)[0];
        assert_eq!((n.after, n.before), ([0.0, 10.0], [0.0, -10.0]));
        let n = edited(&d, [0.0, 10.0], true)[0];
        assert_eq!((n.after, n.before), ([0.0, 10.0], [-10.0, 0.0]));
        // Dragging the point takes its handles along.
        let moved = edited(&Drag { handle: None, ..d.clone() }, [5.0, 5.0], false)[0];
        assert_eq!((moved.at, moved.after), ([5.0, 5.0], [15.0, 5.0]));
    }

    #[test]
    fn alt_click_turns_a_point_smooth_and_back() {
        let nodes = vec![Node::corner([0.0, 0.0]), Node::corner([30.0, 30.0]), Node::corner([60.0, 0.0])];
        let smooth = toggle(&nodes, false, 1);
        assert!(!smooth[1].is_corner());
        assert_eq!(smooth[1].after, [40.0, 30.0]);
        assert!(toggle(&smooth, false, 1)[1].is_corner());
    }
}
