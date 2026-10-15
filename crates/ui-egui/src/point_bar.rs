//! The floating bar shown while a shape is placed point by point (perimeter and area
//! measurements; polygon, connected-lines and cloud markups): Finish, Close, Undo point and Cancel
//! as buttons beside the last point. Double-click and Enter also finish, but on a tablet there is
//! no Enter key and a double tap is easy to miss, so the bar is the reliable way.

use egui::{Pos2, Rect, vec2};

use crate::icons;
use crate::theme::Tokens;

/// Screen distance within which tapping the first point closes the shape: generous enough for a
/// finger or a pen.
pub const CLOSE_RADIUS: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointAction {
    /// Keep the shape as drawn (open for a perimeter or connected lines).
    Finish,
    /// Join the last point to the first, then keep it.
    Close,
    /// Remove the last point.
    Undo,
    /// Throw the shape away.
    Cancel,
}

/// Draw the bar near `anchor` (the last point on screen), inside `bounds`. `can_finish`: there
/// are enough points to keep; `closes`: the shape can be closed (a "Close" button is shown).
pub fn show(
    ctx: &egui::Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    anchor: Pos2,
    bounds: Rect,
    can_finish: bool,
    closes: bool,
) -> Option<PointAction> {
    let t = Tokens::get(ctx);
    let buttons = if closes { 4.0 } else { 3.0 };
    let size = vec2(buttons * 40.0 + 12.0, 48.0);
    // Below and to the right of the point, kept on screen.
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
    egui::Area::new(egui::Id::new(("point-bar", id))).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.card).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_enabled_ui(can_finish, |ui| {
                    if icons::button(ui, "check", 36.0, false, tl!("Finish")).clicked() {
                        action = Some(PointAction::Finish);
                    }
                    if closes && icons::button(ui, "pentagon", 36.0, false, tl!("Close shape")).clicked() {
                        action = Some(PointAction::Close);
                    }
                });
                if icons::button(ui, "undo-2", 36.0, false, tl!("Undo last point")).clicked() {
                    action = Some(PointAction::Undo);
                }
                if icons::button(ui, "x", 36.0, false, tl!("Cancel")).clicked() {
                    action = Some(PointAction::Cancel);
                }
            });
        });
    });
    action
}
