//! RevuCraft: Send quantities. Every measurement and symbol count goes to a workbook's Takeoff
//! sheet, each quantity a named cell (`=Kitchen_floor * 6.50`), and the tables placed from that
//! workbook are updated at once. The Links pane and the Workbook box say when the drawing has
//! changed since, or when the workbook was saved without the latest quantities (it was open in a
//! spreadsheet when they were sent), and send them again.

use std::time::{Duration, Instant};

use pdfcraft_engine::cad_links::{LinkKind, LinkedFile};
use pdfcraft_engine::{DocId, Edit, takeoff};

use crate::links_ui::{file_meta, file_name, here, linked_file_from, locate_path, now_secs, show_pane, update};
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

/// How often the workbook is looked at again.
const LOOK_EVERY: Duration = Duration::from_secs(2);

/// Whether the workbook has the document's latest quantities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// Never sent from this document.
    Unsent,
    UpToDate {
        path: String,
    },
    /// The measurements or counts changed since they were sent.
    Changed {
        path: String,
    },
    /// The workbook was saved since without them (a copy open in a spreadsheet saved over it).
    SavedOver {
        path: String,
    },
    /// The workbook isn't where it was, next to the PDF, or found by name.
    Missing {
        name: String,
    },
}

/// The Send quantities dialog: the workbooks to choose from.
#[derive(Debug)]
struct SendDialog {
    choices: Vec<String>,
    choice: usize,
}

/// Send quantities' state.
#[derive(Debug, Default)]
pub(crate) struct TakeoffState {
    dialog: Option<SendDialog>,
    /// The document's quantities' fingerprint, for an edit generation.
    current: Option<(DocId, u64, String)>,
    /// A workbook looked at: its path, size and date, the fingerprint looked for, and whether
    /// it had those quantities.
    checked: Option<(String, (u64, i64), String, bool)>,
    /// The status shown, for a document, and when it was worked out.
    status: Option<(DocId, u64, Instant, Status)>,
}

impl TakeoffState {
    /// Look again on the next frame (after a send, or a change elsewhere).
    fn forget(&mut self) {
        self.status = None;
        self.checked = None;
    }
}

/// Your own columns' names, written after the fixed columns.
fn columns(app: &PdfCraftApp) -> Vec<String> {
    app.markup_columns.iter().map(|c| c.name.clone()).collect()
}

/// The workbooks quantities could go to: those of the tables placed in the document.
fn candidates(app: &PdfCraftApp) -> Vec<String> {
    let (links, _) = here(app);
    let mut out: Vec<String> = links.into_iter().filter(|l| l.kind == LinkKind::Table && takeoff::can_send(&l.path)).map(|l| l.path).collect();
    out.sort();
    out.dedup();
    out
}

/// File ▸ Send quantities…: straight to the workbook they were last sent to, else the dialog.
pub(crate) fn send_quantities(app: &mut PdfCraftApp) {
    if app.active_ids().is_none() {
        return;
    }
    match status(app) {
        Status::UpToDate { path } | Status::Changed { path } | Status::SavedOver { path } => {
            send_to(app, path, false);
        }
        Status::Unsent | Status::Missing { .. } => choose(app),
    }
}

/// Open the dialog to choose the workbook.
fn choose(app: &mut PdfCraftApp) {
    let choices = candidates(app);
    app.links.takeoff.dialog = Some(SendDialog { choices, choice: 0 });
}

/// Writes `bytes` to `path` through a file beside it, so a failed write leaves the workbook as
/// it was.
fn write_whole(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = format!("{path}.revucraft-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Sends the quantities to the workbook at `path` (made anew when `new`): names the measurements
/// that have none, writes the Takeoff sheet, records it in the PDF and updates the tables placed
/// from that workbook. False when nothing was sent.
pub(crate) fn send_to(app: &mut PdfCraftApp, path: String, new: bool) -> bool {
    let Some((_, id)) = app.active_ids() else { return false };
    let name = file_name(&path);
    if !new && !takeoff::can_send(&name) {
        app.notify(crate::i18n::fmt(tl!("Quantities are sent to XLSX workbooks; {name} isn't one."), &[("name", &name)]));
        return false;
    }
    if app.session.get(id).is_some_and(|d| d.measurements_need_names()) && !app.apply_edit(Edit::NameMeasurements) {
        return false;
    }
    let cols = columns(app);
    let Some(doc) = app.session.get(id) else { return false };
    let t = doc.takeoff(&cols);
    let pdf = doc.path.clone();
    if t.rows.is_empty() {
        app.notify(tl!("There are no measurements or symbols to send yet."));
        return false;
    }
    let made = if new {
        takeoff::new_workbook(&t)
    } else {
        match std::fs::read(&path) {
            Ok(bytes) => takeoff::write(&bytes, &t),
            Err(e) => {
                app.notify(crate::i18n::fmt(tl!("Couldn't read {name}: {e}"), &[("name", &name), ("e", &e.to_string())]));
                return false;
            }
        }
    };
    let sent = match made {
        Ok(s) => s,
        Err(e) => {
            app.notify(crate::i18n::fmt(tl!("Couldn't send the quantities to {name}: {e}"), &[("name", &name), ("e", &e.to_string())]));
            return false;
        }
    };
    if let Err(e) = write_whole(&path, &sent.bytes) {
        app.notify(crate::i18n::fmt(
            tl!("Couldn't save {name}: {e}. If it is open in Excel, close it and send again."),
            &[("name", &name), ("e", &e.to_string())],
        ));
        return false;
    }
    let size = u64::try_from(sent.bytes.len()).unwrap_or(0);
    let file = linked_file_from(&path, pdf.as_deref()).unwrap_or(LinkedFile { path: path.clone(), relative: None, size, mtime: 0, now: now_secs() });
    app.apply_edit(Edit::RecordTakeoff { file, sent: t.fingerprint() });
    // The tables placed from this workbook show the new numbers.
    let (links, _) = here(app);
    for index in links.iter().filter(|l| l.kind == LinkKind::Table && l.path == path).map(|l| l.index).collect::<Vec<_>>() {
        if !update(app, index, false) {
            break;
        }
    }
    let mut msg = crate::i18n::fmt(tl!("Sent the quantities to {name}'s Takeoff sheet."), &[("name", &name)]);
    for note in sent.notes.iter().take(3) {
        msg.push(' ');
        msg.push_str(note);
    }
    app.notify(msg);
    app.links.takeoff.forget();
    true
}

/// Whether the workbook has the document's latest quantities, looked at every few seconds and
/// whenever the document changes.
pub(crate) fn status(app: &mut PdfCraftApp) -> Status {
    let Some((_, id)) = app.active_ids() else { return Status::Unsent };
    let Some(doc) = app.session.get(id) else { return Status::Unsent };
    let generation = doc.edit_generation();
    if let Some((d, g, at, s)) = &app.links.takeoff.status
        && *d == id
        && *g == generation
        && at.elapsed() < LOOK_EVERY
    {
        return s.clone();
    }
    let Some(record) = doc.takeoff_record() else {
        app.links.takeoff.status = Some((id, generation, Instant::now(), Status::Unsent));
        return Status::Unsent;
    };
    let cols = columns(app);
    let Some(doc) = app.session.get(id) else { return Status::Unsent };
    let current = match &app.links.takeoff.current {
        Some((d, g, f)) if *d == id && *g == generation => f.clone(),
        _ => {
            let f = doc.takeoff(&cols).fingerprint();
            app.links.takeoff.current = Some((id, generation, f.clone()));
            f
        }
    };
    let status = match locate_path(&record.path, record.relative.as_deref(), doc.path.as_deref(), false) {
        None => Status::Missing { name: file_name(&record.path) },
        Some((path, _)) => {
            let meta = file_meta(&path);
            if current != record.sent {
                Status::Changed { path }
            } else if meta.is_some_and(|m| m == (record.size, record.mtime)) {
                Status::UpToDate { path }
            } else {
                // Saved since it was sent: does it still have them?
                let has = match &app.links.takeoff.checked {
                    Some((p, m, f, has)) if *p == path && Some(*m) == meta && *f == current => *has,
                    _ => {
                        let t = doc.takeoff(&cols);
                        let has = std::fs::read(&path)
                            .ok()
                            .and_then(|b| pdfcraft_engine::workbook::open(&b, &file_name(&path)).ok())
                            .is_some_and(|book| book.has_takeoff(&t));
                        app.links.takeoff.checked = Some((path.clone(), meta.unwrap_or_default(), current.clone(), has));
                        has
                    }
                };
                if has { Status::UpToDate { path } } else { Status::SavedOver { path } }
            }
        }
    };
    app.links.takeoff.status = Some((id, generation, Instant::now(), status.clone()));
    status
}

/// The Quantities section at the top of the Links pane and the Workbook box.
pub(crate) fn section(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let editable = app.active_ids().and_then(|(_, id)| app.session.get(id)).is_some_and(|d| d.allows_modification() && d.read_only_reason.is_none());
    let warn = egui::Color32::from_rgb(0xd9, 0x9a, 0x1c);
    let status = status(app);
    let (text, colour, button) = match &status {
        Status::Unsent => (
            tl!("Send the measurements and symbol counts to a workbook's Takeoff sheet, each quantity a named cell for its formulas.").to_string(),
            t.text_muted,
            tl!("Send quantities…"),
        ),
        Status::UpToDate { path } => {
            (crate::i18n::fmt(tl!("Sent to {name}: up to date."), &[("name", &file_name(path))]), t.text_muted, tl!("Send again"))
        }
        Status::Changed { path } => (
            crate::i18n::fmt(tl!("The quantities have changed since they were sent to {name}."), &[("name", &file_name(path))]),
            warn,
            tl!("Send quantities"),
        ),
        Status::SavedOver { path } => (
            crate::i18n::fmt(tl!("{name} was saved without the latest quantities (it was open when they were sent)."), &[("name", &file_name(path))]),
            warn,
            tl!("Send again"),
        ),
        Status::Missing { name } => (crate::i18n::fmt(tl!("{name} can't be found."), &[("name", name)]), warn, tl!("Choose a workbook…")),
    };
    egui::Frame::NONE.fill(t.field).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.add(icons::image("send", 14.0, t.icon));
            ui.label(egui::RichText::new(tl!("Quantities")).font(theme::semibold(13.0)));
        });
        ui.label(egui::RichText::new(text).color(colour).small());
        ui.horizontal_wrapped(|ui| {
            let send = ui.add_enabled(editable, egui::Button::new(button)).on_hover_text(tl!(
                "Writes a Takeoff sheet into the workbook: one row per measurement and symbol count, each quantity a named cell. The rest of the workbook is left as it is."
            ));
            if send.clicked() {
                match &status {
                    Status::Missing { .. } => choose(app),
                    _ => send_quantities(app),
                }
            }
            if !matches!(status, Status::Unsent | Status::Missing { .. }) && ui.add_enabled(editable, egui::Button::new(tl!("Change…"))).clicked() {
                choose(app);
            }
        });
    });
    ui.add_space(6.0);
}

/// The Send quantities dialog.
pub(crate) fn dialog(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(d) = app.links.takeoff.dialog.as_mut() else { return };
    let t = Tokens::get(ctx);
    let mut act: Option<Act> = None;
    let modal = egui::Modal::new(egui::Id::new("send_quantities")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.horizontal(|ui| {
            ui.add(icons::image("send", 20.0, t.accent));
            ui.label(egui::RichText::new(tl!("Send quantities")).font(theme::semibold(16.0)));
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(tl!(
                "RevuCraft writes a Takeoff sheet into the workbook: one row per measurement and symbol count, each quantity a named cell, so formulas such as =Kitchen_floor*6.50 follow the drawing. The rest of the workbook is left as it is."
            ))
            .color(t.text_muted),
        );
        ui.add_space(8.0);
        if d.choices.is_empty() {
            ui.label(tl!("No XLSX workbook is placed in this document yet (a CSV can't hold a Takeoff sheet)."));
        } else {
            ui.label(egui::RichText::new(tl!("Workbooks placed in this document")).font(theme::semibold(13.0)));
            for (i, p) in d.choices.iter().enumerate() {
                let _ = widgets::radio_value(ui, &mut d.choice, i, file_name(p));
            }
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(tl!("Choose a workbook…")).clicked() {
                act = Some(Act::Choose);
            }
            if ui.button(tl!("New workbook…")).clicked() {
                act = Some(Act::New);
            }
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(tl!("Close the workbook in GridCraft or Excel first, or its next save puts the old quantities back.")).color(t.text_faint).small(),
        );
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::pill_button(ui, tl!("Send"), true).clicked() && !d.choices.is_empty() {
                act = Some(Act::Send);
            }
            if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                act = Some(Act::Cancel);
            }
        });
    });
    if modal.should_close() && act.is_none() {
        act = Some(Act::Cancel);
    }
    let Some(act) = act else { return };
    let dialog = app.links.takeoff.dialog.take();
    let Some((_, id)) = app.active_ids() else { return };
    match act {
        Act::Cancel => {}
        Act::Send => {
            if let Some(path) = dialog.and_then(|d| d.choices.get(d.choice).cloned()) {
                send_to(app, path, false);
            }
        }
        Act::Choose => {
            let picker = crate::dialog::AsyncFileDialog::new().add_filter(tl!("Workbooks (XLSX)"), &takeoff::EXTENSIONS);
            let _ = app.ask_one(crate::pickers::Ask::File(picker), Some(id), |app, p| {
                send_to(app, p.to_string_lossy().into_owned(), false);
            });
        }
        Act::New => {
            let picker = crate::dialog::AsyncFileDialog::new().set_file_name("Takeoff.xlsx").add_filter(tl!("Workbooks (XLSX)"), &["xlsx"]);
            let _ = app.ask_one(crate::pickers::Ask::Save(picker), Some(id), |app, p| {
                let p = if takeoff::can_send(&p.to_string_lossy()) { p } else { p.with_extension("xlsx") };
                send_to(app, p.to_string_lossy().into_owned(), true);
            });
        }
    }
    show_pane(app, crate::RightPanel::Links);
}

/// What the dialog asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Send,
    Choose,
    New,
    Cancel,
}

/// Screenshots and automation (`send-quantities`, `quantities-dialog`): send to the workbook at
/// `value` (a new one when it doesn't exist), or open the dialog.
pub(crate) fn option(app: &mut PdfCraftApp, value: &str, send: bool) -> Result<(), String> {
    if app.active_ids().is_none() {
        return Err("open a document first".into());
    }
    if !send {
        choose(app);
        return Ok(());
    }
    let new = !std::path::Path::new(value).exists();
    if send_to(app, value.to_string(), new) { Ok(()) } else { Err("the quantities could not be sent".into()) }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A one-page Letter document with a proper xref table.
    fn letter_pdf() -> Vec<u8> {
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>",
            "<< /Type /Page /Parent 2 0 R >>",
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    fn area(label: &str, width: f64) -> Result<Edit, String> {
        use pdfcraft_engine::measure;
        Ok(Edit::AddMeasurement(measure::NewMeasurement {
            page: 0,
            kind: measure::Kind::Area,
            points: vec![[100.0, 100.0], [100.0 + width, 100.0], [100.0 + width, 200.0], [100.0, 200.0]],
            scale: measure::Scale::new(0.1, "ft", 2).map_err(|e| e.to_string())?,
            style: pdfcraft_engine::Style::default(),
            label: label.into(),
            author: "Steve".into(),
            dimension: None,
        }))
    }

    #[test]
    fn quantities_are_sent_watched_and_sent_again() -> Result<(), String> {
        let err = |e: std::io::Error| e.to_string();
        let dir = std::env::temp_dir().join(format!("revucraft-send-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(err)?;
        let book = dir.join("Takeoff.xlsx");
        let path = book.to_string_lossy().into_owned();
        let mut app = PdfCraftApp::new();
        app.open_bytes("plan.pdf", None, letter_pdf())?;
        assert_eq!(status(&mut app), Status::Unsent);
        assert!(!send_to(&mut app, path.clone(), true), "nothing to send yet");
        assert!(app.apply_edit(area("Kitchen floor", 100.0)?));
        assert!(send_to(&mut app, path.clone(), true));
        let bytes = std::fs::read(&book).map_err(err)?;
        let wb = pdfcraft_engine::workbook::open(&bytes, "Takeoff.xlsx").map_err(|e| e.to_string())?;
        assert_eq!(wb.sheet_names(), ["Budget", "Takeoff"]);
        assert_eq!(status(&mut app), Status::UpToDate { path: path.clone() });

        // Another area: the workbook no longer has the latest quantities.
        assert!(app.apply_edit(area("", 50.0)?));
        assert_eq!(status(&mut app), Status::Changed { path: path.clone() });
        send_quantities(&mut app);
        assert_eq!(status(&mut app), Status::UpToDate { path: path.clone() });

        // A spreadsheet that had the first version open saves it over the second.
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&book, &bytes).map_err(err)?;
        app.links.takeoff.forget();
        assert_eq!(status(&mut app), Status::SavedOver { path: path.clone() });
        send_quantities(&mut app);
        assert_eq!(status(&mut app), Status::UpToDate { path: path.clone() });

        // Moved away: missing, and the dialog offers to choose another.
        std::fs::remove_file(&book).map_err(err)?;
        app.links.takeoff.forget();
        assert_eq!(status(&mut app), Status::Missing { name: "Takeoff.xlsx".into() });
        send_quantities(&mut app);
        assert!(app.links.takeoff.dialog.is_some());
        assert!(!send_to(&mut app, dir.join("list.csv").to_string_lossy().into_owned(), false), "only XLSX workbooks");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_failed_write_leaves_the_workbook_and_no_stray_file() -> Result<(), String> {
        let err = |e: std::io::Error| e.to_string();
        let dir = std::env::temp_dir().join(format!("revucraft-takeoff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(err)?;
        let book = dir.join("budget.xlsx");
        std::fs::write(&book, b"old").map_err(err)?;
        let path = book.to_string_lossy().into_owned();
        write_whole(&path, b"new").map_err(err)?;
        assert_eq!(std::fs::read(&book).map_err(err)?, b"new");
        assert!(!std::path::Path::new(&format!("{path}.revucraft-tmp")).exists());
        assert!(write_whole(&dir.join("no-such-folder").join("b.xlsx").to_string_lossy(), b"x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
