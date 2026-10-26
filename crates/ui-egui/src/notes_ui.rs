//! RevuCraft's Markdown notes: placed like photo pins, kept as `.md` files in the job's folder
//! beside the PDF (`Smith Residence.pdf` → `Smith Residence/notes/Note 1.md`), with a copy of
//! the text in the PDF's sticky note so the PDF stands alone when sent. The file is the master:
//! opened, a note is read from it when it is there; closed, both are written.
//!
//! The editor opens over the drawing: a reading view first (headings, lists, tickable
//! checklists, `#tags`, links), and the text with a toolbar after Obsidian's Editing Toolbar
//! once Edit is pressed.

use std::path::{Path, PathBuf};

use egui::{Color32, RichText, vec2};
use pdfcraft_engine::{DocId, Edit};

use crate::markdown::{self, Block, ItemKind, Link, ListKind};
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons};

/// Snapshots kept for the editor's own Undo.
const MAX_HISTORY: usize = 200;
/// Typing this long after the last snapshot starts a new one (seconds).
const SNAP_AFTER: f64 = 1.0;
/// The largest note file read (bytes).
const MAX_FILE: u64 = 4 << 20;

/// An open note.
pub(crate) struct NoteEditor {
    doc: DocId,
    page: usize,
    index: usize,
    /// The note's file, when the PDF has a folder to keep it in.
    file: Option<PathBuf>,
    text: String,
    /// What the PDF holds now.
    saved: String,
    editing: bool,
    /// The selection, in characters, as the text box last had it.
    sel: (usize, usize),
    /// A selection for the text box to take next frame.
    set_sel: Option<(usize, usize)>,
    undo: Vec<String>,
    redo: Vec<String>,
    last_snap: f64,
    /// The format painter's copied markers, waiting for the next selection.
    painter: Option<Vec<&'static str>>,
    problem: Option<String>,
}

/// The job's folder beside the PDF: the PDF's name without `.pdf`.
pub(crate) fn job_folder(pdf: &Path) -> Option<PathBuf> {
    let stem = pdf.file_stem()?.to_string_lossy().into_owned();
    (!stem.trim().is_empty()).then(|| pdf.with_file_name(stem))
}

/// A new note's file name in `dir`: `Note 1.md`, `Note 2.md`…, the first free one.
fn free_name(dir: &Path) -> Option<(u32, String)> {
    (1..10_000u32).map(|n| (n, format!("Note {n}.md"))).find(|(_, name)| !dir.join(name).exists())
}

/// A new note's first lines: its title and who wrote it when.
pub(crate) fn starter(n: u32, stamp: &str, author: &str) -> String {
    let who = [stamp.trim(), author.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    if who.is_empty() { format!("# Note {n}\n\n") } else { format!("# Note {n}\n\n*{who}*\n\n") }
}

fn read_file(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

impl PdfCraftApp {
    /// The Note tool's tap: a new note file in the job's folder, its sticky note at `at`, and
    /// the editor open on it.
    pub(crate) fn place_note(&mut self, page: usize, at: [f64; 2]) {
        let Some((index, id)) = self.active_ids() else { return };
        let Some(pdf) = self.session.get(id).and_then(|d| d.path.clone()).map(PathBuf::from) else {
            self.notify_tr("Save the drawing first: its notes are kept in a folder beside it.");
            return;
        };
        let (Some(job), Some(pdf_dir)) = (job_folder(&pdf), pdf.parent().map(Path::to_path_buf)) else { return };
        let dir = job.join("notes");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.notify_fmt("Couldn't make the notes folder {name}: {e}", &[("name", &dir.display().to_string()), ("e", &e.to_string())]);
            return;
        }
        let Some((n, name)) = free_name(&dir) else { return };
        let author = self.comment_prefs.author.clone();
        let text = starter(n, &pdfcraft_engine::local_stamp(), &author);
        let path = dir.join(&name);
        if let Err(e) = std::fs::write(&path, &text) {
            self.notify_fmt("Couldn't write {name}: {e}", &[("name", &path.display().to_string()), ("e", &e.to_string())]);
            return;
        }
        let rel = path.strip_prefix(&pdf_dir).map_or_else(|_| path.display().to_string(), |r| r.to_string_lossy().replace('\\', "/"));
        let before = self.session.get(id).map(|d| d.markdown_notes(page).len()).unwrap_or(0);
        self.views[index].comments.tool_done = true;
        if !self.apply_edit(Edit::AddMarkdownNote { page, at, text: text.clone(), file: rel, author }) {
            return;
        }
        let notes = self.session.get(id).map(|d| d.markdown_notes(page)).unwrap_or_default();
        if notes.len() > before
            && let Some((i, _)) = notes.iter().max_by_key(|(i, _)| *i)
        {
            let end = text.chars().count();
            self.note_editor = Some(NoteEditor {
                doc: id,
                page,
                index: *i,
                file: Some(path),
                saved: text.clone(),
                text,
                editing: true,
                sel: (end, end),
                set_sel: Some((end, end)),
                undo: Vec::new(),
                redo: Vec::new(),
                last_snap: 0.0,
                painter: None,
                problem: None,
            });
        }
    }

    /// Open the note at `(page, index)`, when it is a Markdown note: from its file when that is
    /// there, else from the copy in the PDF.
    pub(crate) fn open_note(&mut self, page: usize, index: usize) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let Some((_, rel)) = doc.markdown_notes(page).into_iter().find(|(i, _)| *i == index) else { return };
        let saved = doc.info.annotations.iter().find(|a| a.page == page && a.index == index).and_then(|a| a.contents.clone()).unwrap_or_default();
        let file = doc.path.as_ref().and_then(|p| Path::new(p).parent().map(|d| d.join(&rel)));
        let text = file.as_deref().and_then(read_file).unwrap_or_else(|| saved.clone());
        self.note_editor = Some(NoteEditor {
            doc: id,
            page,
            index,
            file,
            text,
            saved,
            editing: false,
            sel: (0, 0),
            set_sel: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_snap: 0.0,
            painter: None,
            problem: None,
        });
    }

    /// Write the open note: its file (made again if it went missing) and, when it changed, the
    /// copy in the PDF. Returns whether both are written.
    fn save_note(&mut self) -> bool {
        let Some(ed) = self.note_editor.as_mut() else { return true };
        if let Some(path) = &ed.file {
            let write = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(path, &ed.text));
            if let Err(e) = write {
                ed.problem = Some(format!("{} {e}", tl!("The note's file couldn't be written:")));
                return false;
            }
        }
        // The PDF's copy is kept without the file's trailing newline.
        if ed.text.trim_end() == ed.saved.trim_end() {
            return true;
        }
        let (doc, page, index, text) = (ed.doc, ed.page, ed.index, ed.text.clone());
        // Another tab in front: the file holds the note; the PDF's copy is brought up to date
        // the next time the note is opened there.
        if self.active_ids().map(|(_, id)| id) != Some(doc) {
            return true;
        }
        if self.apply_edit(Edit::SetAnnotationContents { page, index, text: text.clone() }) {
            if let Some(ed) = self.note_editor.as_mut() {
                ed.saved = text;
            }
            true
        } else {
            false
        }
    }
}

/// What the reading view asks for.
enum ReadAct {
    Toggle(usize),
    Link(Link),
}

/// The open note, over the drawing.
pub(crate) fn editor(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(ed) = app.note_editor.as_mut() else { return };
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let width = (screen.width() - 16.0).clamp(200.0, 900.0);
    let height = (screen.height() - 32.0).max(200.0);
    let id = egui::Id::new("revucraft-note");
    let pos = screen.min + vec2(((screen.width() - width) / 2.0).max(8.0), 16.0);
    let area =
        egui::Area::new(id).kind(egui::UiKind::Modal).sense(egui::Sense::hover()).order(egui::Order::Foreground).interactable(true).fixed_pos(pos);
    let now = ctx.input(|i| i.time);
    let mut close = false;
    let mut acts: Vec<ReadAct> = Vec::new();
    let modal = egui::Modal::new(id).area(area).show(ctx, |ui| {
        ui.set_width(width);
        ui.set_max_width(width);
        ui.set_max_height(height);
        // Title and the buttons that switch between reading and editing.
        ui.horizontal(|ui| {
            let title = markdown::title(&ed.text);
            ui.label(RichText::new(if title.is_empty() { tl!("Note").to_string() } else { title }).font(theme::semibold(16.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::widgets::pill_button(ui, tl!("Done"), true).clicked() {
                    close = true;
                }
                let (label, tip) =
                    if ed.editing { (tl!("Read"), tl!("Show the note formatted")) } else { (tl!("Edit"), tl!("Edit the note's text")) };
                if crate::widgets::pill_button(ui, label, false).on_hover_text(tip).clicked() {
                    ed.editing = !ed.editing;
                }
            });
        });
        if let Some(p) = &ed.problem {
            ui.colored_label(ui.visuals().error_fg_color, p);
        }
        ui.separator();
        if ed.editing {
            toolbar(ui, ed, now);
            ui.add_space(4.0);
            edit_box(ui, ed, now, height - 120.0);
        } else {
            egui::ScrollArea::vertical().id_salt("note-read").max_height((height - 80.0).max(120.0)).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(width - 16.0);
                read_view(ui, &t, &ed.text, &mut acts);
            });
        }
    });
    if modal.should_close() {
        close = true;
    }
    for act in acts {
        match act {
            ReadAct::Toggle(line) => {
                if let Some(ed) = app.note_editor.as_mut() {
                    ed.text = markdown::toggle_task(&ed.text, line);
                }
                app.save_note();
            }
            ReadAct::Link(Link::Web(url)) => app.request_document_url(&url, crate::LinkOrigin::Link),
            ReadAct::Link(Link::Markup(target)) => go_to_markup(app, &target),
        }
    }
    if close && app.save_note() {
        app.note_editor = None;
    }
}

/// A `[[link]]`: the markup on the plan whose label, name, mark or note title it names.
fn go_to_markup(app: &mut PdfCraftApp, target: &str) {
    let Some((index, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let want = target.trim().to_lowercase();
    let found = doc.info.annotations.iter().filter(|a| a.in_reply_to.is_none()).find(|a| {
        let contents = a.contents.as_deref().unwrap_or_default();
        let first = contents.lines().next().unwrap_or_default().trim().to_lowercase();
        first == want || markdown::title(contents).to_lowercase() == want || a.symbol.as_deref().is_some_and(|s| s.to_lowercase() == want)
    });
    let found = found.map(|a| (a.page, a.index)).or_else(|| {
        let names = doc.measurements().map(|l| l.measurements).unwrap_or_default();
        names.into_iter().find(|m| m.label.to_lowercase() == want || m.name.to_lowercase() == want).map(|m| (m.page, m.index))
    });
    match found {
        Some((page, i)) => {
            let view = &mut app.views[index];
            view.go_to_page(page);
            view.comments.select_one(page, i);
            view.comments.reveal = true;
            app.note_editor = None;
        }
        None => app.notify_fmt("Nothing on the plan is called {name}.", &[("name", target)]),
    }
}

fn snapshot(ed: &mut NoteEditor, now: f64, force: bool) {
    if force || now - ed.last_snap > SNAP_AFTER {
        if ed.undo.last() != Some(&ed.text) {
            ed.undo.push(ed.text.clone());
            if ed.undo.len() > MAX_HISTORY {
                ed.undo.remove(0);
            }
        }
        ed.redo.clear();
        ed.last_snap = now;
    }
}

fn apply(ed: &mut NoteEditor, now: f64, e: markdown::Edit) {
    snapshot(ed, now, true);
    ed.text = e.text;
    ed.set_sel = Some((e.start, e.end));
}

/// The toolbar, after Obsidian's Editing Toolbar.
fn toolbar(ui: &mut egui::Ui, ed: &mut NoteEditor, now: f64) {
    let (s, e) = ed.sel;
    egui::ScrollArea::horizontal().id_salt("note-toolbar").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let icon = |ui: &mut egui::Ui, name: &str, tip: &str| icons::button(ui, name, 30.0, false, tip).clicked();
                let text = |ui: &mut egui::Ui, label: &str, tip: &str| {
                    ui.add(egui::Button::new(RichText::new(label).font(theme::semibold(13.0))).min_size(vec2(30.0, 30.0)))
                        .on_hover_text(tip)
                        .clicked()
                };
                if icon(ui, "undo-2", tl!("Undo"))
                    && let Some(prev) = ed.undo.pop()
                {
                    ed.redo.push(std::mem::replace(&mut ed.text, prev));
                }
                if icon(ui, "redo-2", tl!("Redo"))
                    && let Some(next) = ed.redo.pop()
                {
                    ed.undo.push(std::mem::replace(&mut ed.text, next));
                }
                let painting = ed.painter.is_some();
                if icons::button(ui, "brush", 30.0, painting, tl!("Format painter: copy the selection's formatting, then select where it goes"))
                    .clicked()
                {
                    ed.painter = if painting { None } else { Some(markdown::marks_around(&ed.text, s, e)) };
                }
                if icon(ui, "eraser", tl!("Clear formatting")) {
                    let edit = markdown::clear(&ed.text, s, e);
                    apply(ed, now, edit);
                }
                ui.separator();
                if text(ui, "H2", tl!("Heading 2")) {
                    let edit = markdown::set_heading(&ed.text, s, e, 2);
                    apply(ed, now, edit);
                }
                if text(ui, "H3", tl!("Heading 3")) {
                    let edit = markdown::set_heading(&ed.text, s, e, 3);
                    apply(ed, now, edit);
                }
                ui.menu_button(RichText::new("Hn ▾").font(theme::semibold(13.0)), |ui| {
                    for level in 1..=6u8 {
                        if ui.button(format!("H{level}")).clicked() {
                            let edit = markdown::set_heading(&ed.text, s, e, level);
                            apply(ed, now, edit);
                            ui.close();
                        }
                    }
                    if ui.button(tl!("Plain text")).clicked() {
                        let edit = markdown::set_heading(&ed.text, s, e, 0);
                        apply(ed, now, edit);
                        ui.close();
                    }
                });
                ui.separator();
                for (name, marker, tip) in [
                    ("bold", markdown::BOLD, tl!("Bold")),
                    ("italic", markdown::ITALIC, tl!("Italic")),
                    ("strikethrough", markdown::STRIKE, tl!("Strikethrough")),
                ] {
                    if icon(ui, name, tip) {
                        let edit = markdown::wrap(&ed.text, s, e, marker);
                        apply(ed, now, edit);
                    }
                }
                ui.menu_button(RichText::new("</> ▾").font(theme::semibold(13.0)), |ui| {
                    if ui.button(tl!("Inline code")).clicked() {
                        let edit = markdown::wrap(&ed.text, s, e, markdown::CODE);
                        apply(ed, now, edit);
                        ui.close();
                    }
                    if ui.button(tl!("Code block")).clicked() {
                        let edit = markdown::code_block(&ed.text, s, e);
                        apply(ed, now, edit);
                        ui.close();
                    }
                });
                if icon(ui, "highlighter", tl!("Highlight")) {
                    let edit = markdown::wrap(&ed.text, s, e, markdown::HIGHLIGHT);
                    apply(ed, now, edit);
                }
                ui.menu_button(RichText::new("🔗 ▾").font(theme::semibold(13.0)), |ui| {
                    if ui.button(tl!("Web link")).clicked() {
                        let edit = markdown::link(&ed.text, s, e, false);
                        apply(ed, now, edit);
                        ui.close();
                    }
                    if ui.button(tl!("Link to a markup on the plan ([[…]])")).clicked() {
                        let edit = markdown::link(&ed.text, s, e, true);
                        apply(ed, now, edit);
                        ui.close();
                    }
                });
                ui.separator();
                if icon(ui, "list-checks", tl!("Checklist")) {
                    let edit = markdown::toggle_list(&ed.text, s, e, ListKind::Task);
                    apply(ed, now, edit);
                }
                ui.menu_button(RichText::new("☰ ▾").font(theme::semibold(13.0)), |ui| {
                    for (label, kind) in [(tl!("Bulleted list"), ListKind::Bullet), (tl!("Numbered list"), ListKind::Number)] {
                        if ui.button(label).clicked() {
                            let edit = markdown::toggle_list(&ed.text, s, e, kind);
                            apply(ed, now, edit);
                            ui.close();
                        }
                    }
                    if ui.button(tl!("Indent")).clicked() {
                        let edit = markdown::indent(&ed.text, s, e, false);
                        apply(ed, now, edit);
                        ui.close();
                    }
                    if ui.button(tl!("Outdent")).clicked() {
                        let edit = markdown::indent(&ed.text, s, e, true);
                        apply(ed, now, edit);
                        ui.close();
                    }
                });
            });
        },
    );
}

/// The note's text, being typed.
fn edit_box(ui: &mut egui::Ui, ed: &mut NoteEditor, now: f64, height: f32) {
    let id = egui::Id::new("revucraft-note-text");
    if let Some((s, e)) = ed.set_sel.take() {
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(s), egui::text::CCursor::new(e))));
        state.store(ui.ctx(), id);
        ui.ctx().memory_mut(|m| m.request_focus(id));
    }
    let before = ed.text.clone();
    egui::ScrollArea::vertical().id_salt("note-edit").max_height(height.max(120.0)).auto_shrink([false, true]).show(ui, |ui| {
        let out =
            egui::TextEdit::multiline(&mut ed.text).id(id).desired_width(f32::INFINITY).desired_rows(12).font(egui::TextStyle::Monospace).show(ui);
        if let Some(range) = out.cursor_range {
            let r = range.as_sorted_char_range();
            ed.sel = (r.start.into(), r.end.into());
        }
    });
    if ed.text != before {
        // Typing: a new undo step after a pause.
        let typed = std::mem::replace(&mut ed.text, before);
        snapshot(ed, now, false);
        ed.text = typed;
    }
    // The format painter: the next selection takes the copied formatting.
    if let Some(marks) = ed.painter.clone()
        && ed.sel.0 != ed.sel.1
        && ui.input(|i| i.pointer.any_released())
    {
        let edit = markdown::apply_marks(&ed.text, ed.sel.0, ed.sel.1, &marks);
        apply(ed, now, edit);
        ed.painter = None;
    }
}

/// The note, formatted.
fn read_view(ui: &mut egui::Ui, t: &Tokens, text: &str, acts: &mut Vec<ReadAct>) {
    let blocks = markdown::blocks(text);
    if blocks.is_empty() {
        ui.label(RichText::new(tl!("This note is empty. Press Edit to write it.")).color(t.text_muted));
    }
    for b in blocks {
        match b {
            Block::Heading { level, text } => {
                let size = match level {
                    1 => 22.0,
                    2 => 19.0,
                    3 => 17.0,
                    _ => 15.0,
                };
                ui.add_space(4.0);
                line(ui, t, &text, size, true, acts);
            }
            Block::Paragraph(text) => line(ui, t, &text, 14.0, false, acts),
            Block::Quote(text) => {
                egui::Frame::NONE
                    .inner_margin(egui::Margin { left: 10, right: 0, top: 2, bottom: 2 })
                    .stroke(egui::Stroke::new(1.0, t.divider))
                    .show(ui, |ui| {
                        line(ui, t, &text, 14.0, false, acts);
                    });
            }
            Block::Code(code) => {
                egui::Frame::NONE.fill(t.card).corner_radius(egui::CornerRadius::same(4)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                    ui.label(RichText::new(code).monospace());
                });
            }
            Block::Rule => {
                ui.separator();
            }
            Block::Item { indent, kind, text, line: at } => {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(18.0 * indent.min(8) as f32);
                    match kind {
                        ItemKind::Bullet => {
                            ui.label("•");
                        }
                        ItemKind::Number(n) => {
                            ui.label(format!("{n}."));
                        }
                        ItemKind::Task(done) => {
                            let mut on = done;
                            if ui.add(crate::widgets::checkbox(&mut on, "")).clicked() {
                                acts.push(ReadAct::Toggle(at));
                            }
                        }
                    }
                    spans(ui, t, &text, 14.0, false, acts, matches!(kind, ItemKind::Task(true)));
                });
            }
        }
    }
}

fn line(ui: &mut egui::Ui, t: &Tokens, text: &str, size: f32, heading: bool, acts: &mut Vec<ReadAct>) {
    ui.horizontal_wrapped(|ui| spans(ui, t, text, size, heading, acts, false));
}

fn spans(ui: &mut egui::Ui, t: &Tokens, text: &str, size: f32, heading: bool, acts: &mut Vec<ReadAct>, done: bool) {
    ui.spacing_mut().item_spacing.x = 0.0;
    for s in markdown::spans(text) {
        let font = if s.bold || heading { theme::semibold(size) } else { theme::regular(size) };
        let mut rich = RichText::new(&s.text).font(font);
        if s.italic {
            rich = rich.italics();
        }
        if s.strike || done {
            rich = rich.strikethrough();
        }
        if s.code {
            rich = rich.code();
        }
        if s.highlight {
            rich = rich.background_color(Color32::from_rgba_unmultiplied(250, 214, 60, 110));
        }
        if s.tag {
            rich = rich.color(t.accent).background_color(t.accent.gamma_multiply(0.15));
        }
        match s.link {
            Some(link) => {
                if ui.link(rich).clicked() {
                    acts.push(ReadAct::Link(link));
                }
            }
            None => {
                ui.label(rich);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_lives_in_the_job_folder_beside_its_pdf() {
        let pdf = Path::new("/jobs/Smith Residence.pdf");
        assert_eq!(job_folder(pdf), Some(PathBuf::from("/jobs/Smith Residence")));
        assert_eq!(job_folder(Path::new("/")), None);
        assert_eq!(starter(3, "2026-10-26 13:45", "Steve"), "# Note 3\n\n*2026-10-26 13:45 · Steve*\n\n");
        assert_eq!(starter(1, "", ""), "# Note 1\n\n");
        let dir = std::env::temp_dir().join(format!("revucraft-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_name(&dir).map(|f| f.1), Some("Note 1.md".to_string()));
        std::fs::write(dir.join("Note 1.md"), "x").unwrap();
        assert_eq!(free_name(&dir).map(|f| f.1), Some("Note 2.md".to_string()));
        assert_eq!(read_file(&dir.join("Note 1.md")).as_deref(), Some("x"));
        assert_eq!(read_file(&dir.join("missing.md")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
