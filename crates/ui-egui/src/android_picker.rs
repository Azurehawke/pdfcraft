//! Android: a file browser drawn in the app, standing in for the native panels rfd shows elsewhere.
//!
//! Android's own picker (the Storage Access Framework) answers with `content://` URIs;
//! PdfCraft opens and saves plain paths. So the Android build asks for "All files access"
//! (MANAGE_EXTERNAL_STORAGE, granted once in Settings) and browses shared storage itself. For
//! Google Drive and other document apps, a button hands the choice to Android's picker
//! ([`AndroidDocuments`], from the shell), which answers with the path of a local copy that the
//! shell keeps in step with the document. Every pick still goes through `PdfCraftApp::ask`, and the answer
//! arrives on a later frame like a native pick, so nothing else in the app changes.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::files::PickTarget;
use crate::pickers::{Ask, Then};

/// Shared storage ("Internal storage" in Android's Files app).
const SHARED: &str = "/storage/emulated/0";
/// Folders listed at most: a folder is still untrusted input.
const MAX_ENTRIES: usize = 5000;
/// Rows big enough for a finger as well as the S Pen.
const ROW_HEIGHT: f32 = 40.0;

/// The folder the last picker ended in, where the next one starts.
static LAST_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// All files access, from the Android shell (it needs JNI, which this crate can't use).
#[derive(Clone, Copy, Debug)]
pub struct AndroidFileAccess {
    /// Whether the user has turned it on.
    pub granted: fn() -> bool,
    /// Open the Settings screen where it is turned on; the error says why it didn't open.
    pub request: fn() -> Result<(), String>,
}

static FILE_ACCESS: OnceLock<AndroidFileAccess> = OnceLock::new();

/// Called once by the Android shell at startup.
pub fn set_android_file_access(access: AndroidFileAccess) {
    let _ = FILE_ACCESS.set(access);
}

/// What to ask Android's own document picker (Google Drive and other document apps).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemPick {
    /// Existing documents of these MIME types.
    Open { multiple: bool, mime_types: Vec<String> },
    /// A new document, with a suggested name.
    Save { name: String, mime_type: String },
}

/// Android's document picker, from the Android shell. The shell keeps a copy of each chosen
/// document on the device and answers with its path; saving to that path updates the document
/// in its app.
#[derive(Clone, Copy, Debug)]
pub struct AndroidDocuments {
    /// Show the system picker; the error says why it couldn't.
    pub start: fn(SystemPick) -> Result<(), String>,
    /// The answer once there is one: the local paths, or none when cancelled. Polled each frame
    /// while waiting.
    pub poll: fn() -> Option<Result<Vec<PathBuf>, String>>,
}

static DOCUMENTS: OnceLock<AndroidDocuments> = OnceLock::new();

/// Called once by the Android shell at startup.
pub fn set_android_documents(documents: AndroidDocuments) {
    let _ = DOCUMENTS.set(documents);
}

/// A MIME type for a file name's extension, for the system picker.
fn mime_for(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "tif" | "tiff" => "image/tiff",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "xml" | "xfdf" => "application/xml",
        "json" => "application/json",
        "csv" => "text/csv",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "rtf" => "application/rtf",
        "fdf" => "application/vnd.fdf",
        _ => "application/octet-stream",
    }
}

/// Whether the app may browse shared storage (true when nobody can tell).
fn access_granted() -> bool {
    FILE_ACCESS.get().is_none_or(|a| (a.granted)())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    File,
    Files,
    Folder,
    Save,
}

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    path: PathBuf,
    dir: bool,
}

/// One open picker: what it asks for, where it is, and who gets the answer.
pub(crate) struct Browser {
    mode: Mode,
    title: String,
    filters: Vec<(String, Vec<String>)>,
    /// Index into `filters`; `filters.len()` means all files.
    filter: usize,
    dir: PathBuf,
    entries: Vec<Entry>,
    /// Why `dir` couldn't be listed.
    error: Option<String>,
    selected: Vec<PathBuf>,
    /// The name typed for Save.
    name: String,
    /// Save: the existing file the user is asked to replace.
    confirm_replace: Option<PathBuf>,
    new_folder: Option<String>,
    /// All files access was on when last checked (checked every second until it is).
    access: bool,
    /// What happened when the user last asked for access (shown under the button).
    access_note: Option<String>,
    /// Android's document picker is showing (or its document is being fetched).
    system_waiting: bool,
    /// Why the system picker didn't work, shown under its button.
    system_note: Option<String>,
    target: Option<PickTarget>,
    then: Then,
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser").field("mode", &self.mode).field("dir", &self.dir).field("selected", &self.selected).finish_non_exhaustive()
    }
}

impl Browser {
    pub(crate) fn new(ask: Ask, target: Option<PickTarget>, then: Then) -> Self {
        let (mode, dialog) = match ask {
            Ask::File(d) => (Mode::File, d),
            Ask::Files(d) => (Mode::Files, d),
            Ask::Folder(d) => (Mode::Folder, d),
            Ask::Save(d) => (Mode::Save, d),
        };
        let title = dialog.title.clone().unwrap_or_else(|| {
            match mode {
                Mode::File => tl!("Open"),
                Mode::Files => tl!("Choose files"),
                Mode::Folder => tl!("Choose a folder"),
                Mode::Save => tl!("Save As"),
            }
            .to_string()
        });
        // The folder the app asked for (the document's own for Save As), else where the last
        // picker ended, else Documents.
        let last = LAST_DIR.lock().unwrap_or_else(PoisonError::into_inner).clone();
        // A document from Google Drive is a copy in the app's private folder: Save As doesn't
        // start there, where nothing else can see the file.
        let private = std::env::var_os("HOME").map(PathBuf::from);
        let asked = dialog.directory.clone().filter(|d| private.as_ref().is_none_or(|h| !d.starts_with(h)));
        let start = asked.into_iter().chain(last).find(|d| d.is_dir()).unwrap_or_else(default_dir);
        let mut browser = Self {
            mode,
            title,
            filters: dialog.filters,
            filter: 0,
            dir: start.clone(),
            entries: Vec::new(),
            error: None,
            selected: Vec::new(),
            name: dialog.file_name.unwrap_or_default(),
            confirm_replace: None,
            new_folder: None,
            access: access_granted(),
            access_note: None,
            system_waiting: false,
            system_note: None,
            target,
            then,
        };
        browser.go(start);
        browser
    }

    /// Who gets the answer: handed to `Pickers::deliver` once [`Self::show`] has one.
    pub(crate) fn into_answer(self) -> (Option<PickTarget>, Then) {
        (self.target, self.then)
    }

    fn go(&mut self, dir: PathBuf) {
        self.selected.clear();
        self.confirm_replace = None;
        match list(&dir) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(e) => {
                self.entries.clear();
                self.error = Some(e);
            }
        }
        self.dir = dir;
    }

    fn shows(&self, e: &Entry) -> bool {
        if e.dir {
            return true;
        }
        if self.mode == Mode::Folder {
            return false;
        }
        let Some((_, exts)) = self.filters.get(self.filter) else { return true };
        let ext = Path::new(&e.name).extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        exts.iter().any(|x| x == "*" || *x == ext)
    }

    /// The path Save writes: the typed name in this folder, with the filter's extension when the
    /// name has none.
    fn save_path(&self) -> Option<PathBuf> {
        let name = self.name.trim();
        if name.is_empty() || name.contains('/') {
            return None;
        }
        let mut name = name.to_string();
        if Path::new(&name).extension().is_none()
            && let Some(ext) = self.filters.get(self.filter).and_then(|(_, e)| e.first()).filter(|e| *e != "*")
        {
            name = format!("{name}.{ext}");
        }
        Some(self.dir.join(name))
    }

    /// What to ask the system picker for, in this picker's mode.
    fn system_pick(&self) -> Option<SystemPick> {
        match self.mode {
            Mode::File | Mode::Files => {
                let exts: Vec<&String> = self.filters.get(self.filter).map(|(_, e)| e.iter().collect()).unwrap_or_default();
                let mut mime_types: Vec<String> = exts.iter().map(|e| mime_for(e).to_string()).collect();
                mime_types.dedup();
                if mime_types.is_empty() || exts.iter().any(|e| *e == "*") || mime_types.iter().any(|m| m == "application/octet-stream") {
                    mime_types = vec!["*/*".to_string()];
                }
                Some(SystemPick::Open { multiple: self.mode == Mode::Files, mime_types })
            }
            Mode::Save => {
                let path = self.save_path()?;
                let name = path.file_name()?.to_string_lossy().into_owned();
                let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                Some(SystemPick::Save { name, mime_type: mime_for(&ext).to_string() })
            }
            Mode::Folder => None,
        }
    }

    /// The button that hands the choice to Android's document picker, and its answer.
    fn system_picker(&mut self, ui: &mut egui::Ui, answer: &mut Option<Vec<PathBuf>>) {
        let Some(documents) = DOCUMENTS.get() else { return };
        let pick = self.system_pick();
        if self.mode == Mode::Folder {
            return;
        }
        if self.system_waiting {
            match (documents.poll)() {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tl!("Waiting for the document… (Google Drive downloads it first when it isn't on the tablet yet)"));
                    });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                    return;
                }
                Some(Ok(paths)) => {
                    self.system_waiting = false;
                    if !paths.is_empty() {
                        *answer = Some(paths);
                    }
                }
                Some(Err(e)) => {
                    self.system_waiting = false;
                    self.system_note = Some(e);
                }
            }
        }
        let label = if self.mode == Mode::Save { tl!("Save to Google Drive or another app…") } else { tl!("Google Drive and other apps…") };
        let button = egui::Button::new(egui::RichText::new(format!("☁  {label}")).strong()).min_size(egui::vec2(0.0, ROW_HEIGHT));
        let hint = if self.mode == Mode::Save {
            tl!("Choose where to save with Android's picker: Google Drive, Dropbox, OneDrive and other apps. Saving again later updates it there.")
        } else {
            tl!("Open from Google Drive, Dropbox, OneDrive and other apps with Android's picker. Saving updates the file there.")
        };
        let clicked = ui.add_enabled(pick.is_some(), button).on_hover_text(hint).on_disabled_hover_text(tl!("Type a name first")).clicked();
        if clicked && let Some(pick) = pick {
            match (documents.start)(pick) {
                Ok(()) => {
                    self.system_waiting = true;
                    self.system_note = None;
                }
                Err(e) => self.system_note = Some(e),
            }
        }
        if let Some(note) = &self.system_note {
            ui.colored_label(ui.visuals().error_fg_color, note);
        }
    }

    /// Draw the picker. `Some` once the user is done: the chosen paths, or none when cancelled.
    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Option<Vec<PathBuf>> {
        let mut answer: Option<Vec<PathBuf>> = None;
        let mut go_to: Option<PathBuf> = None;
        let screen = ctx.content_rect();
        if !self.access {
            // Back from Settings with access on: list the folder again.
            if access_granted() {
                self.access = true;
                let dir = self.dir.clone();
                self.go(dir);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
        }
        let modal = egui::Modal::new(egui::Id::new("android-file-picker")).show(ctx, |ui| {
            ui.set_width((screen.width() * 0.8).clamp(320.0, 980.0));
            ui.spacing_mut().interact_size.y = ROW_HEIGHT;
            ui.heading(&self.title);
            ui.add_space(6.0);
            self.system_picker(ui, &mut answer);
            ui.add_space(4.0);
            if !self.access {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(tl!("PdfCraft can't see your files yet")).strong());
                    ui.label(tl!(
                        "To open and save PDFs anywhere on this device, turn on All files access for PdfCraft. Settings opens on the switch; come back here afterwards."
                    ));
                    let button = egui::Button::new(egui::RichText::new(tl!("Allow access…")).strong()).min_size(egui::vec2(220.0, ROW_HEIGHT + 8.0));
                    if ui.add(button).clicked() {
                        self.access_note = Some(match FILE_ACCESS.get() {
                            None => tl!("This build can't open Settings by itself.").to_string(),
                            Some(access) => match (access.request)() {
                                Ok(()) => tl!("Opening Settings… If nothing appears, use the steps below.").to_string(),
                                Err(e) => format!("{} {e}", tl!("Settings didn't open:")),
                            },
                        });
                    }
                    if let Some(note) = &self.access_note {
                        ui.label(egui::RichText::new(note).italics());
                    }
                    ui.add_space(4.0);
                    ui.label(tl!("Or yourself: Settings ▸ Apps ▸ ⋮ (top right) ▸ Special access ▸ All files access ▸ PdfCraft ▸ turn it on."));
                });
                ui.add_space(6.0);
            }

            // Places, then where we are.
            ui.horizontal_wrapped(|ui| {
                for (label, path) in places() {
                    if ui.button(label).clicked() {
                        go_to = Some(path);
                    }
                }
            });
            ui.horizontal(|ui| {
                let up = self.dir.parent().map(Path::to_path_buf);
                if ui.add_enabled(up.is_some(), egui::Button::new("⬆")).on_hover_text(tl!("Up one folder")).clicked() {
                    go_to = up;
                }
                ui.label(egui::RichText::new(self.dir.display().to_string()).monospace());
            });
            ui.separator();

            // The folder's contents.
            let list_height = (screen.height() * 0.5).max(200.0);
            egui::ScrollArea::vertical().max_height(list_height).auto_shrink([false, false]).show(ui, |ui| {
                if let Some(e) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let shown: Vec<Entry> = self.entries.iter().filter(|e| self.shows(e)).cloned().collect();
                if shown.is_empty() && self.error.is_none() {
                    ui.weak(tl!("This folder is empty"));
                }
                for e in shown {
                    let icon = if e.dir { "📁" } else { "📄" };
                    let picked = self.selected.contains(&e.path);
                    let row = ui.add_sized([ui.available_width(), ROW_HEIGHT], egui::Button::selectable(picked, format!("{icon}  {}", e.name)));
                    if row.double_clicked() && !e.dir && matches!(self.mode, Mode::File | Mode::Files) {
                        answer = Some(vec![e.path.clone()]);
                    } else if row.clicked() {
                        if e.dir {
                            go_to = Some(e.path.clone());
                        } else {
                            match self.mode {
                                Mode::File => self.selected = vec![e.path.clone()],
                                Mode::Files => {
                                    if picked {
                                        self.selected.retain(|p| p != &e.path);
                                    } else {
                                        self.selected.push(e.path.clone());
                                    }
                                }
                                Mode::Save => self.name.clone_from(&e.name),
                                Mode::Folder => {}
                            }
                        }
                    }
                }
            });
            ui.separator();

            // File types.
            if !self.filters.is_empty() && self.mode != Mode::Folder {
                ui.horizontal(|ui| {
                    ui.label(tl!("Show:"));
                    let all = self.filters.len();
                    let label = |i: usize| self.filters.get(i).map_or_else(|| tl!("All files").to_string(), |(n, _)| n.clone());
                    let current = label(self.filter);
                    let mut filter = self.filter;
                    egui::ComboBox::from_id_salt("android-picker-filter").selected_text(current).show_ui(ui, |ui| {
                        for i in 0..=all {
                            ui.selectable_value(&mut filter, i, label(i));
                        }
                    });
                    if filter != self.filter {
                        self.filter = filter;
                        self.selected.clear();
                    }
                });
            }

            // New folder (where something is being saved or a folder chosen).
            if matches!(self.mode, Mode::Save | Mode::Folder) {
                ui.horizontal(|ui| match &mut self.new_folder {
                    None => {
                        if ui.button(tl!("New folder")).clicked() {
                            self.new_folder = Some(String::new());
                        }
                    }
                    Some(name) => {
                        let field = ui.text_edit_singleline(name);
                        let create = ui.button(tl!("Create")).clicked() || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if create {
                            let name = name.trim().to_string();
                            if !name.is_empty() && !name.contains('/') {
                                let path = self.dir.join(&name);
                                match std::fs::create_dir(&path) {
                                    Ok(()) => go_to = Some(path),
                                    Err(e) => self.error = Some(e.to_string()),
                                }
                            }
                            self.new_folder = None;
                        }
                    }
                });
            }

            if self.mode == Mode::Save {
                ui.horizontal(|ui| {
                    ui.label(tl!("Name:"));
                    let field = ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(ui.available_width()));
                    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.confirm_save(&mut answer);
                    }
                });
            }
            if let Some(path) = self.confirm_replace.clone() {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                ui.colored_label(ui.visuals().warn_fg_color, format!("{name}: {}", tl!("already exists. Replace it?")));
            }

            // Buttons.
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(tl!("Cancel")).clicked() {
                    answer = Some(Vec::new());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match self.mode {
                    Mode::File | Mode::Files => {
                        let n = self.selected.len();
                        let label = if self.mode == Mode::Files && n > 1 { format!("{} ({n})", tl!("Open")) } else { tl!("Open").to_string() };
                        if ui.add_enabled(n > 0, egui::Button::new(label)).clicked() {
                            answer = Some(self.selected.clone());
                        }
                    }
                    Mode::Folder => {
                        if ui.add_enabled(self.error.is_none(), egui::Button::new(tl!("Use this folder"))).clicked() {
                            answer = Some(vec![self.dir.clone()]);
                        }
                    }
                    Mode::Save => {
                        let label = if self.confirm_replace.is_some() { tl!("Replace") } else { tl!("Save") };
                        if ui.add_enabled(self.save_path().is_some(), egui::Button::new(label)).clicked() {
                            self.confirm_save(&mut answer);
                        }
                    }
                });
            });
        });
        if answer.is_none() && (modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
            answer = Some(Vec::new());
        }
        if let Some(dir) = go_to
            && answer.is_none()
        {
            self.go(dir);
        }
        if answer.as_ref().is_some_and(|a| !a.is_empty()) {
            *LAST_DIR.lock().unwrap_or_else(PoisonError::into_inner) = Some(self.dir.clone());
        }
        answer
    }

    /// Save: answer with the path, after asking once when it would replace a file.
    fn confirm_save(&mut self, answer: &mut Option<Vec<PathBuf>>) {
        let Some(path) = self.save_path() else { return };
        if path.exists() && self.confirm_replace.as_ref() != Some(&path) {
            self.confirm_replace = Some(path);
            return;
        }
        *answer = Some(vec![path]);
    }
}

/// Where a picker starts: Documents in shared storage (listed once All files access is on).
fn default_dir() -> PathBuf {
    let shared = PathBuf::from(SHARED);
    let docs = shared.join("Documents");
    if docs.is_dir() { docs } else { shared }
}

/// Shortcuts shown above the list: shared storage and its usual folders, SD cards and USB drives,
/// and the app's own folder.
fn places() -> Vec<(String, PathBuf)> {
    let shared = PathBuf::from(SHARED);
    let mut out = vec![(tl!("Internal storage").to_string(), shared.clone())];
    for (label, sub) in [(tl!("Documents"), "Documents"), (tl!("Downloads"), "Download")] {
        let p = shared.join(sub);
        if p.is_dir() {
            out.push((label.to_string(), p));
        }
    }
    // Removable storage appears as /storage/XXXX-XXXX.
    if let Ok(rd) = std::fs::read_dir("/storage") {
        for e in rd.flatten().take(16) {
            let name = e.file_name().to_string_lossy().to_string();
            if name != "emulated" && name != "self" && e.path().is_dir() {
                out.push((name, e.path()));
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        out.push((tl!("App folder").to_string(), PathBuf::from(home)));
    }
    out
}

/// The folder's visible entries, folders first, each group by name (ignoring case).
fn list(dir: &Path) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut entries: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            // `is_dir` follows links, as opening the entry would.
            Some(Entry { dir: path.is_dir(), name, path })
        })
        .take(MAX_ENTRIES)
        .collect();
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}
