//! RevuCraft's status bar, under the drawing: where the pointer is in real-world units (from the
//! page's measurement scale), the scale, snapping and ortho, and the current layer.

use egui::{Align, Layout, Stroke};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, RightPanel};

/// The pointer's position as the scale writes lengths, from the page's origin (its lower-left
/// corner), or `None` without a pointer over a page.
fn position(app: &PdfCraftApp, index: usize) -> Option<(String, String, String)> {
    let view = app.views.get(index)?;
    let (page, at) = view.cursor?;
    let doc = app.session.get(view.id)?;
    let scale = doc.measurement_scale(page, at).ok()?;
    let origin = crate::draft::origin(view, doc, page);
    let k = scale.distance_factor;
    let x = scale.format_length((at[0] - origin[0]) * scale.x * k).ok()?;
    let y = scale.format_length((at[1] - origin[1]) * scale.y * k).ok()?;
    let calibrated = doc.real_size_points(page, at, 1.0).is_some();
    let ratio = if calibrated { scale.ratio.clone() } else { tl!("Not calibrated (paper size)").to_string() };
    Some((x, y, ratio))
}

/// Draw the status bar for the active document.
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let Some((index, id)) = app.active_ids() else { return };
    let t = Tokens::get(ui.ctx());
    let pos = position(app, index);
    if let Some(v) = app.views.get_mut(index) {
        // The canvas sets it again while the pointer is over a page.
        v.cursor = None;
    }
    let layers = app.session.layer_details(id);
    let current = app.session.get(id).and_then(|d| d.current_layer);
    let layer = current.and_then(|c| layers.iter().find(|l| (l.id.num, l.id.generation) == c));
    egui::Panel::bottom("status_bar")
        .exact_size(28.0)
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(10, 0)).stroke(Stroke::new(1.0, t.divider)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let font = theme::regular(12.0);
                match &pos {
                    Some((x, y, ratio)) => {
                        ui.label(egui::RichText::new(format!("X {x}")).font(theme::medium(12.0)).color(t.text));
                        ui.label(egui::RichText::new(format!("Y {y}")).font(theme::medium(12.0)).color(t.text));
                        ui.label(egui::RichText::new(ratio).font(font.clone()).color(t.text_muted));
                    }
                    None => {
                        ui.label(egui::RichText::new(tl!("Point at the drawing to see coordinates")).font(font.clone()).color(t.text_faint));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Current layer: click to manage layers.
                    let (name, color) = match layer {
                        Some(l) => (l.name.clone(), crate::revu::color32(l.color)),
                        None => (tl!("No layer").to_string(), t.text_faint),
                    };
                    let mut job = egui::text::LayoutJob::default();
                    job.append("● ", 0.0, egui::TextFormat { color, font_id: font.clone(), ..Default::default() });
                    job.append(&name, 0.0, egui::TextFormat { color: t.text, font_id: font.clone(), ..Default::default() });
                    if ui
                        .add(egui::Label::new(job).sense(egui::Sense::click()))
                        .on_hover_text(tl!("Current layer (click to manage layers)"))
                        .clicked()
                        && !app.is_docked(RightPanel::Layers)
                    {
                        app.toggle_docked(RightPanel::Layers);
                    }
                    ui.separator();
                    if let Some(v) = app.views.get_mut(index) {
                        let _ = ui
                            .toggle_value(&mut v.measure.snap_enabled, egui::RichText::new(tl!("Snap")).font(font.clone()))
                            .on_hover_text(tl!("Snap to ends, middles and crossings of the drawing"));
                    }
                    let _ = ui
                        .toggle_value(&mut app.draft.ortho, egui::RichText::new(tl!("Ortho")).font(font.clone()))
                        .on_hover_text(tl!("Draw only horizontal and vertical lines (hold Shift for the other way)"));
                });
            });
        });
}
