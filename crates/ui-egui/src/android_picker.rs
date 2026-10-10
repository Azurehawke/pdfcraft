//! Android: a file browser drawn in the app, standing in for the native panels rfd shows elsewhere.
//!
//! Android's own picker (the Storage Access Framework) answers with `content://` URIs and needs
//! a Java activity to receive them; PdfCraft opens and saves plain paths. So the Android build
//! asks for "All files access" (MANAGE_EXTERNAL_STORAGE, granted once in Settings) and browses
//! shared storage itself. Every pick still goes through `PdfCraftApp::ask`, and the answer
//! arrives on a later frame like a native pick, so nothing else in the app changes.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

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
        let start = LAST_DIR.lock().unwrap_or_else(PoisonError::into_inner).clone().filter(|d| d.is_dir()).unwrap_or_else(default_dir);
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

    /// Draw the picker. `Some` once the user is done: the chosen paths, or none when cancelled.
    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Option<Vec<PathBuf>> {
        let mut answer: Option<Vec<PathBuf>> = None;
        let mut go_to: Option<PathBuf> = None;
        let screen = ctx.content_rect();
        let modal = egui::Modal::new(egui::Id::new("android-file-picker")).show(ctx, |ui| {
            ui.set_width((screen.width() * 0.8).clamp(320.0, 980.0));
            ui.spacing_mut().interact_size.y = ROW_HEIGHT;
            ui.heading(&self.title);
            ui.add_space(6.0);

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
                    if self.dir.starts_with("/storage") {
                        ui.label(tl!(
                            "PdfCraft needs All files access to browse your files: Settings ▸ Apps ▸ PdfCraft ▸ Permissions ▸ Files and media ▸ Allow management of all files. Then come back and tap a place above."
                        ));
                    }
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

/// Where a picker starts: shared storage when the app may read it, else the app's own folder.
fn default_dir() -> PathBuf {
    let shared = PathBuf::from(SHARED);
    if std::fs::read_dir(&shared).is_ok() {
        let docs = shared.join("Documents");
        return if docs.is_dir() { docs } else { shared };
    }
    std::env::var_os("HOME").map_or(shared, PathBuf::from)
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
