//! RevuCraft's Markups list (in the dock): every markup and measurement of the document in one
//! table (type, label, layer, page, author, status, value, date), sorted by any column, filtered
//! by text, type and layer, a click away from the page, and exported as a spreadsheet.

use egui::{Align, Layout, Sense};
use egui_extras::{Column, TableBuilder};
use pdfcraft_engine::{DocId, Document};

use crate::theme::Tokens;
use crate::{PdfCraftApp, icons};

/// Rows above this are not listed (the count says so).
const MAX_ROWS: usize = 20_000;

/// The columns, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Col {
    Type,
    Label,
    Layer,
    #[default]
    Page,
    Author,
    Status,
    Value,
    Date,
    /// One of your own columns (its position in your list).
    Custom(usize),
}

impl Col {
    const ALL: [Col; 8] = [Col::Type, Col::Label, Col::Layer, Col::Page, Col::Author, Col::Status, Col::Value, Col::Date];

    fn title(self) -> &'static str {
        match self {
            Col::Type => "Type",
            Col::Label => "Label",
            Col::Layer => "Layer",
            Col::Page => "Page",
            Col::Author => "Author",
            Col::Status => "Status",
            Col::Value => "Value",
            Col::Date => "Date",
            Col::Custom(_) => "",
        }
    }

    fn width(self) -> f32 {
        match self {
            Col::Label => 140.0,
            Col::Page => 44.0,
            Col::Date => 82.0,
            Col::Value => 80.0,
            _ => 90.0,
        }
    }
}

/// One markup as the list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub page: usize,
    pub index: usize,
    pub rect: [f32; 4],
    pub kind: String,
    pub label: String,
    pub layer: String,
    pub author: String,
    pub status: String,
    pub value: String,
    /// The value as a number, for sorting.
    pub amount: Option<f64>,
    pub date: String,
    /// Your own columns' values (name, value).
    pub fields: Vec<(String, String)>,
}

impl Row {
    fn field(&self, name: &str) -> String {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_default()
    }

    fn text(&self, col: Col, custom: &[String]) -> String {
        match col {
            Col::Custom(i) => custom.get(i).map(|n| self.field(n)).unwrap_or_default(),
            Col::Type => self.kind.clone(),
            Col::Label => self.label.clone(),
            Col::Layer => self.layer.clone(),
            Col::Page => (self.page + 1).to_string(),
            Col::Author => self.author.clone(),
            Col::Status => self.status.clone(),
            Col::Value => self.value.clone(),
            Col::Date => self.date.clone(),
        }
    }
}

/// The list's choices and its rows, per document.
#[derive(Clone, Debug, Default)]
pub struct MarkupsState {
    pub sort: Col,
    pub descending: bool,
    pub query: String,
    pub kind: Option<String>,
    pub layer: Option<String>,
    rows: Option<(DocId, u64, Vec<Row>)>,
}

/// `D:20261015…` as `2026-10-15` (anything else as written, cut short).
fn pdf_date(d: &str) -> String {
    let digits: String = d.trim_start_matches("D:").chars().take(8).collect();
    match (digits.get(0..4), digits.get(4..6), digits.get(6..8)) {
        (Some(y), Some(m), Some(day)) if digits.chars().all(|c| c.is_ascii_digit()) => format!("{y}-{m}-{day}"),
        _ => d.chars().take(16).collect(),
    }
}

fn first_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default();
    let mut out: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        out.push('…');
    }
    out
}

/// The document's markups as rows: top-level comments only (not replies, status changes or
/// pop-ups), with the measurement value and the latest review status.
pub fn rows(doc: &Document) -> Vec<Row> {
    let info = &doc.info;
    let layers = doc.layer_list();
    let layer_name = |l: Option<(u32, u16)>| -> String {
        l.and_then(|l| layers.iter().find(|x| (x.id.num, x.id.generation) == l)).map(|x| x.name.clone()).unwrap_or_default()
    };
    let measured = doc.measurements().map(|l| l.measurements).unwrap_or_default();
    let all_fields = doc.all_annotation_fields();
    let mut out = Vec::new();
    for a in info.annotations.iter().filter(|a| a.in_reply_to.is_none() && a.state.is_none() && a.subtype != "Popup") {
        if out.len() >= MAX_ROWS {
            break;
        }
        let measurement = measured.iter().find(|m| m.page == a.page && m.index == a.index);
        let kind = if let Some(s) = a.symbol.as_deref() {
            pdfcraft_engine::symbols::display_name(s)
        } else if a.photo {
            tl!("Photo").to_string()
        } else if let Some(m) = measurement {
            match m.reading.kind {
                pdfcraft_engine::measure::Kind::Distance => tl!("Distance"),
                pdfcraft_engine::measure::Kind::Perimeter => tl!("Perimeter"),
                pdfcraft_engine::measure::Kind::Area => tl!("Area"),
            }
            .to_string()
        } else {
            tl!(crate::comments_panel::kind_label(a)).to_string()
        };
        let label = match measurement {
            Some(m) if !m.label.is_empty() => m.label.clone(),
            Some(_) => String::new(),
            None => first_line(a.contents.as_deref().unwrap_or_default()),
        };
        // The latest review status set by a reply (checkmarks are private and not shown).
        let status = a
            .name
            .as_deref()
            .and_then(|name| {
                info.annotations
                    .iter()
                    .filter(|r| r.in_reply_to.as_deref() == Some(name) && !r.is_mark())
                    .filter_map(|r| r.state.as_deref())
                    .next_back()
                    .filter(|s| *s != "None")
            })
            .map(|s| tl!(s).to_string())
            .unwrap_or_default();
        out.push(Row {
            page: a.page,
            index: a.index,
            rect: a.rect,
            kind,
            label,
            layer: layer_name(a.layer),
            author: a.author.clone().unwrap_or_default(),
            status,
            value: measurement.map(|m| m.reading.label.clone()).unwrap_or_default(),
            amount: measurement.map(|m| m.reading.value),
            date: a.modified.as_deref().map(pdf_date).unwrap_or_default(),
            fields: all_fields.iter().find(|(p, i, _)| *p == a.page && *i == a.index).map(|(_, _, f)| f.clone()).unwrap_or_default(),
        });
    }
    out
}

/// The rows that pass the filters, in the chosen order.
pub fn visible(rows: &[Row], state: &MarkupsState, custom: &[String]) -> Vec<Row> {
    let cols = all_columns(custom);
    let q = state.query.trim().to_lowercase();
    let mut out: Vec<Row> = rows
        .iter()
        .filter(|r| state.kind.as_ref().is_none_or(|k| &r.kind == k))
        .filter(|r| state.layer.as_ref().is_none_or(|l| &r.layer == l))
        .filter(|r| q.is_empty() || cols.iter().any(|c| r.text(*c, custom).to_lowercase().contains(&q)))
        .cloned()
        .collect();
    let col = state.sort;
    out.sort_by(|a, b| {
        let order = match col {
            Col::Page => (a.page, a.index).cmp(&(b.page, b.index)),
            Col::Value => a.amount.partial_cmp(&b.amount).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.value.cmp(&b.value)),
            other => a
                .text(other, custom)
                .to_lowercase()
                .cmp(&b.text(other, custom).to_lowercase())
                .then_with(|| (a.page, a.index).cmp(&(b.page, b.index))),
        };
        if state.descending { order.reverse() } else { order }
    });
    out
}

/// A spreadsheet cell: quoted when it has to be, and never read as a formula.
fn cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{text}") } else { text.to_string() };
    if text.contains([',', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text }
}

/// The built-in columns, then yours.
fn all_columns(custom: &[String]) -> Vec<Col> {
    Col::ALL.into_iter().chain((0..custom.len()).map(Col::Custom)).collect()
}

/// A column's heading (translated for the built-in ones).
fn heading(c: Col, custom: &[String]) -> String {
    match c {
        Col::Custom(i) => custom.get(i).cloned().unwrap_or_default(),
        other => tl!(other.title()).to_string(),
    }
}

/// The rows as CSV, with a header.
pub fn csv(rows: &[Row], custom: &[String]) -> String {
    let cols = all_columns(custom);
    let mut out = cols
        .iter()
        .map(|c| {
            cell(&match c {
                Col::Custom(i) => custom.get(*i).cloned().unwrap_or_default(),
                other => other.title().to_string(),
            })
        })
        .collect::<Vec<_>>()
        .join(",");
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&cols.iter().map(|c| cell(&r.text(*c, custom))).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

/// Draw the Markups list for the active document.
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((index, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let generation = doc.edit_generation();
    let state = &mut app.markups;
    if state.rows.as_ref().is_none_or(|(d, g, _)| *d != id || *g != generation) {
        state.rows = Some((id, generation, rows(doc)));
    }
    let all = state.rows.as_ref().map(|(_, _, r)| r.clone()).unwrap_or_default();
    let mut kinds: Vec<String> = all.iter().map(|r| r.kind.clone()).collect();
    kinds.sort();
    kinds.dedup();
    let mut layer_names: Vec<String> = all.iter().map(|r| r.layer.clone()).filter(|l| !l.is_empty()).collect();
    layer_names.sort();
    layer_names.dedup();
    let custom: Vec<String> = app.markup_columns.iter().map(|c| c.name.clone()).collect();
    let cols = all_columns(&custom);
    let mut export = false;
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(tl!("Search markups")).desired_width(130.0));
        egui::ComboBox::from_id_salt("markups-kind")
            .selected_text(state.kind.clone().unwrap_or_else(|| tl!("All types").to_string()))
            .width(100.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.kind, None, tl!("All types"));
                for k in &kinds {
                    ui.selectable_value(&mut state.kind, Some(k.clone()), k.as_str());
                }
            });
        if !layer_names.is_empty() {
            egui::ComboBox::from_id_salt("markups-layer")
                .selected_text(state.layer.clone().unwrap_or_else(|| tl!("All layers").to_string()))
                .width(100.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.layer, None, tl!("All layers"));
                    for l in &layer_names {
                        ui.selectable_value(&mut state.layer, Some(l.clone()), l.as_str());
                    }
                });
        }
        if icons::button(ui, "file-spreadsheet", 26.0, false, tl!("Export the list as a spreadsheet (CSV)")).clicked() {
            export = true;
        }
        let r = icons::button(ui, "columns-3", 26.0, false, tl!("Your columns: drop ID, progress and others"));
        egui::Popup::from_toggle_button_response(&r)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| crate::columns::editor(ui, &mut app.markup_columns));
    });
    let shown = visible(&all, state, &custom);
    let count = if shown.len() == all.len() {
        crate::i18n::fmt(tl!("Markups: {n}"), &[("n", &all.len().to_string())])
    } else {
        crate::i18n::fmt(tl!("Markups: {k} of {n}"), &[("k", &shown.len().to_string()), ("n", &all.len().to_string())])
    };
    ui.label(egui::RichText::new(count).small().color(t.text_muted));
    let selected = app.views.get(index).and_then(|v| v.comments.selected);
    let mut clicked: Option<Row> = None;
    let mut sort_by: Option<Col> = None;
    egui::ScrollArea::horizontal().id_salt("markups-scroll").auto_shrink([false, true]).show(ui, |ui| {
        let mut builder = TableBuilder::new(ui)
            .id_salt("markups-table")
            .striped(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .cell_layout(Layout::left_to_right(Align::Center));
        for c in cols.iter().copied() {
            builder = builder.column(Column::initial(c.width()).at_least(36.0).resizable(true).clip(true));
        }
        builder
            .header(22.0, |mut header| {
                for c in cols.iter().copied() {
                    header.col(|ui| {
                        let arrow = if state.sort == c { if state.descending { " ↓" } else { " ↑" } } else { "" };
                        let text = format!("{}{arrow}", heading(c, &custom));
                        if ui.add(egui::Label::new(egui::RichText::new(text).strong()).sense(Sense::click())).clicked() {
                            sort_by = Some(c);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(22.0, shown.len(), |mut row| {
                    let Some(r) = shown.get(row.index()) else { return };
                    row.set_selected(selected == Some((r.page, r.index)));
                    for c in cols.iter().copied() {
                        row.col(|ui| {
                            ui.add(egui::Label::new(r.text(c, &custom)).truncate().selectable(false));
                        });
                    }
                    if row.response().clicked() {
                        clicked = Some(r.clone());
                    }
                });
            });
    });
    if all.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tl!("No markups yet. Everything you draw, measure or place is listed here.")).color(t.text_muted));
    }
    if let Some(c) = sort_by {
        if state.sort == c {
            state.descending = !state.descending;
        } else {
            state.sort = c;
            state.descending = false;
        }
    }
    if let Some(r) = clicked
        && let Some(v) = app.views.get_mut(index)
    {
        v.comments.selected = Some((r.page, r.index));
        v.go_to_page(r.page);
        v.flash = Some((r.page, r.rect, 0.0));
    }
    if export {
        export_csv(app, csv(&shown, &custom));
    }
}

/// Save the list as a spreadsheet.
fn export_csv(app: &mut PdfCraftApp, csv: String) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let write =
            move |app: &mut PdfCraftApp, path: std::path::PathBuf| match crate::editing::write_atomically(&path.to_string_lossy(), csv.as_bytes()) {
                Ok(()) => app.notify(tl!("Saved the markups list.")),
                Err(e) => app.notify(e.to_string()),
            };
        match app.export_dir_override.as_ref().map(|d| std::path::PathBuf::from(d).join("markups.csv")) {
            Some(path) => write(app, path),
            None => {
                let dialog = crate::dialog::AsyncFileDialog::new().set_file_name("markups.csv").add_filter("CSV", &["csv"]);
                app.ask_one(crate::pickers::Ask::Save(dialog), None, write);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Err(e) = crate::editing::download("markups.csv", csv.as_bytes()) {
        app.notify(&e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(page: usize, kind: &str, label: &str, value: Option<f64>) -> Row {
        Row {
            page,
            index: 0,
            rect: [0.0; 4],
            kind: kind.into(),
            label: label.into(),
            layer: String::new(),
            author: "Steve".into(),
            status: String::new(),
            value: value.map(|v| format!("{v}'")).unwrap_or_default(),
            amount: value,
            date: String::new(),
            fields: vec![("Progress".into(), "Pulled".into())],
        }
    }

    #[test]
    fn sorting_filtering_and_csv() {
        let rows = vec![row(2, "Distance", "Wall", Some(12.0)), row(0, "Data jack", "PC", None), row(1, "Distance", "Run", Some(3.5))];
        let mut s = MarkupsState::default();
        let none: [String; 0] = [];
        assert_eq!(visible(&rows, &s, &none).iter().map(|r| r.page).collect::<Vec<_>>(), [0, 1, 2]);
        s.sort = Col::Value;
        s.descending = true;
        assert_eq!(visible(&rows, &s, &none).first().map(|r| r.label.as_str()), Some("Wall"));
        s.kind = Some("Distance".into());
        assert_eq!(visible(&rows, &s, &none).len(), 2);
        s.kind = None;
        s.query = "pc".into();
        assert_eq!(visible(&rows, &s, &none).len(), 1);
        // Your own columns are searched and exported.
        s.query = "pulled".into();
        assert_eq!(visible(&rows, &s, &["Progress".to_string()]).len(), 3);
        assert!(csv(&rows, &["Progress".to_string()]).starts_with("Type,Label,Layer,Page,Author,Status,Value,Date,Progress\r\n"));
        let text = csv(&[row(0, "Note", "=SUM(A1), \"x\"", None)], &none);
        assert!(text.starts_with("Type,Label,Layer,Page,Author,Status,Value,Date\r\n"));
        assert!(text.contains("\"'=SUM(A1), \"\"x\"\"\""), "{text}");
        assert_eq!(pdf_date("D:20261015093000-07'00'"), "2026-10-15");
        assert_eq!(pdf_date("yesterday"), "yesterday");
    }
}
