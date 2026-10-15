//! RevuCraft's field tools: the Symbols ribbon tab (pick a symbol, its layer, size and label,
//! then tap to stamp it again and again), the layer manager in the Layers panel, symbol counts,
//! and photo callouts (pin a picture to the plan; tapping the pin shows it).

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, pos2, vec2};
use pdfcraft_engine::symbols::{NewSymbol, Symbol};
use pdfcraft_engine::{DocId, Edit};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, icons};

/// Layer colours offered when making or recolouring a layer (CAD-style primaries first).
pub const LAYER_COLORS: [[f64; 3]; 10] = [
    [0.90, 0.13, 0.13],
    [0.96, 0.62, 0.04],
    [0.93, 0.85, 0.10],
    [0.18, 0.66, 0.30],
    [0.05, 0.65, 0.75],
    [0.00, 0.40, 0.90],
    [0.45, 0.30, 0.85],
    [0.85, 0.25, 0.65],
    [0.45, 0.45, 0.45],
    [0.00, 0.00, 0.00],
];

/// Symbol colour when no layer is chosen.
const NO_LAYER_COLOR: [f64; 3] = [0.0, 0.40, 0.90];
/// Size on paper when the page has no scale (a quarter inch).
const PAPER_FALLBACK_PT: f64 = 18.0;

/// The Symbols tab's settings.
#[derive(Clone, Debug)]
pub struct SymbolSettings {
    pub symbol: Symbol,
    /// Real-world size (at the page's measurement scale) rather than size on paper.
    pub real_size: bool,
    /// Real-world width, inches.
    pub real_inches: f64,
    /// Width on paper, inches.
    pub paper_inches: f64,
    pub label: String,
    /// Append a number to the label, counting up with each symbol placed.
    pub auto_number: bool,
    pub next: u32,
}

impl Default for SymbolSettings {
    fn default() -> Self {
        Self { symbol: Symbol::DataJack, real_size: true, real_inches: 12.0, paper_inches: 0.25, label: String::new(), auto_number: false, next: 1 }
    }
}

impl SymbolSettings {
    /// The label the next symbol gets.
    pub fn label_now(&self) -> String {
        let label = self.label.trim();
        match (self.auto_number, label.is_empty()) {
            (false, _) => label.to_string(),
            (true, true) => self.next.to_string(),
            (true, false) => format!("{label}-{}", self.next),
        }
    }
}

/// The Layers panel's own state.
#[derive(Clone, Debug, Default)]
pub struct LayerPanel {
    pub new_name: String,
    /// The layer being renamed, and the name typed so far.
    pub renaming: Option<((u32, u16), String)>,
}

/// A photo shown full size.
pub struct PhotoViewer {
    pub doc: DocId,
    pub page: usize,
    pub index: usize,
    pub caption: String,
    pub texture: Result<egui::TextureHandle, String>,
}

fn color32(c: [f64; 3]) -> Color32 {
    let v = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(v(c[0]), v(c[1]), v(c[2]))
}

fn layer_color(app: &PdfCraftApp, id: DocId) -> [f64; 3] {
    let current = app.session.get(id).and_then(|d| d.current_layer);
    current.and_then(|c| app.session.layer_details(id).into_iter().find(|l| (l.id.num, l.id.generation) == c)).map_or(NO_LAYER_COLOR, |l| l.color)
}

/// Draw a library symbol's artwork in `rect` (a preview of what is stamped), from its content
/// operators: paths (`m l c re h`), strokes and fills, and its tags.
pub fn paint_symbol(painter: &egui::Painter, rect: Rect, symbol: Symbol, color: Color32) {
    let k = rect.width().min(rect.height()) / 100.0;
    let origin = pos2(rect.center().x - 50.0 * k, rect.center().y + 50.0 * k);
    let at = |x: f32, y: f32| pos2(origin.x + x * k, origin.y - y * k);
    let stroke = Stroke::new((6.0 * k).max(1.0), color);
    let mut nums: Vec<f32> = Vec::new();
    let mut paths: Vec<(Vec<Pos2>, bool)> = Vec::new();
    let mut current: Vec<Pos2> = Vec::new();
    let mut font = 20.0f32;
    let mut text_at = pos2(0.0, 0.0);
    let art = symbol.artwork();
    let mut rest = art.as_str();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(after) = rest.strip_prefix('(') {
            let end = after.find(')').unwrap_or(after.len());
            let text = after.get(..end).unwrap_or_default();
            painter.text(text_at, Align2::LEFT_BOTTOM, text, egui::FontId::proportional((font * k).max(4.0)), color);
            rest = after.get(end + 1..).unwrap_or_default();
            continue;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = rest.get(..end).unwrap_or_default();
        rest = rest.get(end..).unwrap_or_default();
        if let Ok(v) = token.parse::<f32>() {
            nums.push(v);
            continue;
        }
        let take = |nums: &mut Vec<f32>, n: usize| -> Option<Vec<f32>> {
            let start = nums.len().checked_sub(n)?;
            Some(nums.split_off(start))
        };
        match token {
            "m" => {
                if current.len() > 1 {
                    paths.push((std::mem::take(&mut current), false));
                }
                current.clear();
                if let Some(v) = take(&mut nums, 2) {
                    current.push(at(v[0], v[1]));
                }
            }
            "l" => {
                if let Some(v) = take(&mut nums, 2) {
                    current.push(at(v[0], v[1]));
                }
            }
            "c" => {
                if let (Some(v), Some(&p0)) = (take(&mut nums, 6), current.last()) {
                    let (p1, p2, p3) = (at(v[0], v[1]), at(v[2], v[3]), at(v[4], v[5]));
                    for i in 1..=8 {
                        let t = i as f32 / 8.0;
                        let u = 1.0 - t;
                        let x = u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x;
                        let y = u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y;
                        current.push(pos2(x, y));
                    }
                }
            }
            "re" => {
                if let Some(v) = take(&mut nums, 4) {
                    let (x, y, w, h) = (v[0], v[1], v[2], v[3]);
                    paths.push((vec![at(x, y), at(x + w, y), at(x + w, y + h), at(x, y + h)], true));
                }
            }
            "h" => {
                if current.len() > 1 {
                    paths.push((std::mem::take(&mut current), true));
                }
            }
            "S" | "f" => {
                if current.len() > 1 {
                    paths.push((std::mem::take(&mut current), token == "f"));
                }
                for (points, closed) in paths.drain(..) {
                    if token == "f" {
                        painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
                    } else if closed {
                        painter.add(egui::Shape::closed_line(points, stroke));
                    } else {
                        painter.add(egui::Shape::line(points, stroke));
                    }
                }
                current.clear();
                nums.clear();
            }
            "Tf" => {
                if let Some(v) = take(&mut nums, 1) {
                    font = v[0];
                }
            }
            "Td" => {
                if let Some(v) = take(&mut nums, 2) {
                    text_at = at(v[0], v[1]);
                }
            }
            _ => nums.clear(),
        }
    }
}

/// The Symbols ribbon tab.
pub fn ribbon(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((_, id)) = app.active_ids() else { return };
    let allowed = app.session.get(id).is_some_and(|d| d.allows_annotation() && d.read_only_reason.is_none());
    let color = color32(layer_color(app, id));
    let separator = |ui: &mut egui::Ui| {
        ui.add_space(6.0);
        let x = ui.cursor().left();
        ui.painter().vline(x, ui.max_rect().y_range().shrink(6.0), Stroke::new(1.0, t.divider));
        ui.add_space(6.0);
    };
    ui.add_enabled_ui(allowed, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // The symbol: a preview button that opens the library.
            group(ui, &t, tl!("Symbol"), |ui| {
                let placing = app.quick_tool == QuickTool::Symbol;
                let current = app.symbols.symbol;
                let resp = ui
                    .horizontal(|ui| {
                        let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
                        let fill = if placing { t.accent_soft } else if resp.hovered() { t.hover } else { Color32::TRANSPARENT };
                        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                        paint_symbol(ui.painter(), rect.shrink(5.0), current, color);
                        let name = egui::RichText::new(crate::i18n::t(current.name())).font(theme::regular(12.0));
                        let _ = ui.menu_button(name, |ui| {
                            library(ui, &t, current, color, &mut app.symbols.symbol, &mut app.quick_tool);
                        });
                        resp.on_hover_text(tl!("Tap the drawing to place this symbol")).clicked()
                    })
                    .inner;
                if resp {
                    app.quick_tool = if placing { QuickTool::Select } else { QuickTool::Symbol };
                }
            });
            separator(ui);
            group(ui, &t, tl!("Layer"), |ui| layer_combo(app, ui, id));
            separator(ui);
            group(ui, &t, tl!("Size"), |ui| {
                let s = &mut app.symbols;
                let _ = ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.real_size, true, tl!("Real"));
                    ui.selectable_value(&mut s.real_size, false, tl!("Paper"));
                    if s.real_size {
                        ui.add(egui::DragValue::new(&mut s.real_inches).range(0.5..=600.0).speed(0.5).suffix(" in"));
                    } else {
                        ui.add(egui::DragValue::new(&mut s.paper_inches).range(0.05..=4.0).speed(0.01).suffix(" in"));
                    }
                })
                .response
                .on_hover_text(tl!(
                    "Real: the symbol's width in the real world, at the page's measurement scale (pages without a scale use 1/4 inch on paper). Paper: its width on the printed sheet."
                ));
            });
            separator(ui);
            group(ui, &t, tl!("Label"), |ui| {
                let s = &mut app.symbols;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.label).desired_width(70.0).hint_text(tl!("PC, VoIP…")));
                    let _ = ui.checkbox(&mut s.auto_number, "#").on_hover_text(tl!("Number each symbol, counting up"));
                    if s.auto_number {
                        ui.add(egui::DragValue::new(&mut s.next).range(0..=99_999));
                    }
                });
            });
            separator(ui);
            group(ui, &t, tl!("Photos"), |ui| {
                let pinning = app.quick_tool == QuickTool::PhotoPin;
                if icons::button(ui, "camera", 34.0, pinning, tl!("Pin a photo: tap the drawing where it goes")).clicked() {
                    app.quick_tool = if pinning { QuickTool::Select } else { QuickTool::PhotoPin };
                }
            });
            separator(ui);
            group(ui, &t, tl!("Counts"), |ui| {
                ui.horizontal(|ui| {
                    let n = app.session.placed_symbols(id).len();
                    ui.label(egui::RichText::new(n.to_string()).font(theme::semibold(16.0)));
                    if icons::button(ui, "file-spreadsheet", 30.0, false, tl!("Export symbol counts as CSV")).clicked() {
                        export_counts(app);
                    }
                });
            });
        });
    });
}

/// A ribbon group: its controls, with the caption underneath.
fn group(ui: &mut egui::Ui, t: &Tokens, caption: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(vec2(0.0, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        body(ui);
        ui.label(egui::RichText::new(caption).font(theme::regular(11.0)).color(t.text_faint));
    });
}

/// The symbol library: a grid of previews.
fn library(ui: &mut egui::Ui, t: &Tokens, current: Symbol, color: Color32, chosen: &mut Symbol, tool: &mut QuickTool) {
    ui.label(egui::RichText::new(tl!("Networking")).font(theme::semibold(12.0)).color(t.text_muted));
    egui::Grid::new("symbol-library").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        for (i, symbol) in Symbol::ALL.into_iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(64.0, 60.0), Sense::click());
            let fill = if symbol == current {
                t.accent_soft
            } else if resp.hovered() {
                t.hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
            paint_symbol(ui.painter(), Rect::from_center_size(rect.center() - vec2(0.0, 7.0), vec2(36.0, 36.0)), symbol, color);
            let name = crate::i18n::t(symbol.name());
            let short: String = name.chars().take(11).collect();
            ui.painter().text(rect.center_bottom() - vec2(0.0, 3.0), Align2::CENTER_BOTTOM, short, theme::regular(10.0), t.text_muted);
            if resp.on_hover_text(name).clicked() {
                *chosen = symbol;
                *tool = QuickTool::Symbol;
                ui.close();
            }
            if i % 5 == 4 {
                ui.end_row();
            }
        }
    });
}

/// The current layer, as a drop-down.
fn layer_combo(app: &mut PdfCraftApp, ui: &mut egui::Ui, id: DocId) {
    let layers = app.session.layer_details(id);
    let current = app.session.get(id).and_then(|d| d.current_layer);
    let name_of = |c: Option<(u32, u16)>| {
        c.and_then(|c| layers.iter().find(|l| (l.id.num, l.id.generation) == c)).map_or_else(|| tl!("No layer").to_string(), |l| l.name.clone())
    };
    let mut choice = current;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("symbol-layer").selected_text(name_of(current)).width(120.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut choice, None, tl!("No layer"));
            for l in &layers {
                let key = Some((l.id.num, l.id.generation));
                let mut job = egui::text::LayoutJob::default();
                job.append("● ", 0.0, egui::TextFormat { color: color32(l.color), ..Default::default() });
                job.append(&l.name, 0.0, egui::TextFormat { color: ui.visuals().text_color(), ..Default::default() });
                ui.selectable_value(&mut choice, key, job);
            }
        });
        if icons::button(ui, "layers", 26.0, false, tl!("Manage layers")).clicked() {
            app.choose_right_panel(Some(crate::RightPanel::Layers));
        }
    });
    if choice != current {
        let _ = app.session.set_current_layer(id, choice);
    }
}

/// The edit that places the chosen symbol at `at` on `page`.
pub fn symbol_edit(s: &SymbolSettings, doc: &pdfcraft_engine::Document, page: usize, at: [f64; 2], author: &str) -> Edit {
    let size = if s.real_size { doc.real_size_points(page, at, s.real_inches).unwrap_or(PAPER_FALLBACK_PT) } else { s.paper_inches * 72.0 };
    let current = doc.current_layer;
    let color = current.and_then(|c| doc.layer_list().into_iter().find(|l| (l.id.num, l.id.generation) == c)).map_or(NO_LAYER_COLOR, |l| l.color);
    Edit::AddSymbol(NewSymbol {
        page,
        symbol: s.symbol,
        center: at,
        size: size.clamp(1.0, 20_000.0),
        label: s.label_now(),
        color,
        author: author.to_string(),
    })
}

/// The Layers panel: show and hide, make, rename, recolour and lock layers, choose the current
/// one, and move the selected markup to a layer.
pub fn layers_panel(app: &mut PdfCraftApp, ui: &mut egui::Ui, index: usize, id: DocId) {
    let t = Tokens::get(ui.ctx());
    let Some(doc) = app.session.get(id) else { return };
    let editable = doc.allows_annotation() && doc.read_only_reason.is_none();
    let shown: Vec<((u32, u16), String, bool)> = doc.info.layers.iter().map(|l| (l.id, l.name.clone(), l.visible)).collect();
    let details = app.session.layer_details(id);
    let current = doc.current_layer;
    let selected = app.views.get(index).and_then(|v| v.comments.selected);
    let selected_layer = selected.and_then(|(p, i)| doc.info.annotations.iter().find(|a| a.page == p && a.index == i)).map(|a| a.layer);
    let mut edit: Option<Edit> = None;
    let mut toggle: Option<(usize, bool)> = None;
    let mut set_current: Option<Option<(u32, u16)>> = None;
    let panel = &mut app.layer_panel;

    ui.add_enabled_ui(editable, |ui| {
        ui.horizontal(|ui| {
            let field =
                ui.add(egui::TextEdit::singleline(&mut panel.new_name).desired_width(ui.available_width() - 90.0).hint_text(tl!("New layer name")));
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button(tl!("Add layer")).clicked() || enter) && !panel.new_name.trim().is_empty() {
                let color = LAYER_COLORS.get(details.len() % LAYER_COLORS.len()).copied().unwrap_or(NO_LAYER_COLOR);
                edit = Some(Edit::AddLayer { name: panel.new_name.trim().to_string(), color });
                panel.new_name.clear();
            }
        });
    });
    ui.add_space(6.0);
    if shown.is_empty() {
        ui.label(
            egui::RichText::new(tl!("No layers yet. Add one, then choose it as the current layer: new markups, symbols and photos go on it."))
                .color(t.text_muted),
        );
    }
    // "No layer" as a choice for the current layer.
    let row = |ui: &mut egui::Ui| ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover()).0;
    {
        let r = row(ui);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let text = egui::RichText::new(tl!("No layer")).color(if current.is_none() { t.text } else { t.text_muted });
        child.add_space(6.0);
        if child.radio(current.is_none(), text).clicked() {
            set_current = Some(None);
        }
    }
    for (li, (key, name, visible)) in shown.iter().enumerate() {
        let detail = details.iter().find(|d| (d.id.num, d.id.generation) == *key);
        let ours = detail.is_some();
        let color = detail.map_or(Color32::GRAY, |d| color32(d.color));
        let locked = detail.is_some_and(|d| d.locked);
        let is_current = current == Some(*key);
        let r = row(ui);
        if is_current {
            ui.painter().rect_filled(r, CornerRadius::same(6), t.accent_soft);
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 4.0;
        if icons::button(ui, if *visible { "eye" } else { "eye-off" }, 26.0, false, if *visible { tl!("Hide layer") } else { tl!("Show layer") })
            .clicked()
        {
            toggle = Some((li, !visible));
        }
        if ours {
            ui.add_enabled_ui(editable, |ui| {
                let _ = ui
                    .menu_button(egui::RichText::new("●").color(color).font(theme::regular(16.0)), |ui| {
                        ui.horizontal(|ui| {
                            for c in LAYER_COLORS {
                                let (sw, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                                ui.painter().circle_filled(sw.center(), 9.0, color32(c));
                                if resp.clicked() {
                                    edit = Some(Edit::SetLayerColor { layer: *key, color: c });
                                    ui.close();
                                }
                            }
                        });
                    })
                    .response
                    .on_hover_text(tl!("Layer colour"));
            });
        }
        match &mut panel.renaming {
            Some((k, text)) if k == key => {
                let field = ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width() - 64.0));
                field.request_focus();
                let done = field.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                if done {
                    if !text.trim().is_empty() && text.trim() != name.as_str() {
                        edit = Some(Edit::RenameLayer { layer: *key, name: text.trim().to_string() });
                    }
                    panel.renaming = None;
                }
            }
            _ => {
                let font = if is_current { theme::semibold(13.0) } else { theme::regular(13.0) };
                let label = egui::Label::new(egui::RichText::new(name).font(font).color(if *visible { t.text } else { t.text_faint }))
                    .truncate()
                    .sense(Sense::click());
                let resp = ui.add_sized(vec2((ui.available_width() - 64.0).max(40.0), 26.0), label);
                if resp.on_hover_text(tl!("Make this the current layer")).clicked() {
                    set_current = Some(Some(*key));
                }
            }
        }
        if ours {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(editable, |ui| {
                    if icons::button(
                        ui,
                        if locked { "lock" } else { "lock-open" },
                        26.0,
                        locked,
                        if locked { tl!("Unlock layer") } else { tl!("Lock layer") },
                    )
                    .clicked()
                    {
                        edit = Some(Edit::SetLayerLocked { layer: *key, locked: !locked });
                    }
                    if icons::button(ui, "pencil", 26.0, false, tl!("Rename layer")).clicked() {
                        panel.renaming = Some((*key, name.clone()));
                    }
                });
            });
        }
    }
    // The selected markup's layer.
    if let (Some((page, ai)), Some(on)) = (selected, selected_layer) {
        ui.add_space(10.0);
        ui.separator();
        ui.label(egui::RichText::new(tl!("Selected markup")).font(theme::semibold(12.0)).color(t.text_muted));
        let mut choice = on;
        let name_of = |c: Option<(u32, u16)>| {
            c.and_then(|c| details.iter().find(|l| (l.id.num, l.id.generation) == c)).map_or_else(|| tl!("No layer").to_string(), |l| l.name.clone())
        };
        ui.add_enabled_ui(editable, |ui| {
            egui::ComboBox::from_id_salt("markup-layer").selected_text(name_of(on)).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut choice, None, tl!("No layer"));
                for l in &details {
                    ui.selectable_value(&mut choice, Some((l.id.num, l.id.generation)), l.name.as_str());
                }
            });
        });
        if choice != on {
            edit = Some(Edit::SetAnnotationLayer { page, index: ai, layer: choice });
        }
    }
    if let Some(c) = set_current {
        let _ = app.session.set_current_layer(id, c);
    }
    if let Some((layer, visible)) = toggle
        && app.session.set_layer_visible(id, layer, visible)
    {
        app.views[index].invalidate_content();
    }
    if let Some(e) = edit {
        let made = matches!(e, Edit::AddLayer { .. });
        if app.apply_edit(e) && made {
            // A new layer becomes the current one, as in CAD programs.
            let last = app.session.layer_details(id).last().map(|l| (l.id.num, l.id.generation));
            let _ = app.session.set_current_layer(id, last);
        }
    }
}

/// Save the symbol counts as a spreadsheet.
fn export_counts(app: &mut PdfCraftApp) {
    let Some((_, id)) = app.active_ids() else { return };
    if app.session.placed_symbols(id).is_empty() {
        app.notify(tl!("There are no symbols to count yet."));
        return;
    }
    let csv = app.session.symbol_counts_csv(id);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let write =
            move |app: &mut PdfCraftApp, path: std::path::PathBuf| match crate::editing::write_atomically(&path.to_string_lossy(), csv.as_bytes()) {
                Ok(()) => app.notify(tl!("Saved symbol counts.")),
                Err(e) => app.notify(e.to_string()),
            };
        match app.export_dir_override.as_ref().map(|d| std::path::PathBuf::from(d).join("symbol-counts.csv")) {
            Some(path) => write(app, path),
            None => {
                let dialog = crate::dialog::AsyncFileDialog::new().set_file_name("symbol-counts.csv").add_filter("CSV", &["csv"]);
                app.ask_one(crate::pickers::Ask::Save(dialog), None, write);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Err(e) = crate::editing::download("symbol-counts.csv", csv.as_bytes()) {
        app.notify(&e);
    }
}

impl PdfCraftApp {
    /// Pin a photo at `at` on `page`: choose one (or, on Android, take one with the camera).
    pub(crate) fn pick_photo(&mut self, page: usize, at: [f64; 2]) {
        if let Some((_, data)) = self.attach_override.take() {
            self.pin_photo(page, at, data);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let target = self.active_ids().map(|(_, id)| id);
            let dialog =
                crate::dialog::AsyncFileDialog::new().set_title(tl!("Pin a photo")).add_filter(tl!("Photos"), &["jpg", "jpeg", "png", "tif", "tiff"]);
            self.ask_one(crate::pickers::Ask::File(dialog), target, move |app, path| match std::fs::read(&path) {
                Ok(data) => app.pin_photo(page, at, data),
                Err(e) => app.notify_fmt("Couldn't read {name}: {e}", &[("name", &path.display().to_string()), ("e", &e.to_string())]),
            });
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (page, at);
    }

    /// Scale the photo and pin it, then show it so a caption can be added.
    fn pin_photo(&mut self, page: usize, at: [f64; 2], data: Vec<u8>) {
        let Some((index, id)) = self.active_ids() else { return };
        let jpeg = match pdfcraft_engine::photos::prepare(&data) {
            Ok(j) => j,
            Err(e) => {
                self.notify(e);
                return;
            }
        };
        let n = self.session.get(id).map_or(0, |d| d.info.annotations.iter().filter(|a| a.photo).count()) + 1;
        let edit = Edit::AddPhoto {
            page,
            at,
            name: pdfcraft_engine::photos::file_name(n),
            data: jpeg,
            caption: String::new(),
            author: self.comment_prefs.author.clone(),
        };
        self.views[index].comments.tool_done = true;
        if self.apply_edit(edit) {
            // The new pin is the last photo on its page.
            let new = self.session.get(id).and_then(|d| d.info.annotations.iter().filter(|a| a.photo && a.page == page).map(|a| a.index).max());
            if let Some(i) = new {
                self.open_photo(page, i);
            }
        }
    }

    /// Show the photo pinned at (page, index) of the active document.
    pub(crate) fn open_photo(&mut self, page: usize, index: usize) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(ctx) = self.ctx.clone() else { return };
        let Some(d) = self.session.get(id) else { return };
        let caption = d.info.annotations.iter().find(|a| a.page == page && a.index == index).and_then(|a| a.contents.clone()).unwrap_or_default();
        let source = pdfcraft_render::AttachmentSource::Annotation { page, index };
        let texture = d
            .info
            .attachments
            .iter()
            .find(|a| a.source == source)
            .ok_or_else(|| tl!("The photo couldn't be found.").to_string())
            .and_then(|att| pdfcraft_render::attachment_data(&d.bytes, d.password.as_deref(), att))
            .and_then(|bytes| {
                let side = ctx.input(|i| i.max_texture_side).min(4096) as u32;
                pdfcraft_engine::photos::decode_rgba(&bytes, side)
            })
            .map(|(w, h, px)| {
                let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                ctx.load_texture(format!("photo-{page}-{index}"), image, egui::TextureOptions::LINEAR)
            });
        self.photo_viewer = Some(PhotoViewer { doc: id, page, index, caption, texture });
    }
}

/// The photo viewer, when a photo is open.
pub fn photo_window(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(viewer) = app.photo_viewer.as_mut() else { return };
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let width = (screen.width() * 0.9).clamp(280.0, 1400.0);
    let body = (screen.height() * 0.9 - 140.0).max(160.0);
    let editable = app.session.get(viewer.doc).is_some_and(|d| d.allows_annotation() && d.read_only_reason.is_none());
    let mut close = false;
    let mut save_caption = false;
    let modal = egui::Modal::new(egui::Id::new("photo-viewer")).show(ctx, |ui| {
        ui.set_width(width);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tl!("Photo")).font(theme::semibold(15.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, "x", 30.0, false, tl!("Close")).clicked() {
                    close = true;
                }
            });
        });
        ui.add_space(6.0);
        match &viewer.texture {
            Ok(tex) => {
                let size = tex.size_vec2();
                let k = (width / size.x).min(body / size.y).min(1.0);
                let (rect, _) = ui.allocate_exact_size(vec2(width, size.y * k), Sense::hover());
                let shown = Rect::from_center_size(rect.center(), size * k);
                ui.painter().image(tex.id(), shown, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(tl!("Caption"));
            let field = ui.add_enabled(
                editable,
                egui::TextEdit::singleline(&mut viewer.caption).desired_width(width - 160.0).hint_text(tl!("Rack A, ceiling above room 104…")),
            );
            if field.lost_focus() {
                save_caption = true;
            }
            if ui.add_enabled(editable, egui::Button::new(tl!("Save"))).clicked() {
                save_caption = true;
                close = true;
            }
        });
        ui.label(egui::RichText::new(tl!("Tap the pin on the drawing to see this photo again.")).color(t.text_faint).small());
    });
    if modal.should_close() {
        close = true;
        save_caption = true;
    }
    if save_caption {
        let (doc, page, index, text) = (viewer.doc, viewer.page, viewer.index, viewer.caption.trim().to_string());
        let old = app
            .session
            .get(doc)
            .and_then(|d| d.info.annotations.iter().find(|a| a.page == page && a.index == index))
            .and_then(|a| a.contents.clone())
            .unwrap_or_default();
        if editable && old != text && app.active_ids().is_some_and(|(_, id)| id == doc) {
            app.apply_edit(Edit::SetAnnotationContents { page, index, text });
        }
    }
    if close {
        app.photo_viewer = None;
    }
}
