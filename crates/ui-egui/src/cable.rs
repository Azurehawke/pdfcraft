//! RevuCraft's cable runs (Measure tab): a run is measured along its route, click by click, and
//! each one adds an allowance for the drop down the wall and the service loop at the rack (10'
//! to start; typed the way Draft takes lengths). The totals, by layer, give the quantity to order.

use pdfcraft_engine::Document;

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, icons};

/// One layer's cable runs: how many, and their length in the scale's unit (as written).
#[derive(Clone, Debug, PartialEq)]
pub struct Total {
    pub layer: String,
    pub runs: usize,
    pub length: String,
}

/// The cable runs in the document, totalled by layer (runs on no layer together), then all of them.
pub fn totals(doc: &Document) -> Vec<Total> {
    let Ok(listing) = doc.measurements() else { return Vec::new() };
    let layers = doc.layer_list();
    let layer_of = |page: usize, index: usize| -> String {
        doc.info
            .annotations
            .iter()
            .find(|a| a.page == page && a.index == index)
            .and_then(|a| a.layer)
            .and_then(|l| layers.iter().find(|x| (x.id.num, x.id.generation) == l))
            .map(|x| x.name.clone())
            .unwrap_or_else(|| tl!("No layer").to_string())
    };
    // (layer, runs, sum, a scale to write it with)
    let mut groups: Vec<(String, usize, f64, pdfcraft_engine::measure::Scale)> = Vec::new();
    for m in listing.measurements.iter().filter(|m| m.cable) {
        let layer = layer_of(m.page, m.index);
        match groups.iter_mut().find(|g| g.0 == layer) {
            Some(g) => {
                g.1 += 1;
                g.2 += m.reading.value;
            }
            None => groups.push((layer, 1, m.reading.value, m.scale.clone())),
        }
    }
    let mut out: Vec<Total> = groups
        .iter()
        .map(|(layer, runs, sum, scale)| Total { layer: layer.clone(), runs: *runs, length: scale.format_length(*sum).unwrap_or_default() })
        .collect();
    if groups.len() > 1
        && let Some((_, _, _, scale)) = groups.first()
    {
        let runs = groups.iter().map(|g| g.1).sum();
        let sum: f64 = groups.iter().map(|g| g.2).sum();
        out.push(Total { layer: tl!("All").to_string(), runs, length: scale.format_length(sum).unwrap_or_default() });
    }
    out
}

/// The Measure tab's Cable runs group: the tool, its allowance, and the totals.
pub fn ribbon_group(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(8.0);
    let x = ui.cursor().left();
    ui.painter().vline(x, ui.max_rect().y_range().shrink(6.0), egui::Stroke::new(1.0, t.divider));
    ui.add_space(8.0);
    let tool = QuickTool::Measure(crate::measure_ui::Tool::CableRun);
    ui.allocate_ui_with_layout(egui::vec2(0.0, ui.available_height()), egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let selected = app.quick_tool == tool;
            if icons::button(ui, "move-right", 34.0, selected, tl!("Cable run: click along the route, then Finish")).clicked() {
                if selected {
                    app.quick_tool = QuickTool::Select;
                } else {
                    app.execute("measure.cable");
                }
            }
            // The allowance, typed like a Draft length.
            let id = egui::Id::new("cable-allowance");
            let shown = format_inches(app.draft.cable_inches);
            let mut text = ui.data(|m| m.get_temp::<String>(id)).unwrap_or(shown);
            let r = ui
                .add(egui::TextEdit::singleline(&mut text).desired_width(56.0))
                .on_hover_text(tl!("Added to every run for the drop down the wall and the service loop (10' or 3m)"));
            if r.has_focus() {
                ui.data_mut(|m| m.insert_temp(id, text.clone()));
            }
            if r.lost_focus() {
                if let Some(inches) = crate::draft::parse_length(&text, app.draft.bare).filter(|v| v.is_finite() && (0.0..=1e6).contains(v)) {
                    app.draft.cable_inches = inches;
                }
                ui.data_mut(|m| m.remove::<String>(id));
            }
            let totals_button = icons::button(ui, "list", 28.0, false, tl!("Cable totals by layer"));
            egui::Popup::from_toggle_button_response(&totals_button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let list = app.active_ids().and_then(|(_, id)| app.session.get(id)).map(totals).unwrap_or_default();
                if list.is_empty() {
                    ui.label(tl!("No cable runs yet."));
                    return;
                }
                egui::Grid::new("cable-totals").num_columns(3).spacing([14.0, 4.0]).show(ui, |ui| {
                    ui.label(egui::RichText::new(tl!("Layer")).strong());
                    ui.label(egui::RichText::new(tl!("Runs")).strong());
                    ui.label(egui::RichText::new(tl!("Cable")).strong());
                    ui.end_row();
                    for row in &list {
                        ui.label(row.layer.as_str());
                        ui.label(row.runs.to_string());
                        ui.label(egui::RichText::new(row.length.as_str()).font(theme::semibold(12.5)));
                        ui.end_row();
                    }
                });
            });
        });
        ui.label(egui::RichText::new(tl!("Cable runs")).font(theme::regular(11.0)).color(t.text_faint));
    });
}

/// Inches written as feet and inches (`10'`, `7'6"`, `4"`), for the allowance field.
fn format_inches(inches: f64) -> String {
    if !inches.is_finite() || inches <= 0.0 {
        return "0".into();
    }
    let total = (inches * 8.0).round() / 8.0;
    let feet = (total / 12.0).floor();
    let rest = total - feet * 12.0;
    match (feet > 0.0, rest > 1e-9) {
        (true, true) => format!("{feet}'{}\"", trim(rest)),
        (true, false) => format!("{feet}'"),
        _ => format!("{}\"", trim(rest)),
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowances_read_back_as_typed() {
        assert_eq!(format_inches(120.0), "10'");
        assert_eq!(format_inches(90.0), "7'6\"");
        assert_eq!(format_inches(4.5), "4.5\"");
        assert_eq!(format_inches(f64::NAN), "0");
    }
}
