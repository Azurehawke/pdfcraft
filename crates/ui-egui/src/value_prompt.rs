//! The value box: when a tool needs one value after its clicks (the real length of a line
//! being calibrated, for example), a small box opens beside the last point asking for it. It
//! looks and behaves the same for every tool: a title saying what to type, a text field that
//! already has the keyboard, Enter or the check button applies, Escape or the cross cancels,
//! and a problem with what was typed shows underneath in red. Lengths are typed the way Draft
//! takes them (`12'6"`, `150`, `3.5m`).

use egui::{Pos2, Rect, vec2};

use crate::icons;
use crate::theme::{self, Tokens};

/// What the user did in the box this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptAction {
    Apply,
    Cancel,
}

/// Draw the box near `anchor` (the last point on screen), kept inside `bounds`. `text` is what
/// has been typed; `extra` adds options under the field (a checkbox, say).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    anchor: Pos2,
    bounds: Rect,
    title: &str,
    hint: &str,
    text: &mut String,
    error: Option<&str>,
    extra: impl FnOnce(&mut egui::Ui),
) -> Option<PromptAction> {
    let t = Tokens::get(ctx);
    let size = vec2(250.0, 96.0);
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
    egui::Area::new(area_id).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.card).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_max_width(size.x);
            ui.label(egui::RichText::new(title).font(theme::semibold(13.0)));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let field = ui.add(egui::TextEdit::singleline(text).desired_width(150.0).hint_text(hint).id(area_id.with("field")));
                // The field has the keyboard as soon as the box opens, and again after a
                // click elsewhere in the box.
                if ui.memory(|m| m.focused().is_none()) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    action = Some(PromptAction::Apply);
                }
                if icons::button(ui, "check", 28.0, false, tl!("Apply")).clicked() {
                    action = Some(PromptAction::Apply);
                }
                if icons::button(ui, "x", 28.0, false, tl!("Cancel")).clicked() {
                    action = Some(PromptAction::Cancel);
                }
            });
            extra(ui);
            if let Some(e) = error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
    });
    if action.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = Some(PromptAction::Cancel);
    }
    action
}
