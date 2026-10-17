//! RevuCraft's custom columns: your own fields on markups (drop ID, port, cable type, progress),
//! each free text or a pick list. The columns are yours, kept with the app (like a Revu profile);
//! the values are kept in each markup in the PDF (`pdfcraft_engine::fields`). They show in the
//! Properties box, the Markups list, its spreadsheet export and the summary report.

use serde_json::{Value, json};

/// Columns kept at most, choices per column at most, and the longest choice (characters).
pub const MAX_COLUMNS: usize = 24;
const MAX_CHOICES: usize = 32;

/// One of your columns: its name, and its choices (none: free text).
#[derive(Clone, Debug, PartialEq)]
pub struct MarkupColumn {
    pub name: String,
    pub choices: Vec<String>,
}

/// The columns a new install starts with, for network jobs.
pub fn defaults() -> Vec<MarkupColumn> {
    let list = |items: &[&'static str]| items.iter().map(|s| tl!(s).to_string()).collect::<Vec<_>>();
    vec![
        MarkupColumn { name: tl!("Drop ID").to_string(), choices: Vec::new() },
        MarkupColumn { name: tl!("Port").to_string(), choices: Vec::new() },
        MarkupColumn { name: tl!("Cable type").to_string(), choices: list(&["Cat6", "Cat6A", "Fiber", "Coax"]) },
        MarkupColumn { name: tl!("Progress").to_string(), choices: list(&["Not started", "Pulled", "Terminated", "Tested"]) },
    ]
}

fn clean_choice(s: &str) -> String {
    pdfcraft_engine::fields::clean_name(s)
}

/// Choices typed as one line, separated by commas.
pub fn parse_choices(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in s.split(',').map(clean_choice).filter(|c| !c.is_empty()) {
        if !out.contains(&c) && out.len() < MAX_CHOICES {
            out.push(c);
        }
    }
    out
}

pub fn to_json(cols: &[MarkupColumn]) -> Value {
    Value::Array(cols.iter().map(|c| json!({ "name": c.name, "choices": c.choices })).collect())
}

/// Read back [`to_json`]; anything unreadable is skipped, names repeat at most once.
pub fn from_json(v: &Value) -> Option<Vec<MarkupColumn>> {
    let list = v.as_array()?;
    let mut out: Vec<MarkupColumn> = Vec::new();
    for c in list.iter().take(MAX_COLUMNS) {
        let Some(name) = c["name"].as_str().map(pdfcraft_engine::fields::clean_name).filter(|n| !n.is_empty()) else { continue };
        if out.iter().any(|x| x.name == name) {
            continue;
        }
        let choices = c["choices"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default();
        out.push(MarkupColumn { name, choices: parse_choices(&choices) });
    }
    Some(out)
}

/// The column editor (Markups list ▸ Columns): rename, set choices, add and remove columns.
pub fn editor(ui: &mut egui::Ui, cols: &mut Vec<MarkupColumn>) {
    ui.label(egui::RichText::new(tl!("Your columns")).strong());
    ui.label(egui::RichText::new(tl!("Leave the choices empty for a column you type into.")).small().weak());
    let mut remove: Option<usize> = None;
    egui::Grid::new("markup-columns").num_columns(3).spacing([6.0, 4.0]).show(ui, |ui| {
        ui.label(tl!("Name"));
        ui.label(tl!("Choices (comma-separated)"));
        ui.end_row();
        for (i, c) in cols.iter_mut().enumerate() {
            let mut name = c.name.clone();
            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0)).changed() {
                let cleaned = pdfcraft_engine::fields::clean_name(&name);
                if !cleaned.is_empty() {
                    c.name = cleaned;
                }
            }
            let id = ui.id().with(("choices", i));
            let mut text = ui.data(|m| m.get_temp::<String>(id)).unwrap_or_else(|| c.choices.join(", "));
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(180.0));
            if r.changed() {
                ui.data_mut(|m| m.insert_temp(id, text.clone()));
            }
            if r.lost_focus() {
                c.choices = parse_choices(&text);
                ui.data_mut(|m| m.remove::<String>(id));
            }
            if crate::icons::button(ui, "trash-2", 22.0, false, tl!("Remove this column")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove
        && i < cols.len()
    {
        cols.remove(i);
    }
    if cols.len() < MAX_COLUMNS && ui.button(tl!("Add a column")).clicked() {
        let mut n = cols.len() + 1;
        let mut name = crate::i18n::fmt(tl!("Column {n}"), &[("n", &n.to_string())]);
        while cols.iter().any(|c| c.name == name) {
            n += 1;
            name = crate::i18n::fmt(tl!("Column {n}"), &[("n", &n.to_string())]);
        }
        cols.push(MarkupColumn { name, choices: Vec::new() });
    }
    // Two columns can't share a name (their values would mix).
    let mut seen: Vec<String> = Vec::new();
    for c in cols.iter_mut() {
        while seen.contains(&c.name) {
            c.name.push('*');
        }
        seen.push(c.name.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_round_trip_and_bad_ones_are_skipped() {
        let cols = vec![
            MarkupColumn { name: "Drop ID".into(), choices: vec![] },
            MarkupColumn { name: "Progress".into(), choices: vec!["Pulled".into(), "Tested".into()] },
        ];
        assert_eq!(from_json(&to_json(&cols)), Some(cols));
        let junk = json!([{ "name": "" }, { "name": "A" }, { "name": "A" }, 7, { "name": "B", "choices": [" x ", "x", 3, "y"] }]);
        let back = from_json(&junk).unwrap();
        assert_eq!(back.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(back[1].choices, ["x", "y"]);
        assert_eq!(parse_choices("Cat6, Cat6A,,Cat6"), ["Cat6", "Cat6A"]);
    }
}
