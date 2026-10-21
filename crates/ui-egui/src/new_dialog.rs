//! RevuCraft's New dialog (the tab strip's New, File ▸ New, Home's New or open, Draft's New
//! sheet): start a drawing from a page size on the top half, or open a file from the file browser
//! on the bottom half, after VectorCraft's New Document.
//!
//! Top: size tabs (Recent, ANSI, ARCH, ISO) over page-shaped cards and a Custom card, and the
//! details (width, height, units, orientation, drawing scale, pages, and adding to the open
//! document). Bottom: places (Recent files, Home, Documents, Downloads, Desktop, the computer's
//! drives, pinned folders), the folder path, a search box, the folder's drawings, the file name
//! and type, and Open. Android and the web can't browse folders, so there the bottom half lists
//! recent files and opens the system's file picker.
//!
//! Folders are read lazily, one at a time, capped, and only on a change of folder; names come
//! from the file system and are shown as they are.

use egui::{Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, Stroke, vec2};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

const PT_PER_IN: f64 = 72.0;
/// Entries listed from one folder at most.
const MAX_ENTRIES: usize = 5_000;
/// Recent sizes kept.
const MAX_RECENT: usize = 6;

/// A page size: its name and its portrait width and height in inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub name: &'static str,
    pub width: f64,
    pub height: f64,
}

const fn size(name: &'static str, width: f64, height: f64) -> Size {
    Size { name, width, height }
}

const ANSI: [Size; 6] = [
    size("Letter (A)", 8.5, 11.0),
    size("Legal", 8.5, 14.0),
    size("Tabloid (B)", 11.0, 17.0),
    size("ANSI C", 17.0, 22.0),
    size("ANSI D", 22.0, 34.0),
    size("ANSI E", 34.0, 44.0),
];
const ARCH: [Size; 6] = [
    size("ARCH A", 9.0, 12.0),
    size("ARCH B", 12.0, 18.0),
    size("ARCH C", 18.0, 24.0),
    size("ARCH D", 24.0, 36.0),
    size("ARCH E1", 30.0, 42.0),
    size("ARCH E", 36.0, 48.0),
];
const ISO: [Size; 5] = [
    size("A4", 210.0 / 25.4, 297.0 / 25.4),
    size("A3", 297.0 / 25.4, 420.0 / 25.4),
    size("A2", 420.0 / 25.4, 594.0 / 25.4),
    size("A1", 594.0 / 25.4, 841.0 / 25.4),
    size("A0", 841.0 / 25.4, 1189.0 / 25.4),
];

/// The size tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    Recent,
    #[default]
    Ansi,
    Arch,
    Iso,
}

/// A place in the file browser's side list.
#[derive(Clone, Debug, PartialEq)]
enum Place {
    RecentFiles,
    Folder(std::path::PathBuf),
    /// The computer's drives (Windows) or its root.
    Computer,
}

/// One row of the file list.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    path: std::path::PathBuf,
    name: String,
    folder: bool,
    modified: Option<std::time::SystemTime>,
    size: u64,
}

/// The dialog's state.
#[derive(Clone, Debug, Default)]
pub struct NewDialog {
    pub open: bool,
    pub tab: Tab,
    /// Width and height in inches (portrait).
    pub width: f64,
    pub height: f64,
    pub custom: bool,
    pub metric: bool,
    pub landscape: bool,
    /// Index into the measuring presets; `None`: no drawing scale (paper size).
    pub scale: Option<usize>,
    pub pages: u32,
    /// Add the pages to the open document rather than making a new one.
    pub add_to_document: bool,
    /// Sizes made recently (inches, portrait), newest first.
    pub recent: Vec<(f64, f64)>,
    place: Option<Place>,
    back: Vec<Place>,
    search: String,
    file_name: String,
    all_files: bool,
    listing: Option<(Place, bool, Result<Vec<Entry>, String>)>,
    selected: Option<std::path::PathBuf>,
}

impl NewDialog {
    fn with_defaults(&mut self) {
        if self.width <= 0.0 || self.height <= 0.0 {
            self.width = 11.0;
            self.height = 17.0;
            self.landscape = true;
            self.pages = 1;
            // 1/8" = 1'-0".
            self.scale = crate::measure_ui::PRESETS.iter().position(|p| p.name == "1/8\" = 1'-0\"");
        }
        self.pages = self.pages.clamp(1, 500);
    }

    /// The page size in points, as drawn (landscape turns it).
    fn page_points(&self) -> (f64, f64) {
        let (w, h) = if self.landscape {
            (self.height.max(self.width), self.width.min(self.height))
        } else {
            (self.width.min(self.height), self.height.max(self.width))
        };
        (w * PT_PER_IN, h * PT_PER_IN)
    }
}

impl PdfCraftApp {
    /// Open the New dialog (`add`: from Draft's New sheet, adding to the open document).
    pub(crate) fn show_new_dialog(&mut self, add: bool) {
        let has_doc = self.active_ids().is_some();
        let d = &mut self.new_doc;
        d.with_defaults();
        d.open = true;
        d.add_to_document = add && has_doc;
        d.search.clear();
        d.selected = None;
        d.file_name.clear();
    }
}

fn units(metric: bool) -> &'static str {
    if metric { "mm" } else { "in" }
}

fn shown(inches: f64, metric: bool) -> String {
    if metric { format!("{:.0}", inches * 25.4) } else { format!("{}", (inches * 100.0).round() / 100.0) }
}

/// A size card: a page-shaped picture, the name and the size.
fn card(ui: &mut egui::Ui, t: &Tokens, name: &str, size: Option<(f64, f64)>, metric: bool, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(118.0, 150.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name));
    let fill = if selected {
        t.accent_soft
    } else if resp.hovered() {
        t.hover
    } else {
        t.card
    };
    let stroke = if selected { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.border) };
    ui.painter().rect(rect, CornerRadius::same(8), fill, stroke, egui::StrokeKind::Inside);
    let pic = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(rect.width() - 16.0, 90.0));
    match size {
        Some((w, h)) => {
            let k = 78.0 / w.max(h).max(0.01);
            let page = Rect::from_center_size(pic.center(), vec2((w * k) as f32, (h * k) as f32));
            ui.painter().rect_filled(page.translate(vec2(0.0, 1.5)), CornerRadius::same(2), t.page_shadow);
            ui.painter().rect_filled(page, CornerRadius::same(2), Color32::from_rgb(0xF4, 0xF5, 0xF7));
        }
        None => {
            let r = Rect::from_center_size(pic.center(), vec2(64.0, 64.0));
            ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.5, t.text_faint), egui::StrokeKind::Inside);
            icons::paint(ui, r, "plus", 22.0, t.text_faint);
        }
    }
    ui.painter().text(rect.center_top() + vec2(0.0, 112.0), Align2::CENTER_CENTER, name, theme::medium(12.5), t.text);
    let detail = match size {
        Some((w, h)) => format!("{} × {} {}", shown(w.min(h), metric), shown(w.max(h), metric), units(metric)),
        None => tl!("Your size").to_string(),
    };
    ui.painter().text(rect.center_top() + vec2(0.0, 131.0), Align2::CENTER_CENTER, detail, theme::regular(11.0), t.text_muted);
    resp
}

/// What the dialog was asked to do this frame.
enum Act {
    Create,
    Open(std::path::PathBuf),
    #[cfg_attr(not(any(target_os = "android", target_arch = "wasm32")), allow(dead_code))]
    OpenRecent(String),
    #[cfg_attr(not(any(target_os = "android", target_arch = "wasm32")), allow(dead_code))]
    Picker,
    Close,
}

/// Draw the dialog when it is open.
pub fn show(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if !app.new_doc.open {
        return;
    }
    let t = Tokens::get(ctx);
    let has_doc = app.active_ids().is_some();
    let recent_files: Vec<(String, String)> = app.recent.iter().map(|r| (r.name.clone(), r.path.clone())).collect();
    let pinned: Vec<String> = app.pinned.folders.clone();
    let mut act: Option<Act> = None;
    let screen = ctx.content_rect();
    let width = (screen.width() - 48.0).clamp(360.0, 1100.0);
    let height = (screen.height() - 60.0).clamp(420.0, 800.0);
    let modal = egui::Modal::new(egui::Id::new("revucraft-new")).show(ctx, |ui| {
        ui.set_width(width);
        ui.set_max_height(height);
        let d = &mut app.new_doc;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tl!("New")).font(theme::semibold(17.0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, "x", 30.0, false, tl!("Close")).clicked() {
                    act = Some(Act::Close);
                }
            });
        });
        ui.separator();
        // ── Top half: a new drawing ───────────────────────────────────────────────────────
        ui.horizontal_top(|ui| {
            let details_w = 270.0_f32.min(width * 0.4);
            ui.vertical(|ui| {
                ui.set_width((width - details_w - 20.0).max(160.0));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tl!("Start a new drawing")).font(theme::semibold(12.0)).color(t.text_muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(egui::RichText::new(tl!("Double-click a size to create it")).small().color(t.text_faint));
                    });
                });
                ui.horizontal(|ui| {
                    for (tab, label) in [(Tab::Recent, tl!("Recent")), (Tab::Ansi, "ANSI"), (Tab::Arch, "ARCH"), (Tab::Iso, tl!("ISO (A sizes)"))] {
                        if ui.selectable_label(d.tab == tab, label).clicked() {
                            d.tab = tab;
                        }
                    }
                });
                ui.add_space(4.0);
                egui::ScrollArea::horizontal().id_salt("new-sizes").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let list: Vec<(String, (f64, f64))> = match d.tab {
                            Tab::Recent => {
                                d.recent.iter().map(|(w, h)| (format!("{} × {}", shown(*w, d.metric), shown(*h, d.metric)), (*w, *h))).collect()
                            }
                            Tab::Ansi => ANSI.iter().map(|s| (tl!(s.name).to_string(), (s.width, s.height))).collect(),
                            Tab::Arch => ARCH.iter().map(|s| (s.name.to_string(), (s.width, s.height))).collect(),
                            Tab::Iso => ISO.iter().map(|s| (s.name.to_string(), (s.width, s.height))).collect(),
                        };
                        if list.is_empty() {
                            ui.label(egui::RichText::new(tl!("Sizes you make appear here.")).color(t.text_muted));
                        }
                        for (label, (w, h)) in list {
                            let selected = !d.custom && (d.width - w).abs() < 1e-6 && (d.height - h).abs() < 1e-6;
                            let shape = if d.landscape { (h, w) } else { (w, h) };
                            let resp = card(ui, &t, &label, Some(shape), d.metric, selected);
                            if resp.clicked() || resp.double_clicked() {
                                d.width = w;
                                d.height = h;
                                d.custom = false;
                            }
                            if resp.double_clicked() {
                                act = Some(Act::Create);
                            }
                        }
                        if card(ui, &t, tl!("Custom"), None, d.metric, d.custom).clicked() {
                            d.custom = true;
                        }
                    });
                });
            });
            // Details.
            egui::Frame::NONE
                .fill(t.card)
                .stroke(Stroke::new(1.0, t.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(details_w - 24.0);
                        ui.label(egui::RichText::new(tl!("Details")).font(theme::semibold(12.0)).color(t.text_muted));
                        let k = if d.metric { 25.4 } else { 1.0 };
                        egui::Grid::new("new-details").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.label(tl!("Width"));
                            let mut w = d.width * k;
                            if ui
                                .add(egui::DragValue::new(&mut w).range(1.0 * k..=200.0 * k).speed(0.1).suffix(format!(" {}", units(d.metric))))
                                .changed()
                            {
                                d.width = w / k;
                                d.custom = true;
                            }
                            ui.end_row();
                            ui.label(tl!("Height"));
                            let mut h = d.height * k;
                            if ui
                                .add(egui::DragValue::new(&mut h).range(1.0 * k..=200.0 * k).speed(0.1).suffix(format!(" {}", units(d.metric))))
                                .changed()
                            {
                                d.height = h / k;
                                d.custom = true;
                            }
                            ui.end_row();
                            ui.label(tl!("Units"));
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut d.metric, false, tl!("Inches"));
                                ui.selectable_value(&mut d.metric, true, tl!("Millimetres"));
                            });
                            ui.end_row();
                            ui.label(tl!("Orientation"));
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut d.landscape, false, tl!("Portrait"));
                                ui.selectable_value(&mut d.landscape, true, tl!("Landscape"));
                            });
                            ui.end_row();
                            ui.label(tl!("Drawing scale"));
                            let presets = &crate::measure_ui::PRESETS;
                            let current =
                                d.scale.and_then(|i| presets.get(i)).map_or_else(|| tl!("None (paper size)").to_string(), |p| p.name.to_string());
                            egui::ComboBox::from_id_salt("new-scale").selected_text(current).width(140.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut d.scale, None, tl!("None (paper size)"));
                                for (i, p) in presets.iter().enumerate() {
                                    ui.selectable_value(&mut d.scale, Some(i), p.name);
                                }
                            });
                            ui.end_row();
                            ui.label(tl!("Pages"));
                            ui.add(egui::DragValue::new(&mut d.pages).range(1..=500));
                            ui.end_row();
                        });
                        if has_doc {
                            ui.add(crate::widgets::checkbox(&mut d.add_to_document, tl!("Add to the open document")));
                        }
                        ui.add_space(6.0);
                        if widgets::pill_button(ui, tl!("Create"), true).clicked() {
                            act = Some(Act::Create);
                        }
                    });
                });
        });
        ui.add_space(8.0);
        ui.separator();
        // ── Bottom half: open a file ──────────────────────────────────────────────────────
        ui.label(egui::RichText::new(tl!("Open a file")).font(theme::semibold(12.0)).color(t.text_muted));
        #[cfg(any(target_os = "android", target_arch = "wasm32"))]
        {
            let _ = &pinned;
            ui.horizontal(|ui| {
                if widgets::pill_button(ui, tl!("Open from device…"), true).clicked() {
                    act = Some(Act::Picker);
                }
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("new-recent").max_height(220.0).show(ui, |ui| {
                if recent_files.is_empty() {
                    ui.label(egui::RichText::new(tl!("Files you open appear here.")).color(t.text_muted));
                }
                for (name, path) in &recent_files {
                    if ui.selectable_label(false, name.as_str()).on_hover_text(path.as_str()).clicked() {
                        act = Some(Act::OpenRecent(path.clone()));
                    }
                }
            });
        }
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        browser(ui, &t, d, &recent_files, &pinned, &mut act);
    });
    if modal.should_close() && act.is_none() {
        act = Some(Act::Close);
    }
    match act {
        Some(Act::Close) => app.new_doc.open = false,
        Some(Act::Create) => {
            app.new_doc.open = false;
            create(app);
        }
        Some(Act::Open(path)) => {
            app.new_doc.open = false;
            app.open_path(&path.to_string_lossy());
        }
        Some(Act::OpenRecent(path)) => {
            app.new_doc.open = false;
            app.open_recent(&path);
        }
        Some(Act::Picker) => {
            app.new_doc.open = false;
            app.open_dialog();
        }
        None => {}
    }
}

/// Make the drawing: a new document (or pages added to the open one) at the size, with the
/// drawing scale on every page.
fn create(app: &mut PdfCraftApp) {
    let d = app.new_doc.clone();
    let (w, h) = d.page_points();
    if !(w.is_finite() && h.is_finite() && w >= 72.0 && h >= 72.0 && w <= 14_400.0 && h <= 14_400.0) {
        app.notify(tl!("That page size can't be made; pages are 1 to 200 inches a side.").to_string());
        return;
    }
    // Recent sizes, newest first.
    let size = (d.width, d.height);
    app.new_doc.recent.retain(|r| (r.0 - size.0).abs() > 1e-6 || (r.1 - size.1).abs() > 1e-6);
    app.new_doc.recent.insert(0, size);
    app.new_doc.recent.truncate(MAX_RECENT);
    let scale = match d.scale.and_then(|i| crate::measure_ui::PRESETS.get(i)) {
        Some(p) => match crate::measure_ui::preset_scale(p) {
            Ok(s) => Some((p.name, s)),
            Err(e) => {
                app.notify(e);
                return;
            }
        },
        None => None,
    };
    let pages = d.pages.clamp(1, 500) as usize;
    let set_scale = |page: usize| {
        scale.as_ref().map(|(name, s)| pdfcraft_engine::Edit::SetMeasurementScale {
            page,
            bbox: [0.0, 0.0, w, h],
            name: name.to_string(),
            scale: s.clone(),
        })
    };
    let existing = app.active_ids().filter(|_| d.add_to_document).and_then(|(i, id)| app.session.get(id).map(|doc| (i, doc.info.pages.len())));
    match existing {
        Some((index, first)) => {
            let mut edits = Vec::new();
            for k in 0..pages {
                edits.push(pdfcraft_engine::Edit::InsertBlankPage { at: first + k, width: w, height: h });
                edits.extend(set_scale(first + k));
            }
            if app.apply_edit(pdfcraft_engine::Edit::Batch { label: "New sheet".into(), edits })
                && let Some(view) = app.views.get_mut(index)
            {
                view.go_to_page(first);
            }
        }
        None => {
            let created = app.session.create_blank(w, h, pages).map_err(|e| e.to_string());
            match app.open_created_bytes("Drawing.pdf", created) {
                Ok(()) => {
                    let edits: Vec<_> = (0..pages).filter_map(set_scale).collect();
                    if !edits.is_empty() {
                        app.apply_edit(pdfcraft_engine::Edit::Batch { label: "Set measurement scale".into(), edits });
                    }
                }
                Err(e) => {
                    app.notify(e);
                    return;
                }
            }
        }
    }
    app.workflow = crate::ribbon::Workflow::Draft;
    app.quick_tool = crate::QuickTool::Draft(crate::draft::DraftTool::Wall);
}

// ── the file browser (desktop) ─────────────────────────────────────────────────────────────

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from)
}

#[cfg_attr(any(target_os = "android", target_arch = "wasm32"), allow(dead_code))]
/// Files the browser lists unless "All files" is chosen: PDFs and what converts to PDF.
fn openable(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext == "pdf" || pdfcraft_engine::CONVERTIBLE.contains(&ext) || pdfcraft_engine::CAD_EXTENSIONS.contains(&ext))
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn read_place(place: &Place, all: bool, recent: &[(String, String)]) -> Result<Vec<Entry>, String> {
    let entry = |path: std::path::PathBuf, folder: bool| {
        let meta = std::fs::metadata(&path).ok();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string_lossy().into_owned());
        Entry { name, folder, modified: meta.as_ref().and_then(|m| m.modified().ok()), size: meta.map_or(0, |m| m.len()), path }
    };
    match place {
        Place::RecentFiles => Ok(recent.iter().map(|(_, p)| entry(std::path::PathBuf::from(p), false)).collect()),
        Place::Computer => {
            if cfg!(windows) {
                Ok(('C'..='Z')
                    .map(|c| std::path::PathBuf::from(format!("{c}:\\")))
                    .filter(|p| p.exists())
                    .map(|p| Entry { name: p.to_string_lossy().into_owned(), folder: true, modified: None, size: 0, path: p })
                    .collect())
            } else {
                read_place(&Place::Folder(std::path::PathBuf::from("/")), all, recent)
            }
        }
        Place::Folder(dir) => {
            let mut out = Vec::new();
            for e in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().take(MAX_ENTRIES) {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let folder = e.file_type().is_ok_and(|t| t.is_dir());
                if folder || all || openable(&name) {
                    out.push(entry(e.path(), folder));
                }
            }
            out.sort_by(|a, b| b.folder.cmp(&a.folder).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
            Ok(out)
        }
    }
}

#[cfg_attr(any(target_os = "android", target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn modified_label(m: Option<std::time::SystemTime>) -> String {
    let Some(secs) = m.and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64) else { return String::new() };
    // Days since 1970 to a calendar date (civil-from-days).
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02}")
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn browser(ui: &mut egui::Ui, t: &Tokens, d: &mut NewDialog, recent: &[(String, String)], pinned: &[String], act: &mut Option<Act>) {
    let home = home();
    if d.place.is_none() {
        d.place = Some(match pinned.first() {
            Some(p) => Place::Folder(std::path::PathBuf::from(p)),
            None => home.clone().map_or(Place::RecentFiles, Place::Folder),
        });
    }
    let place = d.place.clone().unwrap_or(Place::RecentFiles);
    if d.listing.as_ref().is_none_or(|(p, all, _)| *p != place || *all != d.all_files) {
        d.listing = Some((place.clone(), d.all_files, read_place(&place, d.all_files, recent)));
    }
    let mut go: Option<Place> = None;
    // The path and search.
    ui.horizontal(|ui| {
        if icons::button(ui, "chevron-left", 26.0, false, tl!("Back")).clicked()
            && let Some(p) = d.back.pop()
        {
            d.place = Some(p);
            d.selected = None;
        }
        if let Place::Folder(dir) = &place
            && let Some(parent) = dir.parent()
            && icons::button(ui, "chevron-up", 26.0, false, tl!("Up a folder")).clicked()
        {
            go = Some(Place::Folder(parent.to_path_buf()));
        }
        let label = match &place {
            Place::RecentFiles => tl!("Recent files").to_string(),
            Place::Computer => tl!("This computer").to_string(),
            Place::Folder(dir) => dir.to_string_lossy().into_owned(),
        };
        ui.add(egui::Label::new(egui::RichText::new(label).color(t.text)).truncate());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::TextEdit::singleline(&mut d.search).hint_text(tl!("Search this folder")).desired_width(200.0));
        });
    });
    let list_h = (ui.available_height() - 46.0).clamp(140.0, 320.0);
    ui.horizontal_top(|ui| {
        // Places.
        ui.vertical(|ui| {
            ui.set_width(170.0);
            let mut places: Vec<(String, Place)> = vec![(tl!("Recent files").to_string(), Place::RecentFiles)];
            if let Some(h) = &home {
                places.push((tl!("Home").to_string(), Place::Folder(h.clone())));
                for (name, sub) in [(tl!("Documents"), "Documents"), (tl!("Downloads"), "Downloads"), (tl!("Desktop"), "Desktop")] {
                    let p = h.join(sub);
                    if p.is_dir() {
                        places.push((name.to_string(), Place::Folder(p)));
                    }
                }
            }
            places.push((tl!("This computer").to_string(), Place::Computer));
            for p in pinned {
                let path = std::path::PathBuf::from(p);
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
                places.push((name, Place::Folder(path)));
            }
            egui::ScrollArea::vertical().id_salt("new-places").max_height(list_h).show(ui, |ui| {
                for (name, p) in places {
                    if ui.selectable_label(place == p, name).clicked() && place != p {
                        go = Some(p);
                    }
                }
            });
        });
        // Files.
        egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).corner_radius(CornerRadius::same(6)).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.set_height(list_h);
                let q = d.search.trim().to_lowercase();
                match d.listing.as_ref().map(|(_, _, l)| l) {
                    Some(Err(e)) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    Some(Ok(entries)) => {
                        let visible: Vec<&Entry> = entries.iter().filter(|e| q.is_empty() || e.name.to_lowercase().contains(&q)).collect();
                        if visible.is_empty() {
                            ui.label(egui::RichText::new(tl!("No drawings here.")).color(t.text_muted));
                        }
                        egui::ScrollArea::vertical().id_salt("new-files").auto_shrink([false, false]).show_rows(
                            ui,
                            26.0,
                            visible.len(),
                            |ui, range| {
                                for e in visible.get(range).unwrap_or_default() {
                                    let selected = d.selected.as_ref() == Some(&e.path);
                                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                                    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &e.name));
                                    if selected {
                                        ui.painter().rect_filled(rect, CornerRadius::same(4), t.accent_soft);
                                    } else if resp.hovered() {
                                        ui.painter().rect_filled(rect, CornerRadius::same(4), t.hover);
                                    }
                                    let icon = if e.folder { "folder" } else { "file-text" };
                                    let tint = if e.folder { t.text_muted } else { t.tool_icon };
                                    icons::paint(ui, Rect::from_min_size(rect.min + vec2(6.0, 4.0), vec2(18.0, 18.0)), icon, 16.0, tint);
                                    let date = modified_label(e.modified);
                                    let size = if e.folder { String::new() } else { crate::panels::human_size(e.size as usize) };
                                    ui.painter().text(
                                        rect.right_center() - vec2(8.0, 0.0),
                                        Align2::RIGHT_CENTER,
                                        size,
                                        theme::regular(11.5),
                                        t.text_muted,
                                    );
                                    ui.painter().text(
                                        rect.right_center() - vec2(96.0, 0.0),
                                        Align2::RIGHT_CENTER,
                                        date,
                                        theme::regular(11.5),
                                        t.text_muted,
                                    );
                                    widgets::row_text(
                                        ui,
                                        rect.min + vec2(30.0, 13.0),
                                        e.name.clone(),
                                        theme::regular(13.0),
                                        t.text,
                                        (rect.width() - 220.0).max(60.0),
                                    );
                                    if resp.clicked() {
                                        d.selected = Some(e.path.clone());
                                        if !e.folder {
                                            d.file_name = e.name.clone();
                                        }
                                    }
                                    if resp.double_clicked() {
                                        if e.folder {
                                            go = Some(Place::Folder(e.path.clone()));
                                        } else {
                                            *act = Some(Act::Open(e.path.clone()));
                                        }
                                    }
                                }
                            },
                        );
                    }
                    None => {}
                }
            });
        });
    });
    // The file name, type and Open.
    ui.horizontal(|ui| {
        ui.label(tl!("File"));
        let r = ui.add(egui::TextEdit::singleline(&mut d.file_name).desired_width(320.0));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        egui::ComboBox::from_id_salt("new-filter")
            .selected_text(if d.all_files { tl!("All files") } else { tl!("PDF, drawings and images") })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut d.all_files, false, tl!("PDF, drawings and images"));
                ui.selectable_value(&mut d.all_files, true, tl!("All files"));
            });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let open = widgets::pill_button(ui, tl!("Open"), true).clicked();
            if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                *act = Some(Act::Close);
            }
            if open || enter {
                // A typed name is in the folder shown, unless it is a whole path.
                let typed = d.file_name.trim();
                let path = if typed.is_empty() {
                    d.selected.clone()
                } else {
                    let p = std::path::PathBuf::from(typed);
                    Some(match (&place, p.is_absolute()) {
                        (Place::Folder(dir), false) => dir.join(p),
                        _ => p,
                    })
                };
                match path {
                    Some(p) if p.is_dir() => go = Some(Place::Folder(p)),
                    Some(p) => *act = Some(Act::Open(p)),
                    None => {}
                }
            }
        });
    });
    if let Some(p) = go {
        d.back.push(place);
        if d.back.len() > 50 {
            d.back.remove(0);
        }
        d.place = Some(p);
        d.selected = None;
        d.search.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_turn_and_dates_read() {
        let mut d = NewDialog::default();
        d.with_defaults();
        assert_eq!(d.page_points(), (17.0 * 72.0, 11.0 * 72.0), "Tabloid, landscape");
        d.landscape = false;
        assert_eq!(d.page_points(), (11.0 * 72.0, 17.0 * 72.0));
        assert_eq!(modified_label(Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_792_195_200))), "2026-10-17");
        assert_eq!(modified_label(None), "");
        assert!(openable("Plan.PDF") && openable("photo.jpg") && !openable("notes.docx") && !openable("pdf"));
    }
}
