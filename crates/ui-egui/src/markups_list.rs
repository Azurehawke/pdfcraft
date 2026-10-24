//! RevuCraft's Markups list (in the dock): every markup and measurement of the document in one
//! table (type, label, layer, page, author, status, value, date), sorted by any column, filtered
//! by text, type and layer, a click away from the page, and exported as a spreadsheet.
//!
//! As BIMCraft's (and Revit's) schedules: your calculated columns are worked out for each row
//! ([`crate::formula`]), rules filter the rows (`Value` greater than 50, `System` equals Data),
//! and the rows can be grouped by a column, each group with its count and totals, the totals of
//! everything shown under the list.

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
    /// A measurement's name in a workbook's Takeoff sheet.
    Name,
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
    const ALL: [Col; 9] = [Col::Type, Col::Label, Col::Name, Col::Layer, Col::Page, Col::Author, Col::Status, Col::Value, Col::Date];

    fn title(self) -> &'static str {
        match self {
            Col::Type => "Type",
            Col::Label => "Label",
            Col::Name => "Name",
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
            Col::Name => 110.0,
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
    /// A measurement's name (empty for other markups).
    pub name: String,
    pub layer: String,
    pub author: String,
    pub status: String,
    pub value: String,
    /// The value as a number, for sorting and totals, and its unit (`ft`, `sq ft`).
    pub amount: Option<f64>,
    pub unit: String,
    pub date: String,
    /// Your own columns' values (name, value).
    pub fields: Vec<(String, String)>,
    /// Drafted linework (Draft tools, or on the Drafting layer).
    pub drafting: bool,
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
            Col::Name => self.name.clone(),
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
    /// Drafted linework listed too (off: only markups, unless a layer is chosen).
    pub drafting: bool,
    /// Rules every row shown holds to.
    pub rules: Vec<Rule>,
    /// The column the rows are grouped by.
    pub group: Option<Col>,
    rows: Option<(DocId, u64, Vec<Row>)>,
}

/// How a filter rule compares a row's value with its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Op {
    #[default]
    Contains,
    Equals,
    Greater,
    Less,
    HasValue,
    NoValue,
}

impl Op {
    const ALL: [Op; 6] = [Op::Contains, Op::Equals, Op::Greater, Op::Less, Op::HasValue, Op::NoValue];

    fn label(self) -> &'static str {
        match self {
            Op::Contains => "contains",
            Op::Equals => "equals",
            Op::Greater => "is greater than",
            Op::Less => "is less than",
            Op::HasValue => "has a value",
            Op::NoValue => "has no value",
        }
    }

    fn takes_value(self) -> bool {
        !matches!(self, Op::HasValue | Op::NoValue)
    }
}

/// A filter rule: a column, how to compare, and the value to compare with.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Rule {
    pub col: Col,
    pub op: Op,
    pub value: String,
}

/// Rules kept at most.
const MAX_RULES: usize = 8;

impl Rule {
    /// Whether `r` holds to the rule. Greater and less compare numbers (the value's own for
    /// Value, the number a column's text starts with otherwise); a row without one doesn't hold.
    pub fn matches(&self, r: &Row, custom: &[String]) -> bool {
        let text = r.text(self.col, custom);
        let number = || if self.col == Col::Value { r.amount } else { crate::formula::leading_number(&text) };
        match self.op {
            Op::Contains => text.to_lowercase().contains(&self.value.trim().to_lowercase()),
            Op::Equals => text.trim().eq_ignore_ascii_case(self.value.trim()),
            Op::Greater => matches!((number(), crate::formula::leading_number(&self.value)), (Some(a), Some(b)) if a > b),
            Op::Less => matches!((number(), crate::formula::leading_number(&self.value)), (Some(a), Some(b)) if a < b),
            Op::HasValue => !text.trim().is_empty(),
            Op::NoValue => text.trim().is_empty(),
        }
    }
}

/// Fill in your calculated columns for each row: `formulas` are (name, formula), worked out in
/// order, so one can use those before it. A formula that can't be worked out shows why.
pub fn calculate(rows: &mut [Row], formulas: &[(String, String)]) {
    for r in rows.iter_mut() {
        for (name, formula) in formulas {
            let value = {
                let row: &Row = r;
                let value_of = |n: &str| -> Option<f64> {
                    match n.to_lowercase().as_str() {
                        "value" => row.amount,
                        "count" => Some(1.0),
                        "page" => Some((row.page + 1) as f64),
                        lower => row.fields.iter().find(|(f, _)| f.to_lowercase() == lower).and_then(|(_, v)| crate::formula::leading_number(v)),
                    }
                };
                match crate::formula::evaluate(formula, value_of) {
                    Ok(v) => crate::formula::show(v),
                    Err(e) => format!("⚠ {e}"),
                }
            };
            match r.fields.iter_mut().find(|(f, _)| f == name) {
                Some(slot) => slot.1 = value,
                None => r.fields.push((name.clone(), value)),
            }
        }
    }
}

/// The count and totals of some rows: values summed per unit, and each calculated column summed.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Totals {
    pub count: usize,
    pub values: Vec<(String, f64)>,
    pub calculated: Vec<(String, f64)>,
}

pub fn totals(rows: &[&Row], calculated: &[String]) -> Totals {
    let mut t = Totals { count: rows.len(), ..Totals::default() };
    for r in rows {
        if let Some(a) = r.amount.filter(|a| a.is_finite()) {
            match t.values.iter_mut().find(|(u, _)| *u == r.unit) {
                Some(slot) => slot.1 += a,
                None => t.values.push((r.unit.clone(), a)),
            }
        }
    }
    for name in calculated {
        let sum: f64 =
            rows.iter().filter_map(|r| r.fields.iter().find(|(f, _)| f == name).and_then(|(_, v)| crate::formula::leading_number(v))).sum();
        t.calculated.push((name.clone(), sum));
    }
    t
}

impl Totals {
    /// The values as one line: `1,234.5 ft  ·  56 sq ft`.
    fn values_text(&self) -> String {
        self.values
            .iter()
            .map(|(u, v)| if u.is_empty() { crate::formula::show(*v) } else { format!("{} {u}", crate::formula::show(*v)) })
            .collect::<Vec<_>>()
            .join("  ·  ")
    }

    fn calculated_of(&self, name: &str) -> Option<String> {
        self.calculated.iter().find(|(n, _)| n == name).map(|(_, v)| crate::formula::show(*v))
    }
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
        } else if measurement.is_some_and(|m| m.cable) {
            tl!("Cable run").to_string()
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
            name: measurement.map(|m| m.name.clone()).unwrap_or_default(),
            layer: layer_name(a.layer),
            author: a.author.clone().unwrap_or_default(),
            status,
            value: measurement.map(|m| m.reading.label.clone()).unwrap_or_default(),
            amount: measurement.map(|m| m.reading.value),
            unit: measurement.map(|m| m.reading.unit.clone()).unwrap_or_default(),
            date: a.modified.as_deref().map(pdf_date).unwrap_or_default(),
            fields: all_fields.iter().find(|(p, i, _)| *p == a.page && *i == a.index).map(|(_, _, f)| f.clone()).unwrap_or_default(),
            drafting: a.drafting,
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
        .filter(|r| !r.drafting || state.drafting || state.layer.is_some())
        .filter(|r| state.rules.iter().all(|rule| rule.matches(r, custom)))
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
            // Your columns of numbers (and calculated ones) sort as numbers.
            other => match (crate::formula::leading_number(&a.text(other, custom)), crate::formula::leading_number(&b.text(other, custom))) {
                (Some(x), Some(y)) if matches!(other, Col::Custom(_)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                _ => a.text(other, custom).to_lowercase().cmp(&b.text(other, custom).to_lowercase()),
            }
            .then_with(|| (a.page, a.index).cmp(&(b.page, b.index))),
        };
        if state.descending { order.reverse() } else { order }
    });
    // Grouped: the groups in order of their name, the chosen order within each (a stable sort).
    if let Some(g) = state.group {
        out.sort_by(|a, b| a.text(g, custom).to_lowercase().cmp(&b.text(g, custom).to_lowercase()));
    }
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
    let mut all = state.rows.as_ref().map(|(_, _, r)| r.clone()).unwrap_or_default();
    let formulas: Vec<(String, String)> =
        app.markup_columns.iter().filter(|c| !c.formula.trim().is_empty()).map(|c| (c.name.clone(), c.formula.clone())).collect();
    calculate(&mut all, &formulas);
    let calculated: Vec<String> = formulas.iter().map(|(n, _)| n.clone()).collect();
    let mut kinds: Vec<String> = all.iter().map(|r| r.kind.clone()).collect();
    kinds.sort();
    kinds.dedup();
    let mut layer_names: Vec<String> = all.iter().map(|r| r.layer.clone()).filter(|l| !l.is_empty()).collect();
    layer_names.sort();
    layer_names.dedup();
    let custom: Vec<String> = app.markup_columns.iter().map(|c| c.name.clone()).collect();
    let cols = all_columns(&custom);
    let mut export = false;
    let mut report = false;
    let mut send = false;
    // Wraps in a narrow dock, so the buttons stay in view.
    ui.horizontal_wrapped(|ui| {
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
        if all.iter().any(|r| r.drafting) {
            ui.add(crate::widgets::checkbox(&mut state.drafting, tl!("Drafting"))).on_hover_text(tl!("List drafted linework too"));
        }
        if icons::button(ui, "file-spreadsheet", 26.0, false, tl!("Export the list as a spreadsheet (CSV)")).clicked() {
            export = true;
        }
        if icons::button(ui, "file-text", 26.0, false, tl!("Summary report (PDF): the markups shown, with photos, by layer")).clicked() {
            report = true;
        }
        if icons::button(ui, "send", 26.0, false, tl!("Send quantities to a workbook's Takeoff sheet, each a named cell")).clicked() {
            send = true;
        }
        // Filter rules and grouping, as a schedule's.
        let f = icons::button(ui, "filter", 26.0, !state.rules.is_empty(), tl!("Filter rules"));
        egui::Popup::from_toggle_button_response(&f).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(320.0);
            ui.label(egui::RichText::new(tl!("Show the markups where")).strong());
            let mut remove = None;
            for (i, rule) in state.rules.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("rule-col", i)).selected_text(heading(rule.col, &custom)).width(100.0).show_ui(ui, |ui| {
                        for c in cols.iter().copied() {
                            ui.selectable_value(&mut rule.col, c, heading(c, &custom));
                        }
                    });
                    egui::ComboBox::from_id_salt(("rule-op", i)).selected_text(tl!(rule.op.label())).width(110.0).show_ui(ui, |ui| {
                        for op in Op::ALL {
                            ui.selectable_value(&mut rule.op, op, tl!(op.label()));
                        }
                    });
                    if rule.op.takes_value() {
                        ui.add(egui::TextEdit::singleline(&mut rule.value).desired_width(80.0).char_limit(80));
                    }
                    if icons::button(ui, "x", 20.0, false, tl!("Remove this rule")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove
                && i < state.rules.len()
            {
                state.rules.remove(i);
            }
            if state.rules.len() < MAX_RULES && ui.button(tl!("Add a rule")).clicked() {
                state.rules.push(Rule::default());
            }
        });
        let group_text =
            state.group.map_or_else(|| tl!("No grouping").to_string(), |g| crate::i18n::fmt(tl!("By {column}"), &[("column", &heading(g, &custom))]));
        egui::ComboBox::from_id_salt("markups-group").selected_text(group_text).width(110.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.group, None, tl!("No grouping"));
            for c in cols.iter().copied().filter(|c| *c != Col::Date) {
                ui.selectable_value(&mut state.group, Some(c), heading(c, &custom));
            }
        });
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
    // The table's lines: each group's heading (with its count and totals) before its rows.
    enum Line {
        Group(String, Totals),
        Row(usize),
    }
    let mut lines: Vec<Line> = Vec::with_capacity(shown.len());
    match state.group {
        Some(g) => {
            let mut k = 0;
            while k < shown.len() {
                let key = shown.get(k).map(|r| r.text(g, &custom)).unwrap_or_default();
                let end = shown.iter().skip(k).position(|r| r.text(g, &custom).to_lowercase() != key.to_lowercase()).map_or(shown.len(), |n| k + n);
                let members: Vec<&Row> = shown.get(k..end).map(|s| s.iter().collect()).unwrap_or_default();
                let key = if key.is_empty() { tl!("(none)").to_string() } else { key };
                lines.push(Line::Group(key, totals(&members, &calculated)));
                lines.extend((k..end).map(Line::Row));
                k = end.max(k + 1);
            }
        }
        None => lines.extend((0..shown.len()).map(Line::Row)),
    }
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
                body.rows(22.0, lines.len(), |mut row| {
                    let r = match lines.get(row.index()) {
                        Some(Line::Row(i)) => match shown.get(*i) {
                            Some(r) => r,
                            None => return,
                        },
                        Some(Line::Group(key, tot)) => {
                            for (n, c) in cols.iter().copied().enumerate() {
                                row.col(|ui| {
                                    let text = match c {
                                        _ if n == 0 => format!("{key}  ({})", tot.count),
                                        Col::Value => tot.values_text(),
                                        Col::Custom(i) => custom.get(i).and_then(|name| tot.calculated_of(name)).unwrap_or_default(),
                                        _ => String::new(),
                                    };
                                    ui.add(egui::Label::new(egui::RichText::new(text).strong()).truncate().selectable(false));
                                });
                            }
                            return;
                        }
                        None => return,
                    };
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
    // The totals of everything shown.
    if !shown.is_empty() {
        let refs: Vec<&Row> = shown.iter().collect();
        let tot = totals(&refs, &calculated);
        let mut parts = vec![crate::i18n::fmt(tl!("Total: {n}"), &[("n", &tot.count.to_string())])];
        let values = tot.values_text();
        if !values.is_empty() {
            parts.push(values);
        }
        for (name, v) in &tot.calculated {
            parts.push(format!("{name} {}", crate::formula::show(*v)));
        }
        ui.label(egui::RichText::new(parts.join("   ·   ")).font(crate::theme::semibold(12.0)));
    }
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
    if report {
        summary_report(app, &shown, &custom);
    }
    if send {
        crate::takeoff_ui::send_quantities(app);
    }
}

/// The summary report of the rows shown (filters apply): grouped by layer, with each markup's
/// facts, your columns, its comment and its photo; cable totals and counts at the top.
fn summary_report(app: &mut PdfCraftApp, shown: &[Row], custom: &[String]) {
    let Some((_, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let no_layer = tl!("No layer").to_string();
    // Layers in name order, markups without one last; the list's order within each.
    let mut groups: Vec<String> = shown.iter().map(|r| if r.layer.is_empty() { no_layer.clone() } else { r.layer.clone() }).collect();
    groups.sort();
    groups.dedup();
    if let Some(i) = groups.iter().position(|g| *g == no_layer) {
        let g = groups.remove(i);
        groups.push(g);
    }
    let mut entries = Vec::new();
    let mut n = 0usize;
    for g in &groups {
        for r in shown.iter().filter(|r| (if r.layer.is_empty() { &no_layer } else { &r.layer }) == g) {
            n += 1;
            let a = doc.info.annotations.iter().find(|a| a.page == r.page && a.index == r.index);
            let text = a.and_then(|a| a.contents.clone()).unwrap_or_default();
            let heading = if r.label.is_empty() { format!("{n}. {}", r.kind) } else { format!("{n}. {} — {}", r.kind, r.label) };
            let mut details = vec![
                crate::i18n::fmt(tl!("Page {n}"), &[("n", &(r.page + 1).to_string())]),
                r.value.clone(),
                r.status.clone(),
                r.author.clone(),
                r.date.clone(),
            ];
            for name in custom {
                let v = r.field(name);
                if !v.is_empty() {
                    details.push(format!("{name}: {v}"));
                }
            }
            let photo = a.is_some_and(|a| a.photo) || doc.callout_side(r.page, r.index).is_some();
            // A measurement's comment is its value; it isn't repeated.
            let text = if text.trim() == r.value.trim() { String::new() } else { text };
            entries.push((
                pdfcraft_engine::report::ReportEntry { group: g.clone(), heading, details, text, photo: None },
                photo.then_some((r.page, r.index)),
            ));
        }
    }
    let mut summary = vec![crate::i18n::fmt(tl!("Markups: {n}"), &[("n", &n.to_string())])];
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for r in shown {
        match kinds.iter_mut().find(|k| k.0 == r.kind) {
            Some(k) => k.1 += 1,
            None => kinds.push((r.kind.clone(), 1)),
        }
    }
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if !kinds.is_empty() {
        summary.push(kinds.iter().map(|(k, c)| format!("{k}: {c}")).collect::<Vec<_>>().join("   "));
    }
    for t in crate::cable::totals(doc) {
        summary.push(crate::i18n::fmt(
            tl!("Cable ({layer}): {runs} runs, {length}"),
            &[("layer", &t.layer), ("runs", &t.runs.to_string()), ("length", &t.length)],
        ));
    }
    let title = crate::i18n::fmt(tl!("{name} — Markup report"), &[("name", &doc.name)]);
    let subtitle = pdf_date(&app.session.signing_date());
    let page_label = |p: usize, total: usize| crate::i18n::fmt(tl!("Page {n} of {total}"), &[("n", &p.to_string()), ("total", &total.to_string())]);
    let stem = doc.name.rsplit_once('.').map_or(doc.name.as_str(), |(s, _)| s).to_string();
    match app.session.markup_report(id, &title, &subtitle, &summary, entries, page_label) {
        Ok(bytes) => save_report(app, format!("{stem}-report.pdf"), bytes),
        Err(e) => app.notify(e.to_string()),
    }
}

/// Save the report where the user chooses.
fn save_report(app: &mut PdfCraftApp, name: String, bytes: std::sync::Arc<Vec<u8>>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let write = move |app: &mut PdfCraftApp, path: std::path::PathBuf| match crate::editing::write_atomically(&path.to_string_lossy(), &bytes) {
            Ok(()) => app.notify(tl!("Saved the summary report.")),
            Err(e) => app.notify(e.to_string()),
        };
        match app.export_dir_override.as_ref().map(|d| std::path::PathBuf::from(d).join(&name)) {
            Some(path) => write(app, path),
            None => {
                let dialog = crate::dialog::AsyncFileDialog::new().set_file_name(&name).add_filter("PDF", &["pdf"]);
                app.ask_one(crate::pickers::Ask::Save(dialog), None, write);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Err(e) = crate::editing::download(&name, &bytes) {
        app.notify(&e);
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
            name: String::new(),
            layer: String::new(),
            author: "Steve".into(),
            status: String::new(),
            value: value.map(|v| format!("{v}'")).unwrap_or_default(),
            amount: value,
            unit: "ft".into(),
            date: String::new(),
            fields: vec![("Progress".into(), "Pulled".into())],
            drafting: false,
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
        assert!(csv(&rows, &["Progress".to_string()]).starts_with("Type,Label,Name,Layer,Page,Author,Status,Value,Date,Progress\r\n"));
        let text = csv(&[row(0, "Note", "=SUM(A1), \"x\"", None)], &none);
        assert!(text.starts_with("Type,Label,Name,Layer,Page,Author,Status,Value,Date\r\n"));
        assert!(text.contains("\"'=SUM(A1), \"\"x\"\"\""), "{text}");
        // Drafted linework is listed only when asked for, or by its layer.
        let mut lines = vec![row(0, "Line", "", None), row(0, "Note", "", None)];
        lines[0].drafting = true;
        lines[0].layer = "Walls".into();
        let mut d = MarkupsState::default();
        assert_eq!(visible(&lines, &d, &none).len(), 1);
        d.drafting = true;
        assert_eq!(visible(&lines, &d, &none).len(), 2);
        d.drafting = false;
        d.layer = Some("Walls".into());
        assert_eq!(visible(&lines, &d, &none).len(), 1);
        assert_eq!(pdf_date("D:20261015093000-07'00'"), "2026-10-15");
    }

    #[test]
    fn calculated_columns_rules_groups_and_totals() {
        let none: [String; 0] = [];
        let mut rows = vec![row(0, "Cable run", "A", Some(100.0)), row(1, "Cable run", "B", Some(50.0)), row(1, "Data jack", "PC", None)];
        rows[0].fields.push(("System".into(), "Data".into()));
        rows[1].fields.push(("System".into(), "Voice".into()));
        calculate(
            &mut rows,
            &[("With slack".into(), "Value * 1.1".into()), ("Cost".into(), "[With slack] * 2".into()), ("Bad".into(), "Value +".into())],
        );
        let field = |r: &Row, n: &str| r.fields.iter().find(|(f, _)| f == n).map(|(_, v)| v.clone()).unwrap_or_default();
        assert_eq!(field(&rows[0], "With slack"), "110");
        assert_eq!(field(&rows[0], "Cost"), "220");
        assert!(field(&rows[2], "With slack").starts_with('⚠'), "a jack has no value");
        assert!(field(&rows[0], "Bad").starts_with('⚠'));
        // Totals: values per unit, calculated columns summed (rows without a number left out).
        let refs: Vec<&Row> = rows.iter().collect();
        let t = totals(&refs, &["With slack".to_string()]);
        assert_eq!(t.count, 3);
        assert_eq!(t.values, vec![("ft".to_string(), 150.0)]);
        assert!((t.calculated[0].1 - 165.0).abs() < 1e-9);
        // Rules: greater than on the value, equals on a column.
        let custom = ["System".to_string()];
        let mut s = MarkupsState { rules: vec![Rule { col: Col::Value, op: Op::Greater, value: "60".into() }], ..MarkupsState::default() };
        assert_eq!(visible(&rows, &s, &custom).iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["A"]);
        s.rules = vec![Rule { col: Col::Custom(0), op: Op::Equals, value: "voice".into() }];
        assert_eq!(visible(&rows, &s, &custom).len(), 1);
        s.rules = vec![Rule { col: Col::Custom(0), op: Op::NoValue, value: String::new() }];
        assert_eq!(visible(&rows, &s, &custom).len(), 1);
        // Grouped by type: the groups' rows together.
        s.rules.clear();
        s.group = Some(Col::Type);
        assert_eq!(visible(&rows, &s, &none).iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(), ["Cable run", "Cable run", "Data jack"]);
        assert_eq!(pdf_date("yesterday"), "yesterday");
    }
}
