//! RevuCraft's field tools: the Symbols ribbon tab (pick a symbol, its layer, size and label,
//! then tap to stamp it again and again), the layer manager in the Layers panel, symbol counts,
//! and photo callouts (pin a picture to the plan; tapping the pin shows it).

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, pos2, vec2};
use pdfcraft_engine::symbols::{CustomArt, NewSymbol, Symbol};
use pdfcraft_engine::{DocId, Edit};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, icons};

/// Your own symbols kept at most.
pub const MAX_MY_SYMBOLS: usize = 64;

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
    /// New symbols turn to line up with the nearest wall or line.
    pub align_to_wall: bool,
    pub next: u32,
    /// Renumber: the symbol kind, the label before the number, the first number, every page.
    pub renumber_kind: Option<String>,
    pub renumber_prefix: String,
    pub renumber_start: u32,
    pub renumber_all_pages: bool,
    /// Photo callouts: the side the description goes, the photo's height (inches), and the
    /// spot chosen while placing one (page, user space).
    pub callout_side: pdfcraft_engine::photo_callout::TextSide,
    pub callout_inches: f64,
    pub callout_spot: Option<(usize, [f64; 2])>,
    /// One of your own symbols, chosen instead of `symbol` (shared: the settings are copied
    /// every frame and artwork can be large).
    pub custom: Option<std::sync::Arc<CustomArt>>,
}

impl Default for SymbolSettings {
    fn default() -> Self {
        Self {
            symbol: Symbol::DataJack,
            real_size: true,
            real_inches: 12.0,
            paper_inches: 0.25,
            label: String::new(),
            auto_number: false,
            align_to_wall: false,
            next: 1,
            renumber_kind: None,
            renumber_prefix: String::new(),
            renumber_start: 1,
            renumber_all_pages: false,
            callout_side: pdfcraft_engine::photo_callout::TextSide::Right,
            callout_inches: 1.5,
            callout_spot: None,
            custom: None,
        }
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
    /// A photo callout (the caption is its description, several lines) rather than a pin.
    pub callout: bool,
}

pub(crate) fn color32(c: [f64; 3]) -> Color32 {
    let v = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(v(c[0]), v(c[1]), v(c[2]))
}

/// The colour of the document's current layer (blue without one).
pub(crate) fn doc_layer_color(doc: &pdfcraft_engine::Document) -> [f64; 3] {
    let current = doc.current_layer;
    current.and_then(|c| doc.layer_list().into_iter().find(|l| (l.id.num, l.id.generation) == c)).map_or(NO_LAYER_COLOR, |l| l.color)
}

/// The size of a photo callout's box on the sheet (points), for the placement preview: a 4:3
/// photo at the chosen height beside the description.
pub(crate) fn callout_box_size(s: &SymbolSettings) -> (f64, f64) {
    use pdfcraft_engine::photo_callout::{GAP, PAD, TEXT_W};
    let h = callout_height(s);
    (h * 4.0 / 3.0 + GAP + TEXT_W + 2.0 * PAD, h + 2.0 * PAD)
}

/// The photo's height on the sheet in points.
pub(crate) fn callout_height(s: &SymbolSettings) -> f64 {
    let inches = if s.callout_inches.is_finite() { s.callout_inches.clamp(0.5, 8.0) } else { 1.5 };
    inches * 72.0
}

fn layer_color(app: &PdfCraftApp, id: DocId) -> [f64; 3] {
    let current = app.session.get(id).and_then(|d| d.current_layer);
    current.and_then(|c| app.session.layer_details(id).into_iter().find(|l| (l.id.num, l.id.generation) == c)).map_or(NO_LAYER_COLOR, |l| l.color)
}

/// Draw a library symbol's artwork in `rect` (a preview of what is stamped), from its content
/// operators: paths (`m l c re h`), strokes and fills, and its tags.
pub fn paint_symbol(painter: &egui::Painter, rect: Rect, symbol: Symbol, color: Color32) {
    paint_art(painter, rect, &symbol.artwork(), color);
}

/// The symbol chosen on the Symbols tab, drawn small: your own, or the library's.
fn paint_chosen(painter: &egui::Painter, rect: Rect, s: &SymbolSettings, color: Color32) {
    match &s.custom {
        Some(c) => paint_art(painter, rect, &c.art, color),
        None => paint_symbol(painter, rect, s.symbol, color),
    }
}

/// Symbol artwork (PDF path operators in the 100-unit square) drawn into `rect` on screen.
pub fn paint_art(painter: &egui::Painter, rect: Rect, art: &str, color: Color32) {
    let k = rect.width().min(rect.height()) / 100.0;
    let origin = pos2(rect.center().x - 50.0 * k, rect.center().y + 50.0 * k);
    let at = |x: f32, y: f32| pos2(origin.x + x * k, origin.y - y * k);
    let mut stroke = Stroke::new((6.0 * k).max(1.0), color);
    let mut nums: Vec<f32> = Vec::new();
    let mut paths: Vec<(Vec<Pos2>, bool)> = Vec::new();
    let mut current: Vec<Pos2> = Vec::new();
    let mut font = 20.0f32;
    let mut text_at = pos2(0.0, 0.0);
    let mut rest = art;
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
            "w" => {
                if let Some(v) = take(&mut nums, 1) {
                    stroke.width = (v[0] * k).max(1.0);
                }
            }
            "S" | "f" | "f*" | "B" | "B*" => {
                let fill = matches!(token, "f" | "f*");
                if current.len() > 1 {
                    paths.push((std::mem::take(&mut current), fill || token.starts_with('B')));
                }
                for (points, closed) in paths.drain(..) {
                    if fill {
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
    let mut add_svg = false;
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
                        paint_chosen(ui.painter(), rect.shrink(5.0), &app.symbols, color);
                        let shown = match &app.symbols.custom {
                            Some(c) => c.name.clone(),
                            None => crate::i18n::t(current.name()).to_string(),
                        };
                        let name = egui::RichText::new(shown).font(theme::regular(12.0));
                        let _ = ui.menu_button(name, |ui| {
                            if library(ui, &t, color, &mut app.symbols, &mut app.my_symbols, &mut app.quick_tool) {
                                add_svg = true;
                            }
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
                let _ = ui
                    .checkbox(&mut s.align_to_wall, tl!("Line up with walls"))
                    .on_hover_text(tl!("Turn new symbols to line up with the nearest wall or line"));
            });
            separator(ui);
            group(ui, &t, tl!("Photos"), |ui| {
                let pinning = app.quick_tool == QuickTool::PhotoPin;
                ui.horizontal(|ui| {
                    if icons::button(ui, "camera", 34.0, pinning, tl!("Pin a photo: tap the drawing where it goes")).clicked() {
                        app.quick_tool = if pinning { QuickTool::Select } else { QuickTool::PhotoPin };
                    }
                    let calling = app.quick_tool == QuickTool::PhotoCallout;
                    if icons::button(ui, "message-square-text", 34.0, calling, tl!("Photo callout: tap the spot, then where the box goes"))
                        .clicked()
                    {
                        app.quick_tool = if calling { QuickTool::Select } else { QuickTool::PhotoCallout };
                        app.symbols.callout_spot = None;
                    }
                    use pdfcraft_engine::photo_callout::TextSide;
                    let s = &mut app.symbols;
                    ui.selectable_value(&mut s.callout_side, TextSide::Left, tl!("Text left"));
                    ui.selectable_value(&mut s.callout_side, TextSide::Right, tl!("Text right"));
                    ui.add(egui::DragValue::new(&mut s.callout_inches).range(0.5..=8.0).speed(0.05).max_decimals(2).suffix(" in"))
                        .on_hover_text(tl!("The photo's height on the sheet"));
                });
            });
            separator(ui);
            group(ui, &t, tl!("Renumber"), |ui| {
                let resp = icons::button(ui, "list-ordered", 34.0, false, tl!("Renumber symbols in order across the page"));
                egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    renumber_popup(app, ui, id);
                });
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
    if add_svg {
        app.add_symbol_from_svg();
    }
}

/// A ribbon group: its controls, with the caption underneath.
fn group(ui: &mut egui::Ui, t: &Tokens, caption: &str, body: impl FnOnce(&mut egui::Ui)) {
    // A vertical stack as wide as its controls (a zero-width box would squeeze text fields).
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        body(ui);
        ui.add(egui::Label::new(egui::RichText::new(caption).font(theme::regular(11.0)).color(t.text_faint)).wrap_mode(egui::TextWrapMode::Extend));
    });
}

/// The symbol library: a grid of previews.
/// Returns whether "Add symbol from SVG" was chosen.
fn library(ui: &mut egui::Ui, t: &Tokens, color: Color32, s: &mut SymbolSettings, mine: &mut Vec<CustomArt>, tool: &mut QuickTool) -> bool {
    let current = if s.custom.is_some() { None } else { Some(s.symbol) };
    let mut chosen: Option<Symbol> = None;
    let mut add = false;
    ui.label(egui::RichText::new(tl!("Networking")).font(theme::semibold(12.0)).color(t.text_muted));
    egui::Grid::new("symbol-library").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        for (i, symbol) in Symbol::ALL.into_iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(64.0, 60.0), Sense::click());
            let fill = if Some(symbol) == current {
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
                chosen = Some(symbol);
            }
            if i % 5 == 4 {
                ui.end_row();
            }
        }
    });
    // Your own symbols, from SVG files.
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tl!("My symbols")).font(theme::semibold(12.0)).color(t.text_muted));
    let mut pick_mine: Option<usize> = None;
    let mut remove: Option<usize> = None;
    if !mine.is_empty() {
        egui::Grid::new("symbol-library-mine").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for (i, art) in mine.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(64.0, 60.0), Sense::click());
                let selected = s.custom.as_ref().is_some_and(|c| c.name == art.name);
                let fill = if selected {
                    t.accent_soft
                } else if resp.hovered() {
                    t.hover
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                paint_art(ui.painter(), Rect::from_center_size(rect.center() - vec2(0.0, 7.0), vec2(36.0, 36.0)), &art.art, color);
                let short: String = art.name.chars().take(11).collect();
                ui.painter().text(rect.center_bottom() - vec2(0.0, 3.0), Align2::CENTER_BOTTOM, short, theme::regular(10.0), t.text_muted);
                let resp = resp.on_hover_text(art.name.as_str());
                resp.context_menu(|ui| {
                    if ui.button(tl!("Remove from my symbols")).clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
                if resp.clicked() {
                    pick_mine = Some(i);
                }
                if i % 5 == 4 {
                    ui.end_row();
                }
            }
        });
    }
    if ui.button(tl!("Add symbol from SVG…")).on_hover_text(tl!("Use a line drawing saved as SVG as a symbol of your own")).clicked() {
        add = true;
        ui.close();
    }
    if let Some(symbol) = chosen {
        s.symbol = symbol;
        s.custom = None;
        *tool = QuickTool::Symbol;
        ui.close();
    }
    if let Some(art) = pick_mine.and_then(|i| mine.get(i)) {
        s.custom = Some(std::sync::Arc::new(art.clone()));
        *tool = QuickTool::Symbol;
        ui.close();
    }
    if let Some(i) = remove
        && i < mine.len()
    {
        let gone = mine.remove(i);
        if s.custom.as_ref().is_some_and(|c| c.name == gone.name) {
            s.custom = None;
        }
    }
    add
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
    let color = doc_layer_color(doc);
    Edit::AddSymbol(NewSymbol {
        page,
        symbol: s.symbol,
        center: at,
        size: size.clamp(1.0, 20_000.0),
        label: s.label_now(),
        color,
        author: author.to_string(),
        // Lined up with a wall or line within a couple of symbol widths.
        rotation: if s.align_to_wall {
            crate::props_box::nearest_line(doc, page, at).filter(|(d, _)| *d <= size.max(12.0) * 2.0).map_or(0.0, |(_, a)| a)
        } else {
            0.0
        },
        custom: s.custom.as_deref().cloned(),
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
            // The new pin is the last photo on its page; its caption is typed beside it.
            let new = self.session.get(id).and_then(|d| d.info.annotations.iter().filter(|a| a.photo && a.page == page).map(|a| a.index).max());
            if let Some(i) = new {
                self.views[index].caption_prompt = Some(CaptionPrompt { page, index: i, text: String::new(), kind: CaptionKind::Photo });
            }
        }
    }

    /// Place a photo callout pointing at `point` with its box at `center`: choose a photo (or,
    /// on Android, take one), then type the description.
    pub(crate) fn pick_callout_photo(&mut self, page: usize, point: [f64; 2], center: [f64; 2]) {
        if let Some((_, data)) = self.attach_override.take() {
            self.place_callout(page, point, center, data);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let target = self.active_ids().map(|(_, id)| id);
            let dialog = crate::dialog::AsyncFileDialog::new()
                .set_title(tl!("Photo for the callout"))
                .add_filter(tl!("Photos"), &["jpg", "jpeg", "png", "tif", "tiff"]);
            self.ask_one(crate::pickers::Ask::File(dialog), target, move |app, path| match std::fs::read(&path) {
                Ok(data) => app.place_callout(page, point, center, data),
                Err(e) => app.notify_fmt("Couldn't read {name}: {e}", &[("name", &path.display().to_string()), ("e", &e.to_string())]),
            });
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (page, point, center);
    }

    fn place_callout(&mut self, page: usize, point: [f64; 2], center: [f64; 2], data: Vec<u8>) {
        let Some((index, id)) = self.active_ids() else { return };
        let before = self.session.get(id).map_or(0, |d| d.info.annotations.iter().filter(|a| a.page == page).count());
        let edit = Edit::AddPhotoCallout {
            page,
            point,
            center,
            data,
            text: String::new(),
            side: self.symbols.callout_side,
            photo_height: callout_height(&self.symbols),
            author: self.comment_prefs.author.clone(),
        };
        self.views[index].comments.tool_done = true;
        if self.apply_edit(edit) {
            // The new callout is the last annotation on its page.
            let new = self.session.get(id).and_then(|d| {
                d.info
                    .annotations
                    .iter()
                    .filter(|a| a.page == page)
                    .map(|a| a.index)
                    .max()
                    .filter(|_| d.info.annotations.iter().filter(|a| a.page == page).count() > before)
            });
            if let Some(i) = new {
                self.views[index].caption_prompt = Some(CaptionPrompt { page, index: i, text: String::new(), kind: CaptionKind::Callout });
            }
        }
    }

    /// Symbols tab ▸ Add symbol from SVG: choose an SVG file; it joins your symbols, named
    /// after the file, and is chosen to place.
    pub(crate) fn add_symbol_from_svg(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let target = self.active_ids().map(|(_, id)| id);
            let dialog = crate::dialog::AsyncFileDialog::new().set_title(tl!("Add symbol from SVG…")).add_filter(tl!("SVG drawings"), &["svg"]);
            self.ask_one(crate::pickers::Ask::File(dialog), target, move |app, path| {
                let name = path.file_stem().map(|s| s.to_string_lossy().replace(['_', '-'], " ")).unwrap_or_default();
                match std::fs::read(&path) {
                    Ok(data) => app.add_my_symbol(&name, &data),
                    Err(e) => app.notify_fmt("Couldn't read {name}: {e}", &[("name", &path.display().to_string()), ("e", &e.to_string())]),
                }
            });
        }
    }

    /// Add an SVG drawing to your symbols as `name` (replacing one of the same name) and choose it.
    pub(crate) fn add_my_symbol(&mut self, name: &str, svg: &[u8]) {
        let name = pdfcraft_engine::symbols::clean_label(name);
        let name = if name.is_empty() { tl!("My symbol").to_string() } else { name };
        match pdfcraft_engine::svg_art::svg_art(svg) {
            Ok(art) => {
                let new = CustomArt { name: name.clone(), art };
                if let Some(slot) = self.my_symbols.iter_mut().find(|s| s.name == name) {
                    *slot = new.clone();
                } else if self.my_symbols.len() < MAX_MY_SYMBOLS {
                    self.my_symbols.push(new.clone());
                } else {
                    self.notify(tl!("You have as many symbols of your own as can be kept; remove one first.").to_string());
                    return;
                }
                self.symbols.custom = Some(std::sync::Arc::new(new));
                self.quick_tool = QuickTool::Symbol;
                self.notify_fmt("Added {name} to your symbols.", &[("name", &name)]);
            }
            Err(e) => self.notify(e.to_string()),
        }
    }

    /// Show the photo pinned at (page, index) of the active document.
    pub(crate) fn open_photo(&mut self, page: usize, index: usize) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(ctx) = self.ctx.clone() else { return };
        let Some(d) = self.session.get(id) else { return };
        let caption = d.info.annotations.iter().find(|a| a.page == page && a.index == index).and_then(|a| a.contents.clone()).unwrap_or_default();
        let source = pdfcraft_render::AttachmentSource::Annotation { page, index };
        let callout = d.callout_side(page, index).is_some();
        let bytes = match d.callout_photo(page, index) {
            Some(jpeg) => Ok(jpeg),
            None => d
                .info
                .attachments
                .iter()
                .find(|a| a.source == source)
                .ok_or_else(|| tl!("The photo couldn't be found.").to_string())
                .and_then(|att| pdfcraft_render::attachment_data(&d.bytes, d.password.as_deref(), att)),
        };
        let texture = bytes
            .and_then(|bytes| {
                let side = ctx.input(|i| i.max_texture_side).min(4096) as u32;
                pdfcraft_engine::photos::decode_rgba(&bytes, side)
            })
            .map(|(w, h, px)| {
                let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                ctx.load_texture(format!("photo-{page}-{index}"), image, egui::TextureOptions::LINEAR)
            });
        self.photo_viewer = Some(PhotoViewer { doc: id, page, index, caption, texture, callout });
    }
}

/// What a caption box is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionKind {
    /// A photo pin's caption.
    Photo,
    /// A photo callout's description (several lines; the first is the title).
    Callout,
    /// A symbol's label.
    Symbol,
}

/// The value box beside something just placed, for its caption, description or label.
#[derive(Clone, Debug)]
pub struct CaptionPrompt {
    pub page: usize,
    pub index: usize,
    pub text: String,
    pub kind: CaptionKind,
}

/// Queue `edit` with whatever the page already has pending this frame (a label typed, then the
/// next symbol placed with the same click), as one step.
fn queue_edit(view: &mut crate::canvas::DocView, edit: Edit) {
    view.pending_edit = Some(match view.pending_edit.take() {
        Some(prev) => Edit::Batch { label: prev.label(), edits: vec![edit, prev] },
        None => edit,
    });
}

/// The caption box on `page`: opens beside a symbol once it is placed, and stays beside a photo,
/// callout or symbol until Enter, Escape or a click elsewhere (which keeps what was typed).
pub(crate) fn caption_box(
    ui: &egui::Ui,
    info: &pdfcraft_render::DocInfo,
    view: &mut crate::canvas::DocView,
    page: usize,
    to_screen: impl Fn([f64; 2]) -> egui::Pos2,
) {
    // A symbol just placed: once it is in the document, its label box opens.
    if let Some((p, before)) = view.label_after
        && p == page
    {
        let symbols: Vec<_> = info.annotations.iter().filter(|a| a.page == page && a.symbol.is_some()).collect();
        if symbols.len() > before
            && let Some(newest) = symbols.iter().max_by_key(|a| a.index)
        {
            view.caption_prompt =
                Some(CaptionPrompt { page, index: newest.index, text: newest.contents.clone().unwrap_or_default(), kind: CaptionKind::Symbol });
            view.label_after = None;
        }
    }
    let Some(prompt) = view.caption_prompt.as_mut().filter(|c| c.page == page) else { return };
    let Some(a) = info.annotations.iter().find(|a| a.page == page && a.index == prompt.index) else {
        view.caption_prompt = None;
        return;
    };
    let [x0, y0, x1, y1] = a.rect.map(f64::from);
    let anchor = to_screen([x0.max(x1), y0.min(y1)]);
    let (title, hint) = match prompt.kind {
        CaptionKind::Photo => (tl!("Caption"), tl!("Rack A, ceiling above room 104…")),
        CaptionKind::Callout => (tl!("Description (the first line is the title)"), tl!("Rack A\n24-port switch and patch panel 1…")),
        CaptionKind::Symbol => (tl!("Label"), tl!("PC, VoIP…")),
    };
    let look = crate::value_prompt::Prompt { multiline: prompt.kind == CaptionKind::Callout, ..crate::value_prompt::Prompt::new(title, hint) };
    let action = crate::value_prompt::show(ui.ctx(), ("caption", page), anchor, ui.clip_rect(), look, &mut prompt.text, |_| {});
    let (kind, index, text) = (prompt.kind, prompt.index, prompt.text.trim().to_string());
    let old = a.contents.clone().unwrap_or_default();
    match action {
        Some(crate::value_prompt::PromptAction::Apply | crate::value_prompt::PromptAction::Leave) => {
            view.caption_prompt = None;
            if text != old.trim() {
                let edit = match kind {
                    CaptionKind::Symbol => Edit::EditSymbol { page, index, label: Some(text), rotation: None },
                    CaptionKind::Photo | CaptionKind::Callout => Edit::SetAnnotationContents { page, index, text },
                };
                queue_edit(view, edit);
            }
        }
        Some(crate::value_prompt::PromptAction::Cancel) => view.caption_prompt = None,
        None => {}
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
        if viewer.callout {
            ui.label(tl!("Description (the first line is the title)"));
            let field = ui.add_enabled(
                editable,
                egui::TextEdit::multiline(&mut viewer.caption)
                    .desired_width(width - 16.0)
                    .desired_rows(3)
                    .hint_text(tl!("Rack A\n24-port switch and patch panel 1…")),
            );
            if field.lost_focus() {
                save_caption = true;
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(editable, egui::Button::new(tl!("Save"))).clicked() {
                    save_caption = true;
                    close = true;
                }
            });
        } else {
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
        }
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

/// The Renumber pop-up: label every symbol of one kind in reading order (top to bottom, then left
/// to right) as prefix + number, counting up from the start.
fn renumber_popup(app: &mut PdfCraftApp, ui: &mut egui::Ui, id: pdfcraft_engine::DocId) {
    let placed = app.session.placed_symbols(id);
    let mut kinds: Vec<String> = placed.iter().map(|p| p.symbol.clone()).collect();
    kinds.sort();
    kinds.dedup();
    if kinds.is_empty() {
        ui.label(tl!("There are no symbols to renumber yet."));
        return;
    }
    let current_page = app.active_ids().and_then(|(i, _)| app.views.get(i)).map_or(0, |v| v.current);
    let s = &mut app.symbols;
    if s.renumber_kind.as_ref().is_none_or(|k| !kinds.contains(k)) {
        s.renumber_kind = kinds.first().cloned();
    }
    egui::Grid::new("renumber-grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(tl!("Symbol"));
        let shown = s.renumber_kind.as_deref().map(pdfcraft_engine::symbols::display_name).unwrap_or_default();
        egui::ComboBox::from_id_salt("renumber-kind").selected_text(shown).width(160.0).show_ui(ui, |ui| {
            for k in &kinds {
                ui.selectable_value(&mut s.renumber_kind, Some(k.clone()), pdfcraft_engine::symbols::display_name(k));
            }
        });
        ui.end_row();
        ui.label(tl!("Label"));
        ui.add(egui::TextEdit::singleline(&mut s.renumber_prefix).desired_width(100.0).hint_text(tl!("PC, VoIP…")));
        ui.end_row();
        ui.label(tl!("Start at"));
        ui.add(egui::DragValue::new(&mut s.renumber_start).range(0..=99_999));
        ui.end_row();
    });
    ui.checkbox(&mut s.renumber_all_pages, tl!("Every page (otherwise this page)"));
    let preview = {
        let prefix = s.renumber_prefix.trim();
        if prefix.is_empty() { s.renumber_start.to_string() } else { format!("{prefix}-{}", s.renumber_start) }
    };
    ui.label(egui::RichText::new(crate::i18n::fmt(tl!("The first one becomes {label}"), &[("label", &preview)])).small());
    if !crate::widgets::pill_button(ui, tl!("Renumber"), true).clicked() {
        return;
    }
    let Some(kind) = s.renumber_kind.clone() else { return };
    let (prefix, start, all) = (s.renumber_prefix.trim().to_string(), s.renumber_start, s.renumber_all_pages);
    let Some(doc) = app.session.get(id) else { return };
    // Reading order: page, then rows from the top, then left to right.
    let mut targets: Vec<(usize, usize, f32, f32)> = placed
        .iter()
        .filter(|p| p.symbol == kind && (all || p.page == current_page))
        .filter_map(|p| {
            doc.info
                .annotations
                .iter()
                .find(|a| a.page == p.page && a.index == p.index)
                .map(|a| (p.page, p.index, a.rect[1].max(a.rect[3]), a.rect[0].min(a.rect[2])))
        })
        .collect();
    targets.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))
    });
    let edits: Vec<Edit> = targets
        .iter()
        .enumerate()
        .map(|(k, (page, index, _, _))| {
            let n = start.saturating_add(u32::try_from(k).unwrap_or(u32::MAX));
            let label = if prefix.is_empty() { n.to_string() } else { format!("{prefix}-{n}") };
            Edit::EditSymbol { page: *page, index: *index, label: Some(label), rotation: None }
        })
        .collect();
    if edits.is_empty() {
        app.notify(tl!("There are no symbols of that kind on this page."));
        return;
    }
    let count = edits.len();
    if app.apply_edit(Edit::Batch { label: "Renumber symbols".into(), edits }) {
        app.notify(crate::i18n::fmt(tl!("Renumbered {n} symbols."), &[("n", &count.to_string())]));
    }
}
