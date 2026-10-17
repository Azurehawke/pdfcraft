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
}

impl<'a> Prompt<'a> {
    pub fn new(title: &'a str, hint: &'a str) -> Self {
        Prompt { title, hint, error: None, focus: true, multiline: false }
    }
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
    // Below and to the right of the point, kept on screen (as the Finish/Undo bar is).
    let mut at = anchor + vec2(16.0, 18.0);
    if at.x + size.x > bounds.right() {
        at.x = anchor.x - 16.0 - size.x;
    }
    if at.y + size.y > bounds.bottom() {
        at.y = anchor.y - 18.0 - size.y;
    }
    at.x = at.x.max(bounds.left());
    at.y = at.y.max(bounds.top());
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
