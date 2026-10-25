//! The value box: when a tool needs a value after its clicks (the real length of a line being
//! calibrated, a scale factor, an angle, a length while drawing, a photo's caption, a symbol's
//! label), a small box opens beside the point asking for it. It looks and behaves the same for
//! every tool: a title saying what to type, a text field, Enter or the check button applies,
//! Escape or the cross cancels, and a problem with what was typed shows underneath in red.
//! Lengths are typed the way Draft takes them (`12'6"`, `150`, `3.5m`).

use egui::{Pos2, Rect, vec2};

use crate::icons;
use crate::theme::{self, Tokens};

/// What the user did in the box this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptAction {
    /// Enter or the check button.
    Apply,
    /// Escape or the cross.
    Cancel,
    /// The field lost the keyboard some other way (a click on the drawing): keep what was typed.
    Leave,
}

/// What the box asks for and how it behaves.
#[derive(Clone, Copy, Debug)]
pub struct Prompt<'a> {
    pub title: &'a str,
    pub hint: &'a str,
    pub error: Option<&'a str>,
    /// The field takes the keyboard when the box opens. Drafting leaves it off: typing reaches
    /// the box anyway, and its keys (Backspace, Enter) keep working on the drawing.
    pub focus: bool,
    /// Several lines (a description): Enter applies, Shift+Enter starts a new line.
    pub multiline: bool,
    /// A point the box keeps clear of (the pointer while drawing from the anchor): the box opens
    /// on the anchor's other side, so the line being drawn, its ruler and its length stay in view.
    pub avoid: Option<Pos2>,
}

impl<'a> Prompt<'a> {
    pub fn new(title: &'a str, hint: &'a str) -> Self {
        Prompt { title, hint, error: None, focus: true, multiline: false, avoid: None }
    }
}

/// Where the box goes: below and to the right of `anchor`, or on the side away from `avoid`,
/// kept on screen (as the Finish/Undo bar is).
fn place(anchor: Pos2, size: egui::Vec2, bounds: Rect, avoid: Option<Pos2>) -> Pos2 {
    let (left, above) = match avoid {
        Some(p) if p.x.is_finite() && p.y.is_finite() => (p.x >= anchor.x, p.y >= anchor.y),
        _ => (false, false),
    };
    let x_left = anchor.x - 16.0 - size.x;
    let x_right = anchor.x + 16.0;
    let y_above = anchor.y - 18.0 - size.y;
    let y_below = anchor.y + 18.0;
    let mut x = if left { x_left } else { x_right };
    let mut y = if above { y_above } else { y_below };
    // Off screen on the chosen side: the other side.
    if x + size.x > bounds.right() {
        x = x_left;
    } else if x < bounds.left() {
        x = x_right;
    }
    if y + size.y > bounds.bottom() {
        y = y_above;
    } else if y < bounds.top() {
        y = y_below;
    }
    egui::pos2(x.min(bounds.right() - size.x).max(bounds.left()), y.min(bounds.bottom() - size.y).max(bounds.top()))
}

/// Draw the box near `anchor` (a point on screen), kept inside `bounds`. `text` is what has
/// been typed; `extra` adds options under the field (a checkbox, say).
pub fn show(
    ctx: &egui::Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    anchor: Pos2,
    bounds: Rect,
    prompt: Prompt<'_>,
    text: &mut String,
    extra: impl FnOnce(&mut egui::Ui),
) -> Option<PromptAction> {
    let t = Tokens::get(ctx);
    let size = vec2(260.0, if prompt.multiline { 140.0 } else { 96.0 });
    let at = place(anchor, size, bounds, prompt.avoid);
    let mut action = None;
    let area_id = egui::Id::new(("value-prompt", id));
    let field_id = area_id.with("field");
    let had_focus = ctx.memory(|m| m.has_focus(field_id));
    egui::Area::new(area_id).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.card).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_max_width(size.x);
            ui.label(egui::RichText::new(prompt.title).font(theme::semibold(13.0)));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                // In a description, Enter applies and Shift+Enter is a new line.
                if prompt.multiline && had_focus && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    action = Some(PromptAction::Apply);
                }
                let edit = if prompt.multiline { egui::TextEdit::multiline(text).desired_rows(3) } else { egui::TextEdit::singleline(text) };
                let field = ui.add(edit.desired_width(160.0).hint_text(prompt.hint).id(field_id));
                // The field has the keyboard as soon as the box opens.
                if prompt.focus && !had_focus && ui.memory(|m| m.focused().is_none()) && !field.lost_focus() {
                    field.request_focus();
                }
                if field.lost_focus() && action.is_none() {
                    action = Some(if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        PromptAction::Cancel
                    } else if !prompt.multiline && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        PromptAction::Apply
                    } else {
                        PromptAction::Leave
                    });
                }
                if icons::button(ui, "check", 28.0, false, tl!("Apply")).clicked() {
                    action = Some(PromptAction::Apply);
                }
                if icons::button(ui, "x", 28.0, false, tl!("Cancel")).clicked() {
                    action = Some(PromptAction::Cancel);
                }
            });
            extra(ui);
            if let Some(e) = prompt.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
    });
    // A click on the box's own buttons also takes the keyboard from the field: the button wins.
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_keeps_clear_of_the_line_being_drawn() {
        let bounds = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 800.0));
        let size = vec2(260.0, 96.0);
        let start = egui::pos2(500.0, 400.0);
        // No pointer to avoid: below right, as before.
        assert_eq!(place(start, size, bounds, None), egui::pos2(516.0, 418.0));
        // Drawing east (a little below): the box goes up and to the left of the start.
        let at = place(start, size, bounds, Some(egui::pos2(700.0, 401.0)));
        assert_eq!(at, egui::pos2(500.0 - 16.0 - 260.0, 400.0 - 18.0 - 96.0));
        assert!(!Rect::from_min_size(at, size).contains(egui::pos2(600.0, 410.0)), "the line and its readout stay in view");
        // Drawing south west: below is taken, so above and to the right.
        assert_eq!(place(start, size, bounds, Some(egui::pos2(300.0, 600.0))), egui::pos2(516.0, 400.0 - 18.0 - 96.0));
        // At the left edge, the box can't go left: it goes right, still on screen.
        let edge = place(egui::pos2(20.0, 400.0), size, bounds, Some(egui::pos2(300.0, 300.0)));
        assert!(edge.x >= 0.0 && edge.x + size.x <= 1000.0 && edge.y + size.y <= 800.0, "{edge:?}");
    }
}
