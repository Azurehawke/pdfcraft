//! RevuCraft's cable runs (Measure tab): a run is measured along its route, click by click, and
//! each one adds an allowance for the drop down the wall and the service loop at the rack (10'
//! to start; typed the way Draft takes lengths). The totals, by layer, give the quantity to order.
//!
//! As BIMCraft's structured cabling: each run can belong to a system (a network, a link, a
//! circuit) and be a cable type (Cat6, Cat6A, fibre), kept in your System and Cable type columns,
//! so the totals also come by system and by cable type.

use pdfcraft_engine::Document;

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, icons};

/// One group's cable runs: how many, and their length in the scale's unit (as written).
#[derive(Clone, Debug, PartialEq)]
pub struct Total {
    pub layer: String,
    pub runs: usize,
    pub length: String,
}

/// What the totals are grouped by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum By {
    #[default]
    Layer,
    System,
    CableType,
}

/// The name of your column that holds `english` (System, Cable type) in any case, or the name it
/// gets when it is added.
fn column_name(columns: &[crate::columns::MarkupColumn], english: &str) -> String {
    let shown = tl!(english);
    columns
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(shown) || c.name.eq_ignore_ascii_case(english))
        .map_or_else(|| shown.to_string(), |c| c.name.clone())
}

/// The fields a new cable run is given: its system and cable type, when chosen.
pub fn run_fields(columns: &[crate::columns::MarkupColumn], settings: &crate::draft::DraftSettings) -> Vec<(String, String)> {
    [("System", settings.cable_system.trim()), ("Cable type", settings.cable_type.trim())]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(col, v)| (column_name(columns, col), pdfcraft_engine::fields::clean_name(v)))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

/// Your System or Cable type column, added when it isn't there yet (so the Markups list shows it).
fn ensure_column(columns: &mut Vec<crate::columns::MarkupColumn>, english: &str, choices: &[&str]) {
    let shown = tl!(english);
    if columns.iter().any(|c| c.name.eq_ignore_ascii_case(shown) || c.name.eq_ignore_ascii_case(english))
        || columns.len() >= crate::columns::MAX_COLUMNS
    {
        return;
    }
    columns.push(crate::columns::MarkupColumn {
        name: shown.to_string(),
        choices: choices.iter().map(|c| c.to_string()).collect(),
        formula: String::new(),
    });
}

/// The cable runs in the document, totalled by layer (runs on no layer together), then all of them.
pub fn totals(doc: &Document) -> Vec<Total> {
    totals_by(doc, By::Layer, &[])
}

/// [`totals`] grouped by layer, system or cable type (`columns` names the latter two).
pub fn totals_by(doc: &Document, by: By, columns: &[crate::columns::MarkupColumn]) -> Vec<Total> {
    let Ok(listing) = doc.measurements() else { return Vec::new() };
    let fields = if by == By::Layer { Vec::new() } else { doc.all_annotation_fields() };
    let field_name = match by {
        By::Layer => String::new(),
        By::System => column_name(columns, "System"),
        By::CableType => column_name(columns, "Cable type"),
    };
    let field_of = |page: usize, index: usize| -> String {
        fields
            .iter()
            .find(|(p, i, _)| *p == page && *i == index)
            .and_then(|(_, _, f)| f.iter().find(|(n, _)| *n == field_name))
            .map(|(_, v)| v.clone())
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| tl!("(none)").to_string())
    };
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
        let layer = if by == By::Layer { layer_of(m.page, m.index) } else { field_of(m.page, m.index) };
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
            // The system and cable type new runs get (kept in your System and Cable type columns).
            // Sized outright: the ribbon group lays out at its content's width.
            let sys = ui
                .add_sized([78.0, 22.0], egui::TextEdit::singleline(&mut app.draft.cable_system).char_limit(40).hint_text(tl!("System")))
                .on_hover_text(tl!("The system new runs belong to: a network, a link or a circuit (Data, Voice, Cameras)"));
            if sys.lost_focus() && !app.draft.cable_system.trim().is_empty() {
                ensure_column(&mut app.markup_columns, "System", &[]);
            }
            let type_col = column_name(&app.markup_columns, "Cable type");
            let mut choices: Vec<String> = app.markup_columns.iter().find(|c| c.name == type_col).map(|c| c.choices.clone()).unwrap_or_default();
            if choices.is_empty() {
                choices = ["Cat6", "Cat6A", "Fiber", "Coax"].iter().map(|c| c.to_string()).collect();
            }
            let before = app.draft.cable_type.clone();
            let shown_type = if app.draft.cable_type.is_empty() { tl!("Cable type").to_string() } else { app.draft.cable_type.clone() };
            egui::ComboBox::from_id_salt("cable-type").selected_text(shown_type).width(76.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.draft.cable_type, String::new(), "—");
                for c in &choices {
                    ui.selectable_value(&mut app.draft.cable_type, c.clone(), c.as_str());
                }
            });
            if app.draft.cable_type != before && !app.draft.cable_type.is_empty() {
                ensure_column(&mut app.markup_columns, "Cable type", &["Cat6", "Cat6A", "Fiber", "Coax"]);
            }
            let totals_button = icons::button(ui, "list", 28.0, false, tl!("Cable totals by layer, system or cable type"));
            egui::Popup::from_toggle_button_response(&totals_button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let by_id = egui::Id::new("cable-totals-by");
                let mut by = ui.data(|m| m.get_temp::<By>(by_id)).unwrap_or_default();
                ui.horizontal(|ui| {
                    for (b, label) in [(By::Layer, tl!("By layer")), (By::System, tl!("By system")), (By::CableType, tl!("By cable type"))] {
                        if ui.selectable_label(by == b, label).clicked() {
                            by = b;
                        }
                    }
                });
                ui.data_mut(|m| m.insert_temp(by_id, by));
                let list =
                    app.active_ids().and_then(|(_, id)| app.session.get(id)).map(|d| totals_by(d, by, &app.markup_columns)).unwrap_or_default();
                if list.is_empty() {
                    ui.label(tl!("No cable runs yet."));
                    return;
                }
                let heading = match by {
                    By::Layer => tl!("Layer"),
                    By::System => tl!("System"),
                    By::CableType => tl!("Cable type"),
                };
                egui::Grid::new("cable-totals").num_columns(3).spacing([14.0, 4.0]).show(ui, |ui| {
                    ui.label(egui::RichText::new(heading).strong());
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
    fn new_runs_carry_their_system_and_cable_type_in_your_columns() {
        let mut settings = crate::draft::DraftSettings::default();
        let columns = vec![crate::columns::MarkupColumn { name: "system".into(), ..Default::default() }];
        assert!(run_fields(&columns, &settings).is_empty(), "nothing chosen, nothing written");
        settings.cable_system = " Data ".into();
        settings.cable_type = "Cat6A".into();
        // The System column as you named it; Cable type as it will be added.
        assert_eq!(
            run_fields(&columns, &settings),
            vec![("system".to_string(), "Data".to_string()), ("Cable type".to_string(), "Cat6A".to_string())]
        );
        let mut cols = columns.clone();
        ensure_column(&mut cols, "System", &[]);
        assert_eq!(cols.len(), 1, "already there");
        ensure_column(&mut cols, "Cable type", &["Cat6"]);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[1].choices, ["Cat6"]);
    }

    #[test]
    fn allowances_read_back_as_typed() {
        assert_eq!(format_inches(120.0), "10'");
        assert_eq!(format_inches(90.0), "7'6\"");
        assert_eq!(format_inches(4.5), "4.5\"");
        assert_eq!(format_inches(f64::NAN), "0");
    }
}
