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
    // What to do next with the tool in hand (the Scale panel keeps its space for settings).
    let picking_area = app.views.get(index).is_some_and(|v| v.measure.area_pick);
    let hint = match app.quick_tool {
        _ if picking_area => Some(tl!("Drag a box around the area that has this scale")),
        crate::QuickTool::Measure(crate::measure_ui::Tool::Calibrate) => Some(tl!("Click two points a known distance apart")),
        crate::QuickTool::Measure(_) => Some(tl!("Click points; finish with Enter or a double-click · Shift for 45°")),
        _ => None,
    };
    egui::Panel::bottom("status_bar")
        .exact_size(28.0)
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(10, 0)).stroke(Stroke::new(1.0, t.divider)))
        .show(ui, |ui| {
            // A phone's width: no hints, so the switches on the right keep their room.
            let compact = ui.available_width() < 600.0;
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = if compact { 8.0 } else { 14.0 };
                let font = theme::regular(12.0);
                match &pos {
                    Some((x, y, ratio)) => {
                        ui.label(egui::RichText::new(format!("X {x}")).font(theme::medium(12.0)).color(t.text));
                        ui.label(egui::RichText::new(format!("Y {y}")).font(theme::medium(12.0)).color(t.text));
                        if !compact {
                            ui.label(egui::RichText::new(ratio).font(font.clone()).color(t.text_muted));
                        }
                    }
                    None if hint.is_none() && !compact => {
                        ui.label(egui::RichText::new(tl!("Point at the drawing to see coordinates")).font(font.clone()).color(t.text_faint));
                    }
                    None => {}
                }
                if let Some(hint) = hint.filter(|_| !compact) {
                    ui.label(egui::RichText::new(hint).font(font.clone()).color(t.text_muted));
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
                        // Snap settings, beside the Snap switch.
                        let more = crate::icons::button(ui, "chevron-up", 20.0, false, tl!("Snap settings"));
                        egui::Popup::menu(&more).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                            ui.set_min_width(200.0);
                            let m = &mut v.measure;
                            ui.checkbox(&mut m.snaps.endpoints, tl!("Endpoints"));
                            ui.checkbox(&mut m.snaps.midpoints, tl!("Midpoints"));
                            ui.checkbox(&mut m.snaps.intersections, tl!("Intersections"));
                            ui.checkbox(&mut m.snaps.paths, tl!("Paths"));
                            ui.add(egui::Slider::new(&mut m.sensitivity, 1.0..=20.0).text(tl!("Sensitivity")));
                        });
                        let _ = ui
                            .toggle_value(&mut v.measure.snap_enabled, egui::RichText::new(tl!("Snap")).font(font.clone()))
                            .on_hover_text(tl!("Snap to ends, middles and crossings of the drawing"));
                    }
                    let _ = ui
                        .toggle_value(&mut app.comment_prefs.hover_tips, egui::RichText::new(tl!("Tips")).font(font.clone()))
                        .on_hover_text(tl!("Show the author and comment when pointing at a markup"));
                    let _ = ui
                        .toggle_value(&mut app.draft.ortho, egui::RichText::new(tl!("Ortho")).font(font.clone()))
                        .on_hover_text(tl!("Draw only horizontal and vertical lines (hold Shift for the other way)"));
                });
            });
        });
}
