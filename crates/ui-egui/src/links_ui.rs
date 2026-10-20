//! RevuCraft's Links pane: the DWG/DXF drawings the document's sheets are linked to, as
//! InDesign's Links panel.
//!
//! Each link shows whether its file is up to date, modified or missing. The active document's
//! files are looked at every two seconds; a changed file must stay the same for two seconds before
//! it counts (a save or a sync still writing it). A drawing opened with Edit original updates its
//! sheet by itself when it is saved; other changes wait for Update.

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
    let Some((index, _)) = app.active_ids() else { return };
    let page = app.views.get(index).map_or(0, |v| v.current);
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
        let file = linked_file(&path).unwrap_or(LinkedFile { path: path.clone(), size, mtime: 0, now: now_secs() });
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
    /// A changed file as last seen: (size, modified, since when it has looked so).
    seen: HashMap<String, (u64, i64, Instant)>,
    status: HashMap<String, Status>,
    last_poll: Option<Instant>,
    /// Relink: the link and the path being typed.
    relink: Option<(usize, String)>,
}

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

/// What to record about the file a drawing was just opened from.
pub(crate) fn linked_file(path: &str) -> Option<LinkedFile> {
    let (size, mtime) = file_meta(path)?;
    Some(LinkedFile { path: path.to_string(), size, mtime, now: now_secs() })
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
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
    let Some((_, id)) = app.active_ids() else { return 0 };
    let Some(doc) = app.session.get(id) else { return 0 };
    doc.cad_links().iter().filter(|l| matches!(app.links.status.get(&l.path), Some(Status::Modified | Status::Missing))).count()
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

/// Redraw link `index` from its file.
fn update(app: &mut PdfCraftApp, index: usize, saved_in_cad: bool) -> bool {
    let Some((_, id)) = app.active_ids() else { return false };
    let Some(link) = app.session.get(id).and_then(|d| d.cad_links().into_iter().find(|l| l.index == index)) else { return false };
    let name = file_name(&link.path);
    let bytes = match std::fs::read(&link.path) {
        Ok(b) => b,
        Err(e) => {
            app.notify(crate::i18n::fmt(tl!("Couldn't read {name}: {e}"), &[("name", &name), ("e", &e.to_string())]));
            return false;
        }
    };
    let Some(file) = linked_file(&link.path) else { return false };
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
fn edit_original(path: &str) -> Result<(), String> {
    use std::process::Command;
    let spawned = match cadcraft_program() {
        Some(p) => Command::new(p).arg(path).spawn(),
        None if cfg!(windows) => Command::new("cmd").args(["/C", "start", "", path]).spawn(),
        None if cfg!(target_os = "macos") => Command::new("open").arg(path).spawn(),
        None => Command::new("xdg-open").arg(path).spawn(),
    };
    spawned.map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn edit_original(_path: &str) -> Result<(), String> {
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

/// What the pane asked for this frame.
enum Act {
    Update(usize),
    UpdateAll,
    Edit(String),
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
    let links: Vec<CadLink> = doc.cad_links();
    let editable = doc.allows_modification();
    let pages = doc.info.pages.len();
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
        ui.add(egui::Label::new(egui::RichText::new(&sel.path).monospace().size(11.5)).wrap());
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
            Ok(()) => {
                app.links.editing.insert(path.clone());
                app.links.status.insert(path, Status::Editing);
            }
            Err(e) => {
                // Android: CADCraft is opened by hand; the sheet still updates when it's saved.
                app.links.editing.insert(path);
                app.notify(e);
            }
        },
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
            if index != usize::MAX && app.apply_edit(Edit::RelinkCad { index, path: path.clone() }) {
                app.links.status.remove(&path);
                app.links.last_poll = None;
            }
        }
        None => {}
    }
}
