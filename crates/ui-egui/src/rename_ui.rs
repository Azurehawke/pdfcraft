//! Rename the drawing: its job folder and the links in its notes follow the new name (the engine's
//! `rename` module works out and makes the changes; this dialog asks for the name and then saves).
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pdfcraft_engine::{DocId, Edit};

use crate::PdfCraftApp;

/// The dialog's state: the new name and why the last try failed.
pub(crate) struct RenameJob {
    doc: DocId,
    text: String,
    problem: Option<String>,
}

/// Open the dialog for the active drawing, with its current name.
pub(crate) fn start(app: &mut PdfCraftApp) {
    let Some((_, id)) = app.active_ids() else { return };
    let Some(path) = app.session.get(id).and_then(|d| d.path.clone()) else {
        app.notify(tl!("Save the drawing first: its notes are kept in a folder beside it.").to_string());
        return;
    };
    let text = Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    app.rename_job = Some(RenameJob { doc: id, text, problem: None });
}

/// The dialog, drawn each frame while it is open.
pub(crate) fn dialog(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(job) = app.rename_job.as_mut() else { return };
    let mut confirm = false;
    let mut cancel = false;
    egui::Window::new(tl!("Rename the drawing")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 80.0]).show(ctx, |ui| {
        ui.label(tl!("The job folder and the links in the notes follow the new name."));
        let edit = ui.add(egui::TextEdit::singleline(&mut job.text).desired_width(260.0).char_limit(200));
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            confirm = true;
        }
        if let Some(p) = &job.problem {
            ui.colored_label(ui.visuals().error_fg_color, p);
        }
        ui.horizontal(|ui| {
            if ui.button(tl!("Rename")).clicked() {
                confirm = true;
            }
            if ui.button(tl!("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if cancel {
        app.rename_job = None;
        return;
    }
    if confirm {
        let Some(job) = app.rename_job.as_ref() else { return };
        let (doc, text) = (job.doc, job.text.clone());
        match rename(app, doc, &text) {
            Ok(()) => app.rename_job = None,
            Err(e) => {
                if let Some(job) = app.rename_job.as_mut() {
                    job.problem = Some(e);
                }
            }
        }
    }
}

/// Rename the drawing `doc` to `new_stem`: the PDF's copy of each note takes the note file's new
/// text, the drawing is saved, the folder and drawing are renamed, and the open document follows.
fn rename(app: &mut PdfCraftApp, doc: DocId, new_stem: &str) -> Result<(), String> {
    if app.active_ids().map(|(_, id)| id) != Some(doc) {
        return Err(tl!("Switch to the drawing first.").to_string());
    }
    let path = app.session.get(doc).and_then(|d| d.path.clone()).ok_or("the drawing has no file")?;
    let pdf = PathBuf::from(&path);
    let plan = pdfcraft_engine::rename::plan(&pdf, new_stem)?;
    let dir = pdf.parent().map(Path::to_path_buf).ok_or("the drawing has no folder")?;

    // Each note's copy in the drawing follows its file's text, which the rename rewrites.
    let mut copies = Vec::new();
    if let Some(d) = app.session.get(doc) {
        let pages: BTreeSet<usize> = d.info.annotations.iter().map(|a| a.page).collect();
        for page in pages {
            for (index, rel) in d.markdown_notes(page) {
                let file = dir.join(&rel);
                if let Some((_, text)) = plan.notes.iter().find(|(f, _)| *f == file) {
                    copies.push((page, index, text.clone()));
                }
            }
        }
    }
    for (page, index, text) in copies {
        if !app.apply_edit(Edit::SetAnnotationContents { page, index, text }) {
            return Err(tl!("The note's copy in the drawing can't be updated.").to_string());
        }
    }

    // The drawing on disk is the saved version before anything is renamed.
    let bytes = app.session.save_bytes(doc).map_err(|e| e.to_string())?;
    crate::editing::write_atomically(&path, &bytes).map_err(|e| e.to_string())?;
    pdfcraft_engine::rename::apply(&plan)?;
    let new_path = plan.new_pdf.to_string_lossy().into_owned();
    app.session.mark_saved(doc, bytes, Some(new_path)).map_err(|e| e.to_string())?;
    // The note editor pointed at the old folder.
    app.note_editor = None;
    app.notify(tl!("Renamed the drawing.").to_string());
    Ok(())
}
