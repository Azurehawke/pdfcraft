//! RevuCraft's Links pane: the DWG/DXF drawings the document's sheets are linked to, as
//! InDesign's Links panel.
//!
//! Each link shows whether its file is up to date, modified or missing. The active document's
//! files are looked at every two seconds; a changed file must stay the same for two seconds before
//! it counts (a save or a sync still writing it). A drawing opened with Edit original updates its
//! sheet by itself when it is saved; other changes wait for Update.
//!
//! A link remembers its file's full path and, when the PDF has a place on disk, the path from the
//! PDF. Another device (a tablet with the folder synced) finds the drawing from the PDF first, then
//! at the full path, then by name next to the PDF and in the folders below it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{CornerRadius, Stroke};
use pdfcraft_engine::Edit;
use pdfcraft_engine::cad_links::{CadLink, LinkedFile};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

const POLL: Duration = Duration::from_secs(2);
/// A changed file counts once it has stayed the same this long.
const SETTLE: Duration = Duration::from_secs(2);
/// A CADCraft that closes this soon after Edit original started it most likely handed the file
/// to a CADCraft already running: the drawing is still open there, so it stays watched.
const HANDOFF: Duration = Duration::from_secs(5);

/// A placed drawing being moved on its sheet: where it is and the other drawings there (user
/// space), and where the drag started.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkMove {
    pub index: usize,
    pub page: usize,
    pub rect: [f64; 4],
    pub others: Vec<[f64; 4]>,
    from: Option<egui::Pos2>,
}

/// How far (screen points) an edge looks for another drawing's edge to line up with.
const SNAP: f32 = 8.0;

/// The drag `d`, nudged so the moved box's edges line up with the nearest edges of the others.
fn snapped(rect: egui::Rect, d: egui::Vec2, others: &[egui::Rect]) -> egui::Vec2 {
    let moved = rect.translate(d);
    let best = |mine: [f32; 2], theirs: &dyn Fn(&egui::Rect) -> [f32; 2]| {
        let mut best: Option<f32> = None;
        for o in others {
            for a in mine {
                for b in theirs(o) {
                    let diff = b - a;
                    if diff.abs() < SNAP && best.is_none_or(|x| diff.abs() < x.abs()) {
                        best = Some(diff);
                    }
                }
            }
        }
        best.unwrap_or(0.0)
    };
    let dx = best([moved.left(), moved.right()], &|o| [o.left(), o.right()]);
    let dy = best([moved.top(), moved.bottom()], &|o| [o.top(), o.bottom()]);
    d + egui::vec2(dx, dy)
}

/// Moving a placed drawing on the sheet: drag its box; its edges snap to the other drawings'.
/// Returns true while the pointer belongs to it.
pub(crate) fn page_input(ui: &egui::Ui, resp: &egui::Response, cx: &crate::comments::PageCx<'_>, view: &mut crate::canvas::DocView) -> bool {
    if view.link_move.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        view.link_move = None;
        return false;
    }
    let Some(mv) = view.link_move.as_mut() else { return false };
    if mv.page != cx.page {
        return false;
    }
    let screen = |r: &[f64; 4]| cx.xf.user_rect(cx.info, cx.page, r.map(|v| v as f32));
    let rect = screen(&mv.rect);
    let others: Vec<egui::Rect> = mv.others.iter().map(screen).collect();
    let (pointer, origin) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.press_origin()));
    if resp.drag_started()
        && let Some(o) = origin.filter(|o| rect.expand(4.0).contains(*o))
    {
        mv.from = Some(o);
    }
    let offset = match (mv.from, pointer) {
        (Some(from), Some(p)) => snapped(rect, p - from, &others),
        _ => egui::Vec2::ZERO,
    };
    let t = Tokens::get(ui.ctx());
    let painter = ui.painter();
    for o in &others {
        painter.rect_stroke(*o, CornerRadius::ZERO, Stroke::new(1.0, t.text_faint), egui::StrokeKind::Outside);
    }
    let shown = rect.translate(offset);
    painter.rect(shown, CornerRadius::ZERO, t.accent.gamma_multiply(0.08), Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
    painter.text(
        shown.left_top() + egui::vec2(4.0, -6.0),
        egui::Align2::LEFT_BOTTOM,
        tl!("Drag to move · Esc when done"),
        theme::medium(11.5),
        t.accent,
    );
    let over = pointer.is_some_and(|p| rect.expand(4.0).contains(p));
    if over || mv.from.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    if resp.drag_stopped()
        && let Some(from) = mv.from.take()
    {
        let (a, b) = (cx.to_user(from), cx.to_user(from + offset));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        if dx.abs() > 0.01 || dy.abs() > 0.01 {
            mv.rect = [mv.rect[0] + dx, mv.rect[1] + dy, mv.rect[2] + dx, mv.rect[3] + dy];
            let index = mv.index;
            view.pending_edit = Some(Edit::MoveCadLink { index, dx, dy });
        }
        return true;
    }
    over || mv.from.is_some()
}

/// Place the drawings at `paths` on the current sheet, linked to their files.
/// File ▸ Place drawing… (also the Draft ribbon and the Links pane): pick DWG or DXF files to
/// place on the current page, linked to them, and show the Links pane where they are listed.
pub(crate) fn place_drawing(app: &mut PdfCraftApp) {
    let Some((_, id)) = app.active_ids() else { return };
    if app.show_ribbon {
        if !app.is_docked(crate::RightPanel::Links) {
            app.toggle_docked(crate::RightPanel::Links);
        }
    } else {
        app.choose_right_panel(Some(crate::RightPanel::Links));
    }
    let dialog = crate::dialog::AsyncFileDialog::new().add_filter(tl!("Drawings (DWG, DXF)"), &pdfcraft_engine::CAD_EXTENSIONS);
    let _ = app.ask(crate::pickers::Ask::Files(dialog), Some(id), place);
}

fn place(app: &mut PdfCraftApp, paths: Vec<std::path::PathBuf>) {
    let Some((index, id)) = app.active_ids() else { return };
    let page = app.views.get(index).map_or(0, |v| v.current);
    let pdf = app.session.get(id).and_then(|d| d.path.clone());
    for p in paths {
        let path = p.to_string_lossy().into_owned();
        let name = file_name(&path);
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => {
                app.notify(crate::i18n::fmt(tl!("Couldn't read {name}: {e}"), &[("name", &name), ("e", &e.to_string())]));
                continue;
            }
        };
        let size = u64::try_from(bytes.len()).unwrap_or(0);
        let relative = pdf.as_deref().and_then(|pdf| relative_to(pdf, &path));
        let file = linked_file_from(&path, pdf.as_deref()).unwrap_or(LinkedFile { path: path.clone(), relative, size, mtime: 0, now: now_secs() });
        if !app.apply_edit(Edit::PlaceCadLink { page, name, bytes: Arc::new(bytes), file }) {
            break;
        }
    }
}

/// A link's state, as the pane shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    UpToDate,
    Modified,
    Missing,
    /// Opened with Edit original: the sheet updates by itself when the file is saved.
    Editing,
}

/// The pane's state.
#[derive(Debug, Default)]
pub struct LinksState {
    /// Keep each replaced drawing as a hidden Rev layer (off by default).
    pub keep_revisions: bool,
    selected: usize,
    /// Files opened with Edit original.
    editing: HashSet<String>,
    /// The CADCraft Edit original started for a file, and when: once it closes, the file is no
    /// longer being edited (a save made just before is still picked up).
    editors: HashMap<String, (std::process::Child, Instant)>,
    /// A changed file as last seen: (size, modified, since when it has looked so).
    seen: HashMap<String, (u64, i64, Instant)>,
    status: HashMap<String, Status>,
    last_poll: Option<Instant>,
    /// Relink: the link and the path being typed.
    relink: Option<(usize, String)>,
    /// Where each linked file was found on this device (by the path recorded in the PDF).
    found: HashMap<String, (String, Found)>,
    /// When the folders next to the PDF were last searched for a missing drawing.
    searched: HashMap<String, Instant>,
    /// Files whose date or size differ from the link's but whose drawing is the same: (size,
    /// modified) as checked, so each is read once.
    same: HashMap<String, (u64, i64)>,
}

/// Larger files aren't read to compare (they count as changed).
const MAX_COMPARE: u64 = 256 << 20;

/// Whether the file at `link.path` (now `size`, `mtime`) holds the drawing the sheet was made from.
fn same_drawing(state: &mut LinksState, link: &CadLink, size: u64, mtime: i64) -> bool {
    if state.same.get(&link.path) == Some(&(size, mtime)) {
        return true;
    }
    if link.fingerprint.is_empty() || size != link.size || size > MAX_COMPARE {
        return false;
    }
    let same = std::fs::read(&link.path).is_ok_and(|b| pdfcraft_engine::cad_links::fingerprint(&b) == link.fingerprint);
    if same {
        state.same.insert(link.path.clone(), (size, mtime));
    }
    same
}

/// How a linked drawing was found on this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// At the path the PDF records.
    AtPath,
    /// From the PDF, where it was when linked (another device, or the job folder moved).
    FromPdf,
    /// By name, next to the PDF.
    NextToPdf,
    /// By name, in a folder below the PDF's.
    BelowPdf,
}

/// The folders below the PDF's searched for a missing drawing: this many levels, this many
/// folders at most, and not more often than this.
const SEARCH_DEPTH: usize = 3;
const SEARCH_FOLDERS: usize = 400;
const SEARCH_EVERY: Duration = Duration::from_secs(30);

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|d| i64::try_from(d.as_secs()).ok()).unwrap_or(0)
}

/// The file's size and modification time (seconds since 1970), or `None` when it can't be read.
#[cfg(not(target_arch = "wasm32"))]
fn file_meta(path: &str) -> Option<(u64, i64)> {
    let m = std::fs::metadata(path).ok()?;
    let modified = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((m.len(), i64::try_from(modified.as_secs()).ok()?))
}

#[cfg(target_arch = "wasm32")]
fn file_meta(_path: &str) -> Option<(u64, i64)> {
    None
}

/// What to record about the file a drawing was just opened from (a new sheet: no PDF on disk yet).
pub(crate) fn linked_file(path: &str) -> Option<LinkedFile> {
    linked_file_from(path, None)
}

/// [`linked_file`], with where the file is from the PDF at `pdf` (when it has a place on disk).
fn linked_file_from(path: &str, pdf: Option<&str>) -> Option<LinkedFile> {
    let (size, mtime) = file_meta(path)?;
    let relative = pdf.and_then(|pdf| relative_to(pdf, path));
    Some(LinkedFile { path: path.to_string(), relative, size, mtime, now: now_secs() })
}

/// The last part of a path written on any system (`M:\Jobs\101.dwg` read on Android is
/// `101.dwg`, not the whole string).
fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or(path).to_string()
}

/// Where `file` is from the PDF at `pdf`, `/`-separated (`101.dwg`, `rooms/101.dwg`,
/// `../drawings/101.dwg`), or `None` when they share no folder (another drive) or are far apart.
pub(crate) fn relative_to(pdf: &str, file: &str) -> Option<String> {
    use std::path::{Component, Path};
    let (pdf, file) = (Path::new(pdf), Path::new(file));
    if !pdf.is_absolute() || !file.is_absolute() {
        return None;
    }
    let dir: Vec<Component> = pdf.parent()?.components().collect();
    let target: Vec<Component> = file.components().collect();
    let common = dir.iter().zip(&target).take_while(|(a, b)| a == b).count();
    // Only the root (or drive) in common: nothing that moves together with the PDF.
    let ups = dir.len().checked_sub(common)?;
    if common <= 1 || ups > 3 {
        return None;
    }
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), ups).collect();
    for c in target.get(common..)? {
        match c {
            Component::Normal(n) => parts.push(n.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Where linked file `link` is on this device, and how it was found, for the PDF at `pdf`.
/// `search` also looks through the folders below the PDF's (slower: a network folder).
fn locate(link: &CadLink, pdf: Option<&str>, search: bool) -> Option<(String, Found)> {
    let exists = |p: &std::path::Path| p.is_file();
    let dir = pdf.and_then(|p| std::path::Path::new(p).parent()).filter(|d| !d.as_os_str().is_empty());
    if let (Some(dir), Some(rel)) = (dir, link.relative.as_deref()) {
        let p = rel.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".").fold(dir.to_path_buf(), |p, s| p.join(s));
        if exists(&p) {
            return Some((p.to_string_lossy().into_owned(), Found::FromPdf));
        }
    }
    if exists(std::path::Path::new(&link.path)) {
        return Some((link.path.clone(), Found::AtPath));
    }
    let dir = dir?;
    let name = file_name(&link.path);
    let beside = dir.join(&name);
    if exists(&beside) {
        return Some((beside.to_string_lossy().into_owned(), Found::NextToPdf));
    }
    if !search {
        return None;
    }
    find_below(dir, &name).map(|p| (p.to_string_lossy().into_owned(), Found::BelowPdf))
}

/// The one file called `name` (any case) in the folders below `dir`, a few levels down. None when
/// there are several: which one was meant can't be told.
fn find_below(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let mut queue: std::collections::VecDeque<(std::path::PathBuf, usize)> = std::collections::VecDeque::new();
    queue.push_back((dir.to_path_buf(), 0));
    let (mut folders, mut hits) = (0usize, Vec::new());
    while let Some((folder, depth)) = queue.pop_front() {
        folders += 1;
        if folders > SEARCH_FOLDERS || hits.len() > 1 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            let path = e.path();
            if kind.is_dir() && depth < SEARCH_DEPTH {
                queue.push_back((path, depth + 1));
            } else if kind.is_file() && depth > 0 && e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
                hits.push(path);
            }
        }
    }
    match hits.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// The active document's links, each with the path where its file is on this device (as last
/// found) instead of the one recorded, and the PDF's own path.
fn here(app: &PdfCraftApp) -> (Vec<CadLink>, Option<String>) {
    let Some(doc) = app.active_ids().and_then(|(_, id)| app.session.get(id)) else { return (Vec::new(), None) };
    let links = doc
        .cad_links()
        .into_iter()
        .map(|mut l| {
            if let Some((p, _)) = app.links.found.get(&l.path) {
                l.path = p.clone();
            }
            l
        })
        .collect();
    (links, doc.path.clone())
}

/// How link `recorded` (its path as the PDF records it) was found here, if not at that path.
fn found_how(state: &LinksState, recorded: &str) -> Option<Found> {
    state.found.get(recorded).map(|(_, how)| *how).filter(|h| *h != Found::AtPath)
}

/// "just now", "5 min ago", "3 h ago", else the date.
fn ago(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    let d = now_secs().saturating_sub(secs);
    if d < 60 {
        tl!("just now").to_string()
    } else if d < 3600 {
        crate::i18n::fmt(tl!("{n} min ago"), &[("n", &(d / 60).to_string())])
    } else if d < 86_400 {
        crate::i18n::fmt(tl!("{n} h ago"), &[("n", &(d / 3600).to_string())])
    } else {
        let t = std::time::UNIX_EPOCH + Duration::from_secs(u64::try_from(secs).unwrap_or(0));
        crate::new_dialog::modified_label(Some(t))
    }
}

/// Links needing attention (modified or missing), for the badge on the rail.
pub fn attention(app: &PdfCraftApp) -> usize {
    here(app).0.iter().filter(|l| matches!(app.links.status.get(&l.path), Some(Status::Modified | Status::Missing))).count()
}

/// Look at the active document's linked files (every two seconds), and update the sheets of
/// files saved from Edit original.
pub fn poll(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some((_, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let links = doc.cad_links();
    if links.is_empty() {
        return;
    }
    ctx.request_repaint_after(POLL);
    let now = Instant::now();
    if app.links.last_poll.is_some_and(|t| now.duration_since(t) < POLL) {
        return;
    }
    app.links.last_poll = Some(now);
    let closed = closed_editors(&mut app.links, now);
    if !closed.is_empty() {
        end_editing(app, &closed);
    }
    // Find each file on this device (from the PDF, at its path, or next to the PDF).
    let pdf = app.session.get(id).and_then(|d| d.path.clone());
    let Some(recorded) = app.session.get(id).map(|d| d.cad_links()) else { return };
    for l in &recorded {
        let search = app.links.searched.get(&l.path).is_none_or(|t| now.duration_since(*t) >= SEARCH_EVERY);
        match locate(l, pdf.as_deref(), search) {
            Some(found) => {
                app.links.found.insert(l.path.clone(), found);
            }
            None => {
                app.links.found.remove(&l.path);
                if search {
                    app.links.searched.insert(l.path.clone(), now);
                }
            }
        }
    }
    let (links, _) = here(app);
    let mut auto = Vec::new();
    let state = &mut app.links;
    for l in &links {
        let editing = state.editing.contains(&l.path);
        let status = match file_meta(&l.path) {
            None => Status::Missing,
            Some((size, mtime)) if size == l.size && mtime == l.mtime => {
                state.seen.remove(&l.path);
                if editing { Status::Editing } else { Status::UpToDate }
            }
            Some((size, mtime)) => {
                let settled = match state.seen.get(&l.path) {
                    Some((s, m, since)) if *s == size && *m == mtime => now.duration_since(*since) >= SETTLE,
                    _ => {
                        state.seen.insert(l.path.clone(), (size, mtime, now));
                        false
                    }
                };
                match (settled, editing) {
                    (true, true) => {
                        auto.push(l.index);
                        Status::Editing
                    }
                    (_, true) => Status::Editing,
                    // A copy with a new date but the same drawing (copied, not synced) isn't a change.
                    (true, false) if same_drawing(state, l, size, mtime) => Status::UpToDate,
                    (true, false) => Status::Modified,
                    (false, false) => state.status.get(&l.path).copied().unwrap_or(Status::UpToDate),
                }
            }
        };
        state.status.insert(l.path.clone(), status);
    }
    for index in auto {
        update(app, index, true);
    }
}

/// Files whose CADCraft (started by Edit original) has closed since the last look.
fn closed_editors(state: &mut LinksState, now: Instant) -> Vec<String> {
    let mut closed = Vec::new();
    state.editors.retain(|path, (child, started)| match child.try_wait() {
        Ok(None) => true,
        // Gone (or no longer answering): editing is over, unless it handed the file on at once.
        Ok(Some(_)) | Err(_) => {
            if now.duration_since(*started) >= HANDOFF {
                closed.push(path.clone());
            }
            false
        }
    });
    closed
}

/// Stop treating `paths` as open in CADCraft (it closed, or Done editing). A save made just
/// before still updates the sheet; otherwise the drawing simply goes back to Up to date.
fn end_editing(app: &mut PdfCraftApp, paths: &[String]) {
    let (links, _) = here(app);
    for path in paths {
        let saved: Vec<usize> = links
            .iter()
            .filter(|l| &l.path == path && file_meta(&l.path).is_some_and(|(size, mtime)| size != l.size || mtime != l.mtime))
            .map(|l| l.index)
            .collect();
        for index in saved {
            if !update(app, index, true) {
                break;
            }
        }
        app.links.editing.remove(path);
        app.links.editors.remove(path);
        app.links.seen.remove(path);
        app.links.status.remove(path);
    }
    // Look again straight away, so the pane shows each file's real state.
    app.links.last_poll = None;
}

/// Redraw link `index` from its file.
fn update(app: &mut PdfCraftApp, index: usize, saved_in_cad: bool) -> bool {
    let (links, pdf) = here(app);
    let Some(link) = links.into_iter().find(|l| l.index == index) else { return false };
    let name = file_name(&link.path);
    let bytes = match std::fs::read(&link.path) {
        Ok(b) => b,
        Err(e) => {
            app.notify(crate::i18n::fmt(tl!("Couldn't read {name}: {e}"), &[("name", &name), ("e", &e.to_string())]));
            return false;
        }
    };
    // Recorded as found here (and from the PDF), so every device finds it next time.
    let Some(file) = linked_file_from(&link.path, pdf.as_deref()) else { return false };
    let edit = Edit::UpdateCadLink { index, name: name.clone(), bytes: Arc::new(bytes), file, keep_revision: app.links.keep_revisions };
    let ok = app.apply_edit(edit);
    if ok {
        app.links.seen.remove(&link.path);
        app.links.status.insert(link.path.clone(), if app.links.editing.contains(&link.path) { Status::Editing } else { Status::UpToDate });
        let msg = if saved_in_cad {
            tl!("{name} was saved in CADCraft: the sheet is updated, and the markups stayed put.")
        } else {
            tl!("{name} is updated on the sheet; the markups stayed put.")
        };
        app.notify(crate::i18n::fmt(msg, &[("name", &name)]));
    }
    ok
}

/// CADCraft's program: `CADCRAFT` in the environment, beside RevuCraft, where its installer puts
/// it, or on the PATH.
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn cadcraft_program() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let exe = if cfg!(windows) { "cadcraft.exe" } else { "cadcraft" };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("CADCRAFT") {
        candidates.push(PathBuf::from(p));
    }
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        candidates.push(dir.join(exe));
        if let Some(up) = dir.parent() {
            candidates.push(up.join("CADCraft").join(exe));
        }
    }
    for var in ["LOCALAPPDATA", "ProgramFiles"] {
        if let Some(base) = std::env::var_os(var) {
            let base = PathBuf::from(base);
            candidates.push(base.join("Programs").join("CADCraft").join(exe));
            candidates.push(base.join("CADCraft").join(exe));
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|d| d.join(exe)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Open the drawing in CADCraft (or, without it, in the app the system opens drawings with).
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
/// Returns CADCraft's process when it was started directly, so its closing can be noticed; the
/// system's opener hands the file on and exits, so there is nothing to follow then.
fn edit_original(path: &str) -> Result<Option<std::process::Child>, String> {
    use std::process::Command;
    match cadcraft_program() {
        Some(p) => Command::new(p).arg(path).spawn().map(Some),
        None if cfg!(windows) => Command::new("cmd").args(["/C", "start", "", path]).spawn().map(|_| None),
        None if cfg!(target_os = "macos") => Command::new("open").arg(path).spawn().map(|_| None),
        None => Command::new("xdg-open").arg(path).spawn().map(|_| None),
    }
    .map_err(|e| e.to_string())
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn edit_original(_path: &str) -> Result<Option<std::process::Child>, String> {
    Err(tl!("Open the drawing in CADCraft yourself; this sheet updates when it is saved.").to_string())
}

/// Show the file in the system's file manager.
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn reveal(path: &str) -> Result<(), String> {
    use std::process::Command;
    let spawned = if cfg!(windows) {
        Command::new("explorer").arg(format!("/select,{path}")).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").args(["-R", path]).spawn()
    } else {
        let dir = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
        Command::new("xdg-open").arg(dir).spawn()
    };
    spawned.map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn reveal(_path: &str) -> Result<(), String> {
    Err(tl!("Showing files isn't available here.").to_string())
}

fn chip(ui: &mut egui::Ui, t: &Tokens, status: Status) {
    let (label, fg) = match status {
        Status::UpToDate => (tl!("Up to date"), egui::Color32::from_rgb(0x4a, 0xa8, 0x6a)),
        Status::Modified => (tl!("Modified"), egui::Color32::from_rgb(0xd9, 0x9a, 0x1c)),
        Status::Missing => (tl!("Missing"), egui::Color32::from_rgb(0xdc, 0x5a, 0x5a)),
        Status::Editing => (tl!("In CADCraft"), t.accent),
    };
    egui::Frame::NONE.fill(fg.gamma_multiply(0.16)).corner_radius(CornerRadius::same(9)).inner_margin(egui::Margin::symmetric(7, 1)).show(ui, |ui| {
        ui.label(egui::RichText::new(label).font(theme::semibold(11.0)).color(fg));
    });
}

/// Relink link `index` to `path`, and every other missing drawing found by name in the same
/// folder with it (as InDesign's relink offers), in one undoable step.
fn relink(app: &mut PdfCraftApp, links: &[CadLink], pdf: Option<&str>, index: usize, path: &str) {
    let edit = |index: usize, path: &str| Edit::RelinkCad { index, path: path.to_string(), relative: pdf.and_then(|pdf| relative_to(pdf, path)) };
    let mut edits = vec![edit(index, path)];
    if let Some(folder) = std::path::Path::new(path).parent().filter(|_| std::path::Path::new(path).is_file()) {
        for l in links.iter().filter(|l| l.index != index && app.links.status.get(&l.path) == Some(&Status::Missing)) {
            let candidate = folder.join(file_name(&l.path));
            if candidate.is_file() {
                edits.push(edit(l.index, &candidate.to_string_lossy()));
            }
        }
    }
    let others = edits.len() - 1;
    let ok = if others == 0 {
        edits.pop().is_some_and(|e| app.apply_edit(e))
    } else {
        app.apply_edit(Edit::Batch { label: "Relink drawings".into(), edits })
    };
    if ok {
        // Look for every file again.
        app.links.found.clear();
        app.links.status.clear();
        app.links.searched.clear();
        app.links.last_poll = None;
        if others > 0 {
            app.notify(crate::i18n::fmt(tl!("Relinked {n} more drawings found in the same folder."), &[("n", &others.to_string())]));
        }
    }
}

/// What the pane asked for this frame.
enum Act {
    Update(usize),
    UpdateAll,
    Edit(String),
    /// Stop watching the files opened with Edit original.
    DoneEditing,
    Reveal(String),
    Relink(usize, String),
    /// Start (or, `true`, finish) moving a placed drawing: index, page, where it is, the others.
    Move(usize, usize, [f64; 4], Vec<[f64; 4]>, bool),
}

/// The pane.
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((_, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let editable = doc.allows_modification();
    let pages = doc.info.pages.len();
    // The paths as the PDF records them, by link.
    let recorded: Vec<(usize, String)> = doc.cad_links().into_iter().map(|l| (l.index, l.path)).collect();
    let (links, pdf) = here(app);
    let state = &mut app.links;
    let status = |l: &CadLink| state.status.get(&l.path).copied().unwrap_or(Status::UpToDate);
    let mut act = None;
    let place_now = ui
        .add_enabled(editable, egui::Button::new(tl!("Place drawing…")))
        .on_hover_text(tl!("Place a DWG or DXF on this sheet at its scale, linked to the file (one per room, say)"))
        .clicked();
    if links.is_empty() {
        ui.label(
            egui::RichText::new(tl!(
                "Sheets opened from a DWG or DXF are linked to it and show here, so you can update them when the drawing changes."
            ))
            .color(t.text_muted),
        );
    }
    if place_now {
        place_drawing(app);
        return;
    }
    if links.is_empty() {
        return;
    }
    let any_modified = links.iter().any(|l| status(l) == Status::Modified);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(crate::i18n::fmt(tl!("{n} linked drawings"), &[("n", &links.len().to_string())])).color(t.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(any_modified && editable, egui::Button::new(tl!("Update all"))).clicked() {
                act = Some(Act::UpdateAll);
            }
        });
    });
    if !state.editing.is_empty() {
        egui::Frame::NONE
            .fill(t.accent.gamma_multiply(0.12))
            .stroke(Stroke::new(1.0, t.accent.gamma_multiply(0.4)))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(tl!("Drawings opened with Edit original update their sheets by themselves when you save them in CADCraft."));
                if ui
                    .button(tl!("Done editing"))
                    .on_hover_text(tl!("Stop watching for saves from CADCraft. This also happens by itself when CADCraft closes."))
                    .clicked()
                {
                    act = Some(Act::DoneEditing);
                }
            });
    }
    ui.add_space(4.0);
    for l in &links {
        let selected = state.selected == l.index;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 42.0), egui::Sense::click());
        let name = file_name(&l.path);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &name));
        if selected {
            ui.painter().rect_filled(rect, CornerRadius::same(6), t.selected);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(6), t.hover);
        }
        icons::paint(ui, egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 12.0), egui::vec2(18.0, 18.0)), "file-text", 16.0, t.icon);
        widgets::row_text(ui, rect.min + egui::vec2(32.0, 13.0), name.clone(), theme::medium(13.0), t.text, (rect.width() - 130.0).max(40.0));
        let sub = if l.placed {
            crate::i18n::fmt(tl!("Sheet {n} · placed"), &[("n", &(l.page + 1).to_string())])
        } else {
            crate::i18n::fmt(tl!("Sheet {n} · {layout}"), &[("n", &(l.page + 1).to_string()), ("layout", &l.layout)])
        };
        widgets::row_text(ui, rect.min + egui::vec2(32.0, 30.0), sub, theme::regular(11.5), t.text_muted, (rect.width() - 130.0).max(40.0));
        let mut chip_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(rect.center_top() + egui::vec2(0.0, 10.0), rect.right_bottom() - egui::vec2(6.0, 8.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        chip(&mut chip_ui, &t, status(l));
        if resp.clicked() {
            state.selected = l.index;
            state.relink = None;
        }
    }
    let Some(sel) = links.iter().find(|l| l.index == state.selected).or(links.first()) else { return };
    let sel_status = status(sel);
    ui.add_space(6.0);
    ui.separator();
    egui::Grid::new("link-details").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        let muted = |s: &str| egui::RichText::new(s).color(t.text_muted);
        ui.label(muted(tl!("File")));
        ui.vertical(|ui| {
            ui.add(egui::Label::new(egui::RichText::new(&sel.path).monospace().size(11.5)).wrap());
            let how = recorded.iter().find(|(i, _)| *i == sel.index).and_then(|(_, r)| found_how(state, r));
            let note = match how {
                Some(Found::FromPdf) => Some(tl!("Found relative to the PDF")),
                Some(Found::NextToPdf) => Some(tl!("Found by name next to the PDF")),
                Some(Found::BelowPdf) => Some(tl!("Found by name in a subfolder of the PDF's folder")),
                Some(Found::AtPath) | None => None,
            };
            if let Some(note) = note {
                ui.label(egui::RichText::new(note).color(t.text_muted).small());
            }
        });
        ui.end_row();
        ui.label(muted(tl!("Layout")));
        ui.label(&sel.layout);
        ui.end_row();
        ui.label(muted(tl!("On sheet")));
        ui.label(crate::i18n::fmt(tl!("Sheet {n} of {total}"), &[("n", &(sel.page + 1).to_string()), ("total", &pages.to_string())]));
        ui.end_row();
        ui.label(muted(tl!("Updated")));
        ui.label(ago(sel.updated));
        ui.end_row();
        ui.label(muted(tl!("File saved")));
        match (sel_status, file_meta(&sel.path)) {
            (Status::Missing, _) | (_, None) => {
                ui.colored_label(egui::Color32::from_rgb(0xdc, 0x5a, 0x5a), tl!("Not found at this path"));
            }
            (Status::Modified, Some((_, m))) => {
                ui.colored_label(
                    egui::Color32::from_rgb(0xd9, 0x9a, 0x1c),
                    crate::i18n::fmt(tl!("{when} (newer than the sheet)"), &[("when", &ago(m))]),
                );
            }
            (_, Some((_, m))) => {
                ui.label(ago(m));
            }
        }
        ui.end_row();
    });
    if sel.revisions > 0 {
        ui.label(
            egui::RichText::new(crate::i18n::fmt(tl!("Previous drawings: {n} (hidden layers)"), &[("n", &sel.revisions.to_string())]))
                .color(t.text_muted),
        );
    }
    ui.add_space(4.0);
    ui.columns(2, |cols| {
        let (c0, rest) = cols.split_at_mut(1);
        let (Some(a), Some(b)) = (c0.first_mut(), rest.first_mut()) else { return };
        a.vertical_centered_justified(|ui| {
            if ui
                .add_enabled(sel_status != Status::Missing && editable, egui::Button::new(egui::RichText::new(tl!("Edit original")).strong()))
                .clicked()
            {
                act = Some(Act::Edit(sel.path.clone()));
            }
            if ui.button(tl!("Relink…")).clicked() {
                state.relink = Some((sel.index, sel.path.clone()));
            }
        });
        b.vertical_centered_justified(|ui| {
            if ui.add_enabled(matches!(sel_status, Status::Modified | Status::Editing) && editable, egui::Button::new(tl!("Update"))).clicked() {
                act = Some(Act::Update(sel.index));
            }
            if ui.add_enabled(sel_status != Status::Missing, egui::Button::new(tl!("Reveal file"))).clicked() {
                act = Some(Act::Reveal(sel.path.clone()));
            }
        });
    });
    let moving = sel.placed && app.views.iter().any(|v| v.link_move.as_ref().is_some_and(|m| m.index == sel.index));
    if sel.placed
        && let Some(rect) = sel.rect
    {
        let label = if moving { tl!("Done moving") } else { tl!("Move on the sheet") };
        if ui.add_enabled(editable, egui::Button::new(label)).clicked() {
            let others = links.iter().filter(|o| o.page == sel.page && o.index != sel.index).filter_map(|o| o.rect).collect();
            act = Some(Act::Move(sel.index, sel.page, rect, others, moving));
        }
    }
    let state = &mut app.links;
    if let Some((index, path)) = state.relink.as_mut() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tl!("Where the drawing is now")).color(t.text_muted));
        ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if widgets::pill_button(ui, tl!("Relink"), true).clicked() {
                act = Some(Act::Relink(*index, path.clone()));
            }
            if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                act = Some(Act::Relink(usize::MAX, String::new()));
            }
        });
    }
    ui.add_space(8.0);
    ui.separator();
    let mut keep = state.keep_revisions;
    if ui
        .checkbox(&mut keep, tl!("Keep previous drawings as hidden Rev layers"))
        .on_hover_text(tl!("Off: an update replaces the drawing. On: the drawing it replaces stays as a hidden layer, to compare."))
        .changed()
    {
        app.set_keep_link_revisions(keep);
    }
    match act {
        Some(Act::Update(i)) => {
            update(app, i, false);
        }
        Some(Act::UpdateAll) => {
            let modified: Vec<usize> = links.iter().filter(|l| app.links.status.get(&l.path) == Some(&Status::Modified)).map(|l| l.index).collect();
            for i in modified {
                if !update(app, i, false) {
                    break;
                }
            }
        }
        Some(Act::Edit(path)) => match edit_original(&path) {
            Ok(child) => {
                match child {
                    Some(child) => {
                        app.links.editors.insert(path.clone(), (child, Instant::now()));
                    }
                    None => {
                        app.links.editors.remove(&path);
                    }
                }
                app.links.editing.insert(path.clone());
                app.links.status.insert(path, Status::Editing);
            }
            Err(e) => {
                // Android: CADCraft is opened by hand; the sheet still updates when it's saved.
                app.links.editing.insert(path);
                app.notify(e);
            }
        },
        Some(Act::DoneEditing) => {
            let paths: Vec<String> = app.links.editing.iter().cloned().collect();
            end_editing(app, &paths);
        }
        Some(Act::Reveal(path)) => {
            if let Err(e) = reveal(&path) {
                app.notify(e);
            }
        }
        Some(Act::Move(index, page, rect, others, done)) => {
            if let Some((vi, _)) = app.active_ids()
                && let Some(view) = app.views.get_mut(vi)
            {
                view.link_move = (!done).then_some(LinkMove { index, page, rect, others, from: None });
                if !done {
                    view.go_to_page(page);
                }
            }
        }
        Some(Act::Relink(index, path)) => {
            app.links.relink = None;
            if index != usize::MAX {
                relink(app, &links, pdf.as_deref(), index, path.trim());
            }
        }
        None => {}
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn finished_child() -> Result<std::process::Child, String> {
        let mut child = std::process::Command::new("true").spawn().map_err(|e| e.to_string())?;
        child.wait().map_err(|e| e.to_string())?;
        Ok(child)
    }

    #[test]
    fn a_closed_cadcraft_ends_editing_but_a_handoff_does_not() -> Result<(), String> {
        let now = Instant::now();
        let mut state = LinksState::default();
        let long_ago = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        state.editors.insert("closed.dwg".into(), (finished_child()?, long_ago));
        state.editors.insert("handed-on.dwg".into(), (finished_child()?, now));
        let closed = closed_editors(&mut state, now);
        assert_eq!(closed, vec!["closed.dwg".to_string()], "only the CADCraft that ran for a while counts as closed");
        assert!(state.editors.is_empty(), "both finished processes are let go");
        Ok(())
    }

    #[test]
    fn paths_from_the_pdf_are_portable() {
        assert_eq!(relative_to("/jobs/smith/sheet.pdf", "/jobs/smith/101.dwg").as_deref(), Some("101.dwg"));
        assert_eq!(relative_to("/jobs/smith/sheet.pdf", "/jobs/smith/rooms/101.dwg").as_deref(), Some("rooms/101.dwg"));
        assert_eq!(relative_to("/jobs/smith/pdf/sheet.pdf", "/jobs/smith/dwg/101.dwg").as_deref(), Some("../dwg/101.dwg"));
        assert_eq!(relative_to("/home/a/sheet.pdf", "/mnt/b.dwg"), None, "only the root in common");
        assert_eq!(relative_to("sheet.pdf", "/jobs/101.dwg"), None, "a PDF with no place on disk");
        assert_eq!(file_name("M:\\Jobs\\Smith\\101.dwg"), "101.dwg", "a Windows path read on Android");
        assert_eq!(file_name("/storage/emulated/0/Sync/101.dwg"), "101.dwg");
    }

    fn link(path: &str, relative: Option<&str>) -> CadLink {
        CadLink {
            index: 0,
            page: 0,
            path: path.into(),
            relative: relative.map(Into::into),
            layout: "Model".into(),
            size: 0,
            mtime: 0,
            fingerprint: String::new(),
            updated: 0,
            revisions: 0,
            placed: true,
            rect: None,
        }
    }

    #[test]
    fn a_drawing_linked_on_another_device_is_found_next_to_the_pdf() -> Result<(), String> {
        let err = |e: std::io::Error| e.to_string();
        let root = std::env::temp_dir().join(format!("revucraft-links-{}", std::process::id()));
        let job = root.join("Sync").join("Smith");
        std::fs::create_dir_all(job.join("rooms")).map_err(err)?;
        std::fs::write(job.join("101.dwg"), b"room 101").map_err(err)?;
        std::fs::write(job.join("rooms").join("102.dwg"), b"room 102").map_err(err)?;
        let pdf = job.join("suite.pdf").to_string_lossy().into_owned();
        let found = |l: &CadLink, search: bool| locate(l, Some(&pdf), search).map(|(_, how)| how);
        // Recorded on the PC; the tablet has the job folder synced somewhere else.
        assert_eq!(found(&link("M:\\Jobs\\Smith\\101.dwg", Some("101.dwg")), false), Some(Found::FromPdf));
        assert_eq!(found(&link("M:\\Jobs\\Smith\\rooms\\102.dwg", Some("rooms/102.dwg")), false), Some(Found::FromPdf));
        // Linked before the PDF was saved: no relative path, found by name.
        assert_eq!(found(&link("M:\\Jobs\\Smith\\101.dwg", None), false), Some(Found::NextToPdf));
        assert_eq!(found(&link("M:\\Jobs\\Smith\\rooms\\102.dwg", None), false), None, "the folders below wait for a search");
        assert_eq!(found(&link("M:\\Jobs\\Smith\\rooms\\102.dwg", None), true), Some(Found::BelowPdf));
        let here = job.join("101.dwg").to_string_lossy().into_owned();
        assert_eq!(found(&link(&here, None), false), Some(Found::AtPath));
        assert_eq!(found(&link("M:\\Jobs\\Smith\\103.dwg", None), true), None);
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
