//! RevuCraft's Punch list and Spaces boxes ([`pdfcraft_engine::punch`]).
//!
//! A punch item is a markup with a number (drawn in a badge on the sheet), an assignee, a due
//! date and an item status. A space is a named zone of a sheet; every markup whose middle is in
//! it belongs to it, and the Markups list gets a Space column.

use std::collections::HashMap;

use egui::RichText;
use pdfcraft_engine::punch::{Punch, PunchStatus};
use pdfcraft_engine::{DocId, Document, Edit};

use crate::comments::CommentTool;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, icons, widgets};

/// What the boxes remember between frames.
#[derive(Debug, Default)]
pub(crate) struct PunchState {
    /// The highest number given out per document while it is open, so a number taken off is not
    /// given to the next item.
    high: HashMap<DocId, u32>,
    hide_closed: bool,
    /// A field being typed in: (page, index, which field), its text. Saved when it loses focus.
    typing: Option<((usize, usize, u8), String)>,
    /// Drawing a new space: the document and how many rectangles and polygons it had.
    drawing: Option<(DocId, usize)>,
    /// The space whose name is being typed: (page, index, name).
    naming: Option<(usize, usize, String)>,
    /// Put the cursor in the name box (once, when naming starts).
    focus_name: bool,
}

/// A space: where it is, what it is called and its corners.
#[derive(Clone, Debug)]
pub(crate) struct SpaceInfo {
    pub page: usize,
    pub index: usize,
    pub name: String,
    pub corners: Vec<[f64; 2]>,
}

/// The spaces of a document.
pub(crate) fn spaces(doc: &Document) -> Vec<SpaceInfo> {
    let mut pages: Vec<usize> = doc.info.annotations.iter().map(|a| a.page).collect();
    pages.sort_unstable();
    pages.dedup();
    let mut out = Vec::new();
    for page in pages {
        for index in doc.spaces(page) {
            let Some(a) = doc.info.annotations.iter().find(|a| a.page == page && a.index == index) else { continue };
            let corners: Vec<[f64; 2]> = if a.points.len() >= 3 {
                a.points.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect()
            } else {
                let r = a.rect.map(f64::from);
                vec![[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
            };
            out.push(SpaceInfo { page, index, name: a.contents.clone().unwrap_or_default(), corners });
        }
    }
    out
}

/// The space a markup on `page` with rectangle `rect` is in (its middle decides).
pub(crate) fn space_of(spaces: &[SpaceInfo], page: usize, index: usize, rect: [f32; 4]) -> Option<&SpaceInfo> {
    let mid = [f64::from(rect[0] + rect[2]) / 2.0, f64::from(rect[1] + rect[3]) / 2.0];
    let here: Vec<(usize, Vec<[f64; 2]>)> =
        spaces.iter().enumerate().filter(|(_, s)| s.page == page && s.index != index).map(|(i, s)| (i, s.corners.clone())).collect();
    pdfcraft_engine::punch::space_at(&here, mid).and_then(|i| spaces.get(i))
}

/// One punch item as the box shows it.
#[derive(Clone, Debug)]
struct Item {
    page: usize,
    index: usize,
    punch: Punch,
    kind: String,
    label: String,
    space: String,
    status: String,
    author: String,
    date: String,
}

fn items(doc: &Document) -> Vec<Item> {
    let rows = crate::markups_list::rows(doc);
    let mut pages: Vec<usize> = doc.info.annotations.iter().map(|a| a.page).collect();
    pages.sort_unstable();
    pages.dedup();
    let mut out: Vec<Item> = pages
        .into_iter()
        .flat_map(|page| doc.punch_items(page).into_iter().map(move |(index, punch)| (page, index, punch)))
        .map(|(page, index, punch)| {
            let row = rows.iter().find(|r| r.page == page && r.index == index);
            let get = |f: fn(&crate::markups_list::Row) -> String| row.map(f).unwrap_or_default();
            Item {
                page,
                index,
                kind: get(|r| r.kind.clone()),
                label: get(|r| r.label.clone()),
                space: row.and_then(|r| r.fields.iter().find(|(n, _)| n == tl!("Space")).map(|(_, v)| v.clone())).unwrap_or_default(),
                status: get(|r| r.status.clone()),
                author: get(|r| r.author.clone()),
                date: get(|r| r.date.clone()),
                punch,
            }
        })
        .collect();
    out.sort_by_key(|i| (i.punch.no, i.page, i.index));
    out
}

/// A due date that has passed on an item still open.
fn overdue(p: &Punch, today: chrono::NaiveDate) -> bool {
    p.status != PunchStatus::Closed && chrono::NaiveDate::parse_from_str(p.due.trim(), "%Y-%m-%d").is_ok_and(|d| d < today)
}

/// The next number for `doc`: after the highest in it and the highest given out while it is open.
fn next_number(state: &PunchState, id: DocId, doc: &Document) -> u32 {
    let in_doc = items(doc).iter().map(|i| i.punch.no).max().unwrap_or(0);
    in_doc.max(state.high.get(&id).copied().unwrap_or(0)).saturating_add(1).min(pdfcraft_engine::punch::MAX_NUMBER)
}

/// Put the selected markups on the punch list, in page order and then the order they were drawn.
pub(crate) fn add_selected(app: &mut PdfCraftApp) {
    let Some((i, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let mut chosen = app.views.get(i).map(|v| v.comments.group()).unwrap_or_default();
    chosen.sort_unstable();
    let already: Vec<(usize, usize)> = items(doc).iter().map(|i| (i.page, i.index)).collect();
    let spaces: Vec<(usize, usize)> = spaces(doc).iter().map(|s| (s.page, s.index)).collect();
    chosen.retain(|k| !already.contains(k) && !spaces.contains(k));
    if chosen.is_empty() {
        app.notify_tr("Select the markups to add first (ones already on the list are skipped).");
        return;
    }
    let mut no = next_number(&app.punch, id, doc);
    let mut edits = Vec::new();
    for (page, index) in &chosen {
        edits.push(Edit::SetPunch { page: *page, index: *index, punch: Some(Punch { no, ..Punch::default() }) });
        app.punch.high.insert(id, no);
        no = no.saturating_add(1).min(pdfcraft_engine::punch::MAX_NUMBER);
    }
    let n = edits.len();
    let edit = if n == 1 { edits.remove(0) } else { Edit::Batch { label: "Add to the punch list".into(), edits } };
    if app.apply_edit(edit) {
        crate::cable::ensure_column(&mut app.markup_columns, "Punch", &[]);
        let msg = if n == 1 {
            tl!("Added 1 item to the punch list.").to_string()
        } else {
            crate::i18n::fmt(tl!("Added {n} items to the punch list."), &[("n", &n.to_string())])
        };
        app.notify(msg);
    }
}

fn go_to(app: &mut PdfCraftApp, page: usize, index: usize) {
    let Some((i, _)) = app.active_ids() else { return };
    if let Some(view) = app.views.get_mut(i) {
        view.go_to_page(page);
        view.comments.select_one(page, index);
        view.comments.reveal = true;
    }
}

/// A text field that is saved when it loses focus: `Some(text)` then, when it changed.
fn field(
    ui: &mut egui::Ui,
    typing: &mut Option<((usize, usize, u8), String)>,
    key: (usize, usize, u8),
    value: &str,
    hint: &str,
    width: f32,
) -> Option<String> {
    let mine = typing.as_ref().filter(|(k, _)| *k == key).map(|(_, t)| t.clone());
    let mut text = mine.clone().unwrap_or_else(|| value.to_string());
    let resp = ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(width));
    if resp.has_focus() || resp.changed() {
        *typing = Some((key, text.clone()));
    }
    if resp.lost_focus() {
        if mine.is_some() {
            *typing = None;
        }
        return (text.trim() != value).then(|| text.trim().to_string());
    }
    None
}

/// The Punch list box.
pub(crate) fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((i, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let list = items(doc);
    let selected = app.views.get(i).is_some_and(|v| !v.comments.group().is_empty());
    let today = chrono::Local::now().date_naive();
    let mut add = false;
    let mut export = false;
    let mut goto = None;
    let mut edits: Vec<Edit> = Vec::new();
    ui.horizontal_wrapped(|ui| {
        add = ui.add_enabled(selected, egui::Button::new(tl!("Add selected to the punch list"))).clicked();
        export = ui.add_enabled(!list.is_empty(), egui::Button::new(tl!("Export CSV"))).clicked();
    });
    ui.add(widgets::checkbox(&mut app.punch.hide_closed, tl!("Hide closed items")));
    let open = list.iter().filter(|i| i.punch.status != PunchStatus::Closed).count();
    let late = list.iter().filter(|i| overdue(&i.punch, today)).count();
    ui.label(
        RichText::new(crate::i18n::fmt(
            tl!("{all} items, {open} open, {late} overdue"),
            &[("all", &list.len().to_string()), ("open", &open.to_string()), ("late", &late.to_string())],
        ))
        .color(t.text_muted)
        .small(),
    );
    if list.is_empty() {
        ui.label(
            RichText::new(tl!("Select markups on the sheet (a cloud, a note, a photo) and add them: each gets a number on the sheet."))
                .color(t.text_muted),
        );
    }
    egui::ScrollArea::vertical().id_salt("punch-list").auto_shrink([false, false]).show(ui, |ui| {
        for item in list.iter().filter(|i| !(app.punch.hide_closed && i.punch.status == PunchStatus::Closed)) {
            ui.separator();
            let (page, index) = (item.page, item.index);
            ui.horizontal(|ui| {
                let head = format!("#{}  {}", item.punch.no, if item.label.is_empty() { &item.kind } else { &item.label });
                if ui.add(egui::Label::new(RichText::new(head).font(theme::semibold(13.5))).sense(egui::Sense::click()).truncate()).clicked() {
                    goto = Some((page, index));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, "x", 20.0, false, tl!("Take off the punch list")).clicked() {
                        edits.push(Edit::SetPunch { page, index, punch: None });
                    }
                });
            });
            let mut place = crate::i18n::fmt(tl!("Page {n}"), &[("n", &(page + 1).to_string())]);
            if !item.space.is_empty() {
                place = format!("{place} · {}", item.space);
            }
            if overdue(&item.punch, today) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&place).color(t.text_faint).small());
                    ui.label(RichText::new(tl!("Overdue")).color(ui.visuals().error_fg_color).small().strong());
                });
            } else {
                ui.label(RichText::new(&place).color(t.text_faint).small());
            }
            ui.horizontal_wrapped(|ui| {
                let typing = &mut app.punch.typing;
                if let Some(v) = field(ui, typing, (page, index, 0), &item.punch.assignee, tl!("Assignee"), 110.0) {
                    edits.push(Edit::SetPunch { page, index, punch: Some(Punch { assignee: v, ..item.punch.clone() }) });
                }
                if let Some(v) = field(ui, typing, (page, index, 1), &item.punch.due, tl!("Due (YYYY-MM-DD)"), 110.0) {
                    edits.push(Edit::SetPunch { page, index, punch: Some(Punch { due: v, ..item.punch.clone() }) });
                }
                let mut status = item.punch.status;
                egui::ComboBox::from_id_salt(("punch-status", page, index)).selected_text(tl!(status.label())).show_ui(ui, |ui| {
                    for s in PunchStatus::ALL {
                        ui.selectable_value(&mut status, s, tl!(s.label()));
                    }
                });
                if status != item.punch.status {
                    edits.push(Edit::SetPunch { page, index, punch: Some(Punch { status, ..item.punch.clone() }) });
                }
            });
        }
    });
    if add {
        add_selected(app);
    }
    for edit in edits {
        app.apply_edit(edit);
    }
    if let Some((page, index)) = goto {
        go_to(app, page, index);
    }
    if export {
        export_csv(app, &list);
    }
}

/// The list as a spreadsheet, in number order.
fn csv(list: &[Item]) -> String {
    let cell = crate::markups_list::cell;
    let mut out = String::new();
    let head = ["No.", "Page", "Space", "Item", "Comment", "Assignee", "Due", "Status", "Markup status", "Author", "Date"];
    out.push_str(&head.iter().map(|h| cell(tl!(h))).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
    for i in list {
        let row = [
            i.punch.no.to_string(),
            (i.page + 1).to_string(),
            i.space.clone(),
            i.kind.clone(),
            i.label.clone(),
            i.punch.assignee.clone(),
            i.punch.due.clone(),
            tl!(i.punch.status.label()).to_string(),
            i.status.clone(),
            i.author.clone(),
            i.date.clone(),
        ];
        out.push_str(&row.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

fn export_csv(app: &mut PdfCraftApp, list: &[Item]) {
    let csv = csv(list);
    let name = format!("{}.csv", tl!("punch list"));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let write =
            move |app: &mut PdfCraftApp, path: std::path::PathBuf| match crate::editing::write_atomically(&path.to_string_lossy(), csv.as_bytes()) {
                Ok(()) => app.notify(tl!("Saved the punch list.")),
                Err(e) => app.notify(e.to_string()),
            };
        match app.export_dir_override.as_ref().map(|d| std::path::PathBuf::from(d).join(&name)) {
            Some(path) => write(app, path),
            None => {
                let dialog = crate::dialog::AsyncFileDialog::new().set_file_name(&name).add_filter("CSV", &["csv"]);
                app.ask_one(crate::pickers::Ask::Save(dialog), None, write);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Err(e) = crate::editing::download(&name, csv.as_bytes()) {
        app.notify(&e);
    }
}

/// How many rectangles and polygons a document has (a new one is a space being drawn).
fn shapes(doc: &Document) -> usize {
    doc.info.annotations.iter().filter(|a| a.in_reply_to.is_none() && matches!(a.subtype.as_str(), "Square" | "Polygon")).count()
}

/// Every frame: a space being drawn becomes one as soon as its shape is made.
pub(crate) fn tick(app: &mut PdfCraftApp) {
    let Some((doc_id, before)) = app.punch.drawing else { return };
    if !matches!(app.quick_tool, QuickTool::Comment(CommentTool::Rectangle | CommentTool::Polygon)) {
        app.punch.drawing = None;
        return;
    }
    let Some((i, id)) = app.active_ids() else { return };
    if id != doc_id {
        return;
    }
    let Some(doc) = app.session.get(id) else { return };
    if shapes(doc) <= before {
        return;
    }
    // The shape just drawn is the selected one.
    let Some((page, index)) = app.views.get(i).and_then(|v| v.comments.selected) else { return };
    let is_shape = doc.info.annotations.iter().any(|a| a.page == page && a.index == index && matches!(a.subtype.as_str(), "Square" | "Polygon"));
    if !is_shape {
        return;
    }
    let name = crate::i18n::fmt(tl!("Space {n}"), &[("n", &(spaces(doc).len() + 1).to_string())]);
    app.punch.drawing = None;
    app.quick_tool = QuickTool::Select;
    if app.apply_edit(Edit::SetSpace { page, index, name: name.clone() }) {
        crate::cable::ensure_column(&mut app.markup_columns, "Space", &[]);
        app.punch.naming = Some((page, index, name));
        app.punch.focus_name = true;
        crate::links_ui::show_pane(app, crate::RightPanel::Spaces);
    }
}

/// The Spaces box.
pub(crate) fn spaces_box(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((i, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let all = spaces(doc);
    let rows = crate::markups_list::rows(doc);
    let before = shapes(doc);
    let selected_shape = app.views.get(i).and_then(|v| v.comments.selected).filter(|(p, x)| {
        doc.info.annotations.iter().any(|a| a.page == *p && a.index == *x && matches!(a.subtype.as_str(), "Square" | "Polygon"))
            && !all.iter().any(|s| s.page == *p && s.index == *x)
    });
    let mut draw = None;
    let mut make = false;
    let mut goto = None;
    let mut rename = None;
    if app.punch.drawing.is_some() {
        ui.label(
            RichText::new(tl!("Draw the space on the sheet: drag a rectangle, or click its corners and double-click to finish.")).color(t.text_muted),
        );
        if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
            app.punch.drawing = None;
            app.quick_tool = QuickTool::Select;
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            if widgets::ghost_button(ui, "square-dashed", tl!("Rectangle space")).clicked() {
                draw = Some(CommentTool::Rectangle);
            }
            if widgets::ghost_button(ui, "square-dashed", tl!("Polygon space")).clicked() {
                draw = Some(CommentTool::Polygon);
            }
            make = ui.add_enabled(selected_shape.is_some(), egui::Button::new(tl!("Make the selected shape a space"))).clicked();
        });
    }
    // Naming a space: a new one, or a rename.
    if let Some((page, index, mut name)) = app.punch.naming.take() {
        ui.add_space(4.0);
        ui.label(RichText::new(tl!("Space name")).small());
        let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
        if std::mem::take(&mut app.punch.focus_name) {
            resp.request_focus();
        }
        let done = resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
        if done {
            if !name.trim().is_empty() {
                rename = Some((page, index, name));
            }
        } else {
            app.punch.naming = Some((page, index, name));
        }
    }
    if all.is_empty() {
        ui.label(RichText::new(tl!("Name the rooms or zones of a sheet, then filter, count and export the markups in each.")).color(t.text_muted));
    }
    egui::ScrollArea::vertical().id_salt("spaces-list").auto_shrink([false, false]).show(ui, |ui| {
        for s in &all {
            let count =
                rows.iter().filter(|r| r.page == s.page && space_of(&all, r.page, r.index, r.rect).is_some_and(|x| x.index == s.index)).count();
            ui.horizontal(|ui| {
                let head = RichText::new(&s.name).font(theme::semibold(13.5));
                if ui.add(egui::Label::new(head).sense(egui::Sense::click()).truncate()).clicked() {
                    goto = Some((s.page, s.index));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, "pencil", 20.0, false, tl!("Rename")).clicked() {
                        app.punch.naming = Some((s.page, s.index, s.name.clone()));
                        app.punch.focus_name = true;
                    }
                    let what = crate::i18n::fmt(tl!("Page {n}"), &[("n", &(s.page + 1).to_string())]);
                    let markups = crate::i18n::fmt(tl!("{n} markups"), &[("n", &count.to_string())]);
                    ui.label(RichText::new(format!("{what} · {markups}")).color(t.text_faint).small());
                });
            });
        }
    });
    if let Some(tool) = draw {
        app.punch.drawing = Some((id, before));
        app.quick_tool = QuickTool::Comment(tool);
    }
    if make && let Some((page, index)) = selected_shape {
        let name = crate::i18n::fmt(tl!("Space {n}"), &[("n", &(all.len() + 1).to_string())]);
        if app.apply_edit(Edit::SetSpace { page, index, name: name.clone() }) {
            crate::cable::ensure_column(&mut app.markup_columns, "Space", &[]);
            app.punch.naming = Some((page, index, name));
            app.punch.focus_name = true;
        }
    }
    if let Some((page, index, name)) = rename {
        app.apply_edit(Edit::SetSpace { page, index, name });
    }
    if let Some((page, index)) = goto {
        go_to(app, page, index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overdue_items_are_open_ones_past_their_date() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 28).unwrap();
        let p = |due: &str, status| Punch { no: 1, assignee: String::new(), due: due.into(), status };
        assert!(overdue(&p("2026-10-27", PunchStatus::Open), today));
        assert!(!overdue(&p("2026-10-28", PunchStatus::Open), today));
        assert!(!overdue(&p("2026-10-01", PunchStatus::Closed), today));
        assert!(!overdue(&p("next week", PunchStatus::Open), today));
        assert!(!overdue(&p("", PunchStatus::InProgress), today));
    }

    #[test]
    fn the_csv_lists_items_in_number_order_with_their_details() {
        let item = |no| Item {
            page: 1,
            index: 0,
            punch: Punch { no, assignee: "Plumber, licensed".into(), due: "2026-11-02".into(), status: PunchStatus::Ready },
            kind: "Cloud".into(),
            label: "Leak".into(),
            space: "Kitchen".into(),
            status: String::new(),
            author: "Steve".into(),
            date: String::new(),
        };
        let text = csv(&[item(1), item(2)]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].starts_with("1,2,Kitchen,Cloud,Leak,\"Plumber, licensed\",2026-11-02,Ready for review"), "{}", lines[1]);
    }
}
