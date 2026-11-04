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
    /// The preview of the workbook link last tapped in the reading view.
    workbook: Option<WorkbookView>,
    /// The paperclip was tapped: the picker opens after this frame.
    pick_workbook: bool,
    /// The images the note shows, loaded from the PDF's folder.
    pictures: Vec<Picture>,
    /// The text changed in edit mode: the pictures are loaded again before the next read.
    pictures_stale: bool,
    /// The size the reader or editor was resized to (`None`: fills the window).
    size: Option<egui::Vec2>,
    /// A `[[file]]` link tapped in the reading view: its note, image or problem.
    linked: Option<Linked>,
    /// The camera was tapped: the photo picker opens after this frame.
    pick_photo: bool,
}

/// The most images one note loads.
const MAX_PICTURES: usize = 16;

/// An image a note shows on its own line (`[[Plan.images/photo.jpg]]`), loaded once.
pub(crate) struct Picture {
    target: String,
    tex: Result<egui::TextureHandle, String>,
}

/// What a tapped `[[file]]` link shows beneath the note.
pub(crate) enum Linked {
    Note { title: String, text: String },
    Image { title: String, tex: egui::TextureHandle },
    Problem { title: String, message: String },
}

/// The image a line names when it is only `[[file.jpg]]` (or `[[file.jpg|shown]]`).
fn picture_target(text: &str) -> Option<String> {
    let inner = text.trim().strip_prefix("[[")?.strip_suffix("]]")?;
    let target = inner.split('|').next()?.trim();
    (!target.is_empty() && !target.contains("]]") && !target.contains("[[") && pdfcraft_engine::comments::is_image(target))
        .then(|| target.to_string())
}

/// The images `text` shows, loaded from `id`'s folder: each once, at most [`MAX_PICTURES`].
fn load_pictures(app: &PdfCraftApp, id: DocId, text: &str) -> Vec<Picture> {
    let (Some(ctx), Some(dir)) = (app.ctx.as_ref(), pdf_dir(app, id)) else { return Vec::new() };
    let mut out: Vec<Picture> = Vec::new();
    for block in markdown::blocks(markdown::strip_frontmatter(text)) {
        let Block::Paragraph(line) = block else { continue };
        let Some(target) = picture_target(&line) else { continue };
        if out.iter().any(|p| p.target == target) {
            continue;
        }
        if out.len() >= MAX_PICTURES {
            break;
        }
        let tex = pdfcraft_engine::comments::resolve(&dir, &target).and_then(|path| texture(ctx, &target, &path));
        out.push(Picture { target, tex });
    }
    out
}

/// Decodes the image at `path` into a texture for the reading view.
fn texture(ctx: &egui::Context, name: &str, path: &Path) -> Result<egui::TextureHandle, String> {
    let (w, h, pixels) = pdfcraft_engine::comments::load_image(path)?;
    let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);
    Ok(ctx.load_texture(format!("linked-{name}"), img, egui::TextureOptions::LINEAR))
}

/// The job's folder beside the PDF: the PDF's name without `.pdf`.
pub(crate) fn job_folder(pdf: &Path) -> Option<PathBuf> {
    let stem = pdf.file_stem()?.to_string_lossy().into_owned();
    (!stem.trim().is_empty()).then(|| pdf.with_file_name(stem))
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
        self.place_note_with(page, at, None);
    }

    /// A markdown note pinned at `at`. With a photo, the photo is kept in `<pdf-name>.images/` and
    /// the note links to it; the note's text is the caption.
    pub(crate) fn place_note_with(&mut self, page: usize, at: [f64; 2], photo: Option<(&str, Vec<u8>)>) {
        let Some((index, id)) = self.active_ids() else { return };
        let Some(pdf) = self.session.get(id).and_then(|d| d.path.clone()).map(PathBuf::from) else {
            self.notify_tr("Save the drawing first: its notes are kept in a folder beside it.");
            return;
        };
        // The photo is kept first, so a failure leaves no note behind.
        let link = match photo {
            Some((name, bytes)) => match pdfcraft_engine::comments::add_photo(&pdf, name, &bytes) {
                Ok(rel) => Some(rel),
                Err(e) => {
                    self.notify_fmt("{e}", &[("e", &e)]);
                    return;
                }
            },
            None => None,
        };
        let author = self.comment_prefs.author.clone();
        // The comment's file goes in `<pdf-name>.comments/`; its text lives there, not in the PDF.
        let made = match pdfcraft_engine::comments::create(&pdf, page, &author, &pdfcraft_engine::comments::stamp_now()) {
            Ok(made) => made,
            Err(e) => {
                self.notify_fmt("{e}", &[("e", &e)]);
                return;
            }
        };
        let (path, text) = (made.path.clone(), made.text.clone());
        // With a photo, its link is added to the text now: the note is unsaved until it is closed.
        let (saved, text) = match &link {
            Some(rel) => (text.clone(), with_photo_link(&text, rel)),
            None => (text.clone(), text),
        };
        let before = self.session.get(id).map(|d| d.markdown_notes(page).len()).unwrap_or(0);
        // A photo from the camera keeps the tool on for the next spot.
        self.views[index].comments.tool_done = link.is_none() || !self.symbols.photo_camera;
        if !self.apply_edit(Edit::AddMarkdownNote { page, at, file: made.rel, author }) {
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
                saved,
                text,
                editing: true,
                sel: (end, end),
                set_sel: Some((end, end)),
                undo: Vec::new(),
                redo: Vec::new(),
                last_snap: 0.0,
                painter: None,
                problem: None,
                workbook: None,
                pick_workbook: false,
                pictures: Vec::new(),
                pictures_stale: false,
                size: None,
                linked: None,
                pick_photo: false,
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
        let in_file = doc.note_in_file(page, index);
        let file = pdf_dir(self, id).and_then(|dir| pdfcraft_engine::comments::resolve(&dir, &rel).ok());
        let loaded = file.as_deref().and_then(read_file);
        // A note kept in its file alone has nothing to show when that file is gone.
        let problem = (in_file && loaded.is_none()).then(|| format!("The note's file {rel} isn't there."));
        let text = loaded.unwrap_or_else(|| saved.clone());
        let pictures = load_pictures(self, id, &text);
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
            problem,
            workbook: None,
            pick_workbook: false,
            pictures,
            pictures_stale: false,
            size: None,
            linked: None,
            pick_photo: false,
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
        // A note kept in its file alone has no copy in the PDF to update.
        let in_file = self.session.get(doc).is_some_and(|d| d.note_in_file(page, index));
        if in_file {
            if let Some(ed) = self.note_editor.as_mut() {
                ed.saved = text;
            }
            return true;
        }
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

/// A workbook link's preview, read from the file when the link is tapped.
pub(crate) struct WorkbookView {
    title: String,
    grid: Result<pdfcraft_engine::workbook::SheetGrid, String>,
}

/// The PDF's folder: the note files and linked workbooks are relative to it.
fn pdf_dir(app: &PdfCraftApp, doc: DocId) -> Option<PathBuf> {
    app.session.get(doc).and_then(|d| d.path.as_deref()).and_then(|p| Path::new(p).parent()).map(Path::to_path_buf)
}

/// The cells a workbook link shows, read from `path` beside the PDF (`dir`).
fn workbook_preview(
    dir: Option<&Path>,
    path: &str,
    sheet: Option<&str>,
    range: Option<&str>,
) -> Result<pdfcraft_engine::workbook::SheetGrid, String> {
    let dir = dir.ok_or("Save the drawing first: a linked workbook is read from its folder.")?;
    if Path::new(path).is_absolute() {
        return Err("A linked workbook must be beside the PDF, not at an absolute path.".into());
    }
    let file = dir.join(path);
    let len = std::fs::metadata(&file).map_err(|_| format!("Can't find {path}."))?.len();
    if len > pdfcraft_engine::workbook::MAX_BYTES as u64 {
        return Err(format!("{path} is too large to show."));
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("Can't read {path}: {e}"))?;
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let book = pdfcraft_engine::workbook::open(&bytes, &name).map_err(|e| e.to_string())?;
    book.preview(sheet.unwrap_or(""), range.unwrap_or("")).map_err(|e| e.to_string())
}

/// The path from `base` to `file`, with `../` where `file` leaves `base`, joined with `/`.
fn relative_to(file: &Path, base: &Path) -> Option<String> {
    let f: Vec<_> = file.components().collect();
    let b: Vec<_> = base.components().collect();
    let common = f.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = (common..b.len()).map(|_| "..".to_string()).collect();
    for c in f.get(common..)? {
        parts.push(c.as_os_str().to_str()?.to_string());
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The reading view's preview of a linked workbook, with a way to close it.
fn workbook_view(ui: &mut egui::Ui, view: &WorkbookView, close: &mut bool) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&view.title).font(theme::semibold(13.0)));
        if ui.small_button(tl!("Close")).clicked() {
            *close = true;
        }
    });
    match &view.grid {
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        Ok(grid) => {
            egui::ScrollArea::horizontal().id_salt("note-workbook").auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("note-workbook-cells").striped(true).show(ui, |ui| {
                    for row in &grid.rows {
                        for cell in row {
                            ui.label(cell);
                        }
                        ui.end_row();
                    }
                });
            });
            if grid.cut {
                ui.label(RichText::new(tl!("Only the first rows and columns are shown.")).weak());
            }
        }
    }
}

/// Read the linked workbook for the reading view (live: read again each time it is tapped).
fn show_workbook(app: &mut PdfCraftApp, path: &str, sheet: Option<&str>, range: Option<&str>) {
    let Some(doc) = app.note_editor.as_ref().map(|ed| ed.doc) else { return };
    let dir = pdf_dir(app, doc);
    let title = match (sheet, range) {
        (Some(s), Some(r)) => format!("{path} · {s}!{r}"),
        (Some(s), None) => format!("{path} · {s}"),
        _ => path.to_string(),
    };
    let grid = workbook_preview(dir.as_deref(), path, sheet, range);
    if let Some(ed) = app.note_editor.as_mut() {
        ed.workbook = Some(WorkbookView { title, grid });
    }
}

impl PdfCraftApp {
    /// The paperclip's picked workbook: a link to it, relative to the PDF's folder, in the note.
    pub(crate) fn insert_workbook_link(&mut self, file: &Path) {
        let Some(doc) = self.note_editor.as_ref().map(|ed| ed.doc) else { return };
        let Some(dir) = pdf_dir(self, doc) else {
            self.notify_tr("Save the drawing first: its notes are kept in a folder beside it.");
            return;
        };
        let Some(rel) = relative_to(file, &dir) else {
            self.notify_tr("That workbook's name can't be linked.");
            return;
        };
        // Spaces and brackets are escaped so the link stays one Markdown link; `%` first.
        let target = rel.replace('%', "%25").replace(' ', "%20").replace('(', "%28").replace(')', "%29");
        let shown = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let now = self.ctx.as_ref().map_or(0.0, |c| c.input(|i| i.time));
        if let Some(ed) = self.note_editor.as_mut() {
            let (s, e) = ed.sel;
            let edit = markdown::link_to(&ed.text, s, e, &shown, &target);
            apply(ed, now, edit);
        }
    }
}

/// The open note, over the drawing.
pub(crate) fn editor(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(ed) = app.note_editor.as_mut() else { return };
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let max_width = (screen.width() - 16.0).max(200.0);
    let max_height = (screen.height() - 32.0).max(200.0);
    let (width, height) = match ed.size {
        Some(v) => (fit(v.x, 200.0, max_width), fit(v.y, 200.0, max_height)),
        None => ((screen.width() - 16.0).clamp(200.0, 900.0), max_height),
    };
    let mut drag = egui::Vec2::ZERO;
    let id = egui::Id::new("revucraft-note");
    let pos = screen.min + vec2(((screen.width() - width) / 2.0).max(8.0), 16.0);
    let area =
        egui::Area::new(id).kind(egui::UiKind::Modal).sense(egui::Sense::hover()).order(egui::Order::Foreground).interactable(true).fixed_pos(pos);
    let now = ctx.input(|i| i.time);
    let mut close = false;
    let mut acts: Vec<ReadAct> = Vec::new();
    let mut pick = false;
    let mut close_view = false;
    let mut close_linked = false;
    let mut pick_photo = false;
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
                    ed.pictures_stale |= !ed.editing;
                }
            });
        });
        if let Some(p) = &ed.problem {
            ui.colored_label(ui.visuals().error_fg_color, p);
        }
        ui.separator();
        if ed.editing {
            toolbar(ui, ed, now);
            pick |= std::mem::take(&mut ed.pick_workbook);
            pick_photo |= std::mem::take(&mut ed.pick_photo);
            ui.add_space(4.0);
            edit_box(ui, ed, now, height - 120.0);
        } else {
            egui::ScrollArea::vertical().id_salt("note-read").max_height((height - 80.0).max(120.0)).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(width - 16.0);
                read_view(ui, &t, markdown::strip_frontmatter(&ed.text), &ed.pictures, &mut acts);
                if let Some(view) = ed.workbook.as_ref() {
                    workbook_view(ui, view, &mut close_view);
                }
                if let Some(linked) = ed.linked.as_ref() {
                    linked_view(ui, &t, linked, &mut close_linked);
                }
            });
            if close_view {
                ed.workbook = None;
            }
            if close_linked {
                ed.linked = None;
            }
        }
        // The grip in the bottom-right corner: drag to resize.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::drag());
            let color = if resp.hovered() || resp.dragged() { t.text } else { t.text_faint };
            for k in [4.0_f32, 8.0, 12.0] {
                let a = rect.right_bottom() - vec2(k, 2.0);
                let b = rect.right_bottom() - vec2(2.0, k);
                ui.painter().line_segment([a, b], egui::Stroke::new(1.0, color));
            }
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            drag += resp.drag_delta();
        });
    });
    if drag != egui::Vec2::ZERO
        && let Some(ed) = app.note_editor.as_mut()
    {
        let v = vec2(width, height) + drag;
        ed.size = Some(vec2(fit(v.x, 200.0, max_width), fit(v.y, 200.0, max_height)));
    }
    if pick {
        app.pick_files(crate::files::FilePurpose::NoteWorkbook, false);
    }
    if std::mem::take(&mut pick_photo) {
        app.pick_files(crate::files::FilePurpose::NotePhoto, false);
    }
    if app.note_editor.as_ref().is_some_and(|ed| ed.pictures_stale && !ed.editing) {
        refresh_pictures(app);
    }
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
            ReadAct::Link(Link::Workbook { path, sheet, range }) => show_workbook(app, &path, sheet.as_deref(), range.as_deref()),
        }
    }
    if close && app.save_note() {
        app.note_editor = None;
    }
}

/// Shows the note or image a `[[file]]` link names, beneath the open note. A link that leaves the
/// PDF's folder, or names a file that is not there, shows a warning in its place.
fn open_linked(app: &mut PdfCraftApp, target: &str) {
    let Some((_, id)) = app.active_ids() else { return };
    let title = target.trim().to_string();
    let linked = match pdf_dir(app, id) {
        None => Linked::Problem { title, message: "Save the drawing first: its linked files are read from the folder beside it.".into() },
        Some(dir) => match pdfcraft_engine::comments::resolve(&dir, target) {
            Err(message) => Linked::Problem { title, message: format!("Broken link: {message}") },
            Ok(path) if !path.is_file() => Linked::Problem { title, message: format!("Broken link: {} isn't in the PDF's folder.", target.trim()) },
            Ok(path) if pdfcraft_engine::comments::is_image(target) => match app.ctx.clone() {
                Some(ctx) => match texture(&ctx, target, &path) {
                    Ok(tex) => Linked::Image { title, tex },
                    Err(message) => Linked::Problem { title, message },
                },
                None => Linked::Problem { title, message: "The image can't be shown yet.".into() },
            },
            Ok(path) => match read_file(&path) {
                Some(text) => Linked::Note { title, text },
                None => Linked::Problem { title, message: format!("{} can't be read.", target.trim()) },
            },
        },
    };
    if let Some(ed) = app.note_editor.as_mut() {
        ed.linked = Some(linked);
    }
}

/// The PDF this note belongs to, when it has been saved.
fn note_pdf_of(app: &PdfCraftApp) -> Option<PathBuf> {
    let doc = app.note_editor.as_ref()?.doc;
    app.session.get(doc).and_then(|d| d.path.as_deref()).map(PathBuf::from)
}

impl PdfCraftApp {
    /// The PDF the open note belongs to.
    pub(crate) fn note_pdf(&self) -> Option<PathBuf> {
        note_pdf_of(self)
    }

    /// A photo was kept (its link, relative to the PDF's folder) or refused (why). A kept photo's
    /// link goes on its own line in the open note, and the note's pictures are loaded again.
    pub(crate) fn photo_done(&mut self, result: Result<String, String>) {
        let rel = match result {
            Ok(rel) => rel,
            Err(e) => {
                self.notify_fmt("{e}", &[("e", &e)]);
                return;
            }
        };
        let now = self.ctx.as_ref().map_or(0.0, |c| c.input(|i| i.time));
        let Some(ed) = self.note_editor.as_mut() else { return };
        let (s, e) = ed.sel;
        let edit = markdown::insert_line(&ed.text, s, e, &format!("[[{rel}]]"));
        apply(ed, now, edit);
        refresh_pictures(self);
    }
}

/// Loads the open note's pictures again, after its text was edited.
fn refresh_pictures(app: &mut PdfCraftApp) {
    let Some(ed) = app.note_editor.as_ref() else { return };
    let (id, text) = (ed.doc, ed.text.clone());
    let pictures = load_pictures(app, id, &text);
    if let Some(ed) = app.note_editor.as_mut() {
        ed.pictures = pictures;
        ed.pictures_stale = false;
    }
}

/// A `[[link]]`: a note or image beside the PDF, or else the markup on the plan whose label,
/// name, mark or note title it names.
fn go_to_markup(app: &mut PdfCraftApp, target: &str) {
    if pdfcraft_engine::comments::is_file_target(target) {
        open_linked(app, target);
        return;
    }
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
                if icon(ui, "paperclip", tl!("Link a workbook beside the PDF")) {
                    ed.pick_workbook = true;
                }
                if icon(ui, "camera", tl!("Add a photo: kept beside the PDF and linked here")) {
                    ed.pick_photo = true;
                }
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

/// `v` kept between `lo` and `hi` (`hi` is raised to `lo` when the window is smaller than that),
/// so a small window never panics.
fn fit(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi.max(lo))
}

/// The note's text as a live preview: every line styled as it reads, with the markers hidden
/// except on the cursor's line (`cursor_line`), where they show dimmed.
fn live_job(text: &str, cursor_line: Option<usize>, t: &Tokens) -> egui::text::LayoutJob {
    use crate::markdown::{Look, line_looks};
    let mut job = egui::text::LayoutJob::default();
    let mut in_code = false;
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            job.append("\n", 0.0, body_format(14.0, t.text));
        }
        let editing = cursor_line == Some(n);
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            job.append(line, 0.0, code_format(t, 13.0, t.text_muted));
            continue;
        }
        if in_code {
            job.append(line, 0.0, code_format(t, 13.0, t.text));
            continue;
        }
        let looks = line_looks(line);
        let mut run = String::new();
        let mut current: Option<Look> = None;
        for (i, ch) in line.chars().enumerate() {
            let look = looks.get(i).copied().unwrap_or_default();
            if current != Some(look) && !run.is_empty() {
                job.append(&run, 0.0, look_format(t, current.unwrap_or_default(), editing));
                run.clear();
            }
            current = Some(look);
            run.push(ch);
        }
        if !run.is_empty() {
            job.append(&run, 0.0, look_format(t, current.unwrap_or_default(), editing));
        }
    }
    job
}

fn body_format(size: f32, color: Color32) -> egui::TextFormat {
    egui::TextFormat { font_id: egui::FontId::proportional(size), color, ..Default::default() }
}

fn code_format(t: &Tokens, size: f32, color: Color32) -> egui::TextFormat {
    egui::TextFormat { font_id: egui::FontId::monospace(size), color, background: t.card, ..Default::default() }
}

/// The format of one run of text: a marker off the cursor's line takes no room; on the cursor's
/// line it shows dimmed.
fn look_format(t: &Tokens, l: crate::markdown::Look, editing: bool) -> egui::TextFormat {
    if l.marker {
        return if editing {
            egui::TextFormat { font_id: egui::FontId::proportional(13.0), color: t.text_muted, ..Default::default() }
        } else {
            egui::TextFormat { font_id: egui::FontId::proportional(0.01), color: Color32::TRANSPARENT, ..Default::default() }
        };
    }
    let size = match l.heading {
        1 => 26.0,
        2 => 21.0,
        3 => 18.0,
        0 => 14.0,
        _ => 15.0,
    };
    let color = if l.quote {
        t.text_muted
    } else if l.bold {
        t.accent
    } else {
        t.text
    };
    if l.code {
        return code_format(t, size - 1.0, color);
    }
    let mut format = egui::TextFormat {
        font_id: egui::FontId::proportional(size),
        color,
        italics: l.italic,
        background: if l.highlight { Color32::from_rgba_unmultiplied(255, 214, 80, 90) } else { Color32::TRANSPARENT },
        ..Default::default()
    };
    if l.link {
        format.color = t.accent;
        format.underline = egui::Stroke::new(1.0, t.accent);
    }
    if l.strike {
        format.strikethrough = egui::Stroke::new(1.0, color);
    }
    format
}

/// The note's text with a link to its photo on a line of its own, after the rest.
fn with_photo_link(text: &str, rel: &str) -> String {
    format!("{}\n\n[[{rel}]]\n", text.trim_end())
}

/// The line (0-based) holding the character at `index`.
fn line_of(text: &str, index: usize) -> usize {
    text.chars().take(index).filter(|c| *c == '\n').count()
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
    // The cursor's line keeps its markers; every other line shows as it reads.
    let cursor_line =
        egui::TextEdit::load_state(ui.ctx(), id).and_then(|st| st.cursor.char_range()).map(|r| line_of(&ed.text, usize::from(r.primary.index)));
    let t = Tokens::get(ui.ctx());
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = live_job(buf.as_str(), cursor_line, &t);
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    egui::ScrollArea::vertical().id_salt("note-edit").max_height(height.max(120.0)).auto_shrink([false, true]).show(ui, |ui| {
        let out = egui::TextEdit::multiline(&mut ed.text).id(id).layouter(&mut layouter).desired_width(f32::INFINITY).desired_rows(12).show(ui);
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
fn read_view(ui: &mut egui::Ui, t: &Tokens, text: &str, pictures: &[Picture], acts: &mut Vec<ReadAct>) {
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
            Block::Paragraph(text) => match picture_target(&text) {
                Some(target) => picture(ui, t, &target, pictures),
                None => line(ui, t, &text, 14.0, false, acts),
            },
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

/// An image on its own line, scaled to the width of the note.
fn picture(ui: &mut egui::Ui, t: &Tokens, target: &str, pictures: &[Picture]) {
    match pictures.iter().find(|p| p.target == target).map(|p| &p.tex) {
        Some(Ok(tex)) => {
            let size = tex.size_vec2();
            let w = size.x.min(ui.available_width()).clamp(1.0, 480.0);
            let h = if size.x > 0.0 { size.y * w / size.x } else { 0.0 };
            ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), vec2(w, h))));
        }
        Some(Err(message)) => {
            ui.colored_label(ui.visuals().error_fg_color, format!("[{target}] {message}"));
        }
        None => {
            ui.label(RichText::new(format!("[{target}]")).color(t.text_muted));
        }
    }
}

/// What a tapped `[[file]]` link shows: a note's text, an image, or the warning why it cannot.
fn linked_view(ui: &mut egui::Ui, t: &Tokens, linked: &Linked, close: &mut bool) {
    ui.separator();
    ui.horizontal(|ui| {
        let title = match linked {
            Linked::Note { title, .. } | Linked::Image { title, .. } | Linked::Problem { title, .. } => title,
        };
        ui.label(RichText::new(title).font(theme::semibold(14.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::widgets::pill_button(ui, tl!("Close"), false).clicked() {
                *close = true;
            }
        });
    });
    match linked {
        // Links inside a linked note are not followed from here.
        Linked::Note { text, .. } => read_view(ui, t, text, &[], &mut Vec::new()),
        Linked::Image { tex, .. } => {
            let size = tex.size_vec2();
            let w = size.x.min(ui.available_width()).max(1.0);
            let h = if size.x > 0.0 { size.y * w / size.x } else { 0.0 };
            ui.add(egui::Image::new(egui::load::SizedTexture::new(tex.id(), vec2(w, h))));
        }
        Linked::Problem { message, .. } => {
            ui.colored_label(ui.visuals().error_fg_color, message);
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
    fn a_line_that_is_only_an_image_link_shows_the_picture() {
        assert_eq!(picture_target("[[Plan.images/Photo 1.jpg]]"), Some("Plan.images/Photo 1.jpg".into()));
        assert_eq!(picture_target("  [[photo.png|Shown]] "), Some("photo.png".into()));
        assert_eq!(picture_target("[[note.md]]"), None, "a note is a link, not a picture");
        assert_eq!(picture_target("see [[photo.jpg]] here"), None);
        assert_eq!(picture_target("[[a.jpg]] [[b.jpg]]"), None, "two links on a line are not one picture");
        assert_eq!(picture_target("[[]]"), None);
        assert_eq!(picture_target("[[.jpg"), None);
    }

    #[test]
    fn a_note_file_is_read_back_or_none() {
        let dir = std::env::temp_dir().join(format!("revucraft-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Note 1.md"), "x").unwrap();
        assert_eq!(read_file(&dir.join("Note 1.md")).as_deref(), Some("x"));
        assert_eq!(read_file(&dir.join("missing.md")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod size_tests {
    use super::fit;

    /// A resized note stays inside the window, and a window smaller than the minimum still
    /// clamps without panicking.
    #[test]
    fn fit_keeps_the_size_between_the_bounds() {
        assert_eq!(fit(5000.0, 200.0, 900.0), 900.0);
        assert_eq!(fit(10.0, 200.0, 900.0), 200.0);
        assert_eq!(fit(500.0, 200.0, 900.0), 500.0);
        assert_eq!(fit(500.0, 200.0, 100.0), 200.0);
    }
}

#[cfg(test)]
mod live_preview_tests {
    use super::*;
    use crate::theme::ThemeKind;

    /// The live preview hides a note's markers on every line but the cursor's. The cursor's line
    /// shows them, and that is the only line that does.
    #[test]
    fn live_preview_hides_markers_away_from_the_cursor() {
        let t = Tokens::for_kind(ThemeKind::Light);
        let text = "**bold**\nplain";
        let hidden = |cursor: Option<usize>| -> usize {
            let job = live_job(text, cursor, &t);
            job.sections
                .iter()
                .filter(|sec| sec.format.font_id.size < 1.0)
                .map(|sec| usize::from(sec.byte_range.end) - usize::from(sec.byte_range.start))
                .sum()
        };
        // Four marker characters (two `**` pairs) on the first line.
        assert_eq!(hidden(None), 4);
        assert_eq!(hidden(Some(1)), 4, "the cursor is on the second line");
        assert_eq!(hidden(Some(0)), 0, "the cursor's line shows its markers");
    }

    /// The line a character index falls on.
    #[test]
    fn line_of_counts_newlines_before_the_index() {
        let text = "a\nbc\nd";
        assert_eq!(line_of(text, 0), 0);
        assert_eq!(line_of(text, 2), 1);
        assert_eq!(line_of(text, 5), 2);
        assert_eq!(line_of(text, 99), 2);
    }
}

#[cfg(test)]
mod photo_pin_tests {
    use super::with_photo_link;

    /// A photo pin's note keeps its text, and the photo link is a paragraph of its own, which is
    /// what the reading view needs to draw the picture.
    #[test]
    fn photo_link_is_its_own_paragraph_after_the_text() {
        let text = "---\ntitle: Rack A\n---\n*2026-11-04 · steve*\n";
        let out = with_photo_link(text, "Smith.images/IMG_1.jpg");
        assert!(out.starts_with("---\ntitle: Rack A\n---\n"));
        let blocks = crate::markdown::blocks(&out);
        assert!(blocks.iter().any(|b| matches!(b, crate::markdown::Block::Paragraph(l) if l == "[[Smith.images/IMG_1.jpg]]")), "{blocks:?}");
    }
}
