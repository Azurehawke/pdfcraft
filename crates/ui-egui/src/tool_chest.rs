//! RevuCraft's Tool chest and workspaces.
//!
//! The Tool chest (a dock panel) keeps tools with their settings: a highlighter in a chosen
//! colour, a wall-width line, a labelled data jack. "Save current tool" adds the tool in use,
//! with its settings as they are now; clicking a saved tool picks it up with those settings
//! again. The tools used most recently are listed underneath.
//!
//! Workspaces are named dock layouts (which boxes are open, their order, collapsed and floating,
//! and whether the Draft panel is collapsed), switched from View ▸ Workspaces or the rail.
//!
//! Both are saved in the settings file, which is untrusted: entries are capped and anything
//! unreadable is skipped.

use egui::RichText;
use pdfcraft_engine::Style;
use serde_json::{Value, json};

use crate::draft::LineWidth;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, icons, widgets};

/// At most this many saved tools, recent tools and workspaces are kept (and read back).
const MAX_SAVED: usize = 64;
const MAX_RECENT: usize = 10;
const MAX_WORKSPACES: usize = 24;
/// Names are cut to this many characters.
const MAX_NAME: usize = 60;

/// A symbol's settings as saved with it.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolSnapshot {
    pub symbol: String,
    pub label: String,
    pub real_size: bool,
    pub real_inches: f64,
    pub paper_inches: f64,
}

/// A tool kept in the chest, with its settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ChestTool {
    pub name: String,
    /// The tool's id ([`QuickTool::id`]).
    pub tool: String,
    /// A comment tool's colour, opacity, width and fill.
    pub style: Option<Style>,
    /// A drafting tool's line width.
    pub line_width: Option<LineWidth>,
    pub symbol: Option<SymbolSnapshot>,
}

/// The chest's contents and the workspaces.
#[derive(Clone, Debug, Default)]
pub struct ToolChest {
    pub saved: Vec<ChestTool>,
    /// Tool ids, the most recent first.
    pub recent: Vec<String>,
    /// The tool in use last frame, to notice a new one.
    last: Option<QuickTool>,
    /// The name typed for the tool being saved, while the field is open.
    pub naming: Option<String>,
    /// Named dock layouts.
    pub workspaces: Vec<(String, Value)>,
    pub workspace_naming: Option<String>,
}

fn clean_name(s: &str) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect()
}

fn finite(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

fn rgb(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    let c = [finite(a.first()?)?, finite(a.get(1)?)?, finite(a.get(2)?)?];
    Some(c.map(|x| x.clamp(0.0, 1.0)))
}

impl ChestTool {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "tool": self.tool,
            "style": self.style.as_ref().map(|s| json!({ "color": s.color, "opacity": s.opacity, "width": s.width, "fill": s.fill })),
            "line_width": self.line_width.map(|w| match w {
                LineWidth::Paper(pt) => json!({ "paper": pt }),
                LineWidth::Real(inches) => json!({ "real": inches }),
                LineWidth::Hairline => json!({ "hairline": true }),
            }),
            "symbol": self.symbol.as_ref().map(|s| json!({
                "symbol": s.symbol, "label": s.label, "real_size": s.real_size, "real_inches": s.real_inches, "paper_inches": s.paper_inches,
            })),
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let name = clean_name(v["name"].as_str()?);
        let tool: String = v["tool"].as_str()?.chars().take(MAX_NAME).collect();
        if name.is_empty() || tool.is_empty() {
            return None;
        }
        let style = v["style"].is_object().then(|| {
            let s = &v["style"];
            Some(Style {
                color: rgb(&s["color"])?,
                opacity: finite(&s["opacity"])?.clamp(0.0, 1.0),
                width: finite(&s["width"])?.clamp(0.0, 72.0),
                fill: rgb(&s["fill"]),
            })
        });
        let line_width = match (finite(&v["line_width"]["paper"]), finite(&v["line_width"]["real"])) {
            _ if v["line_width"]["hairline"].as_bool() == Some(true) => Some(LineWidth::Hairline),
            (Some(pt), _) => Some(LineWidth::Paper(pt.clamp(0.1, 72.0))),
            (None, Some(inches)) => Some(LineWidth::Real(inches.clamp(0.01, 1200.0))),
            _ => None,
        };
        let symbol = v["symbol"].is_object().then(|| {
            let s = &v["symbol"];
            Some(SymbolSnapshot {
                symbol: s["symbol"].as_str()?.chars().take(MAX_NAME).collect(),
                label: s["label"].as_str().unwrap_or_default().chars().take(MAX_NAME).collect(),
                real_size: s["real_size"].as_bool().unwrap_or(true),
                real_inches: finite(&s["real_inches"]).unwrap_or(12.0).clamp(0.1, 1200.0),
                paper_inches: finite(&s["paper_inches"]).unwrap_or(0.25).clamp(0.01, 12.0),
            })
        });
        Some(ChestTool { name, tool, style: style.flatten(), line_width, symbol: symbol.flatten() })
    }
}

impl ToolChest {
    /// For the settings file.
    pub fn to_json(&self) -> Value {
        json!({
            "saved": self.saved.iter().map(ChestTool::to_json).collect::<Vec<_>>(),
            "recent": self.recent,
            "workspaces": self.workspaces.iter().map(|(n, l)| json!({ "name": n, "layout": l })).collect::<Vec<_>>(),
        })
    }

    /// Read back what [`Self::to_json`] wrote; anything unreadable is skipped.
    pub fn from_json(v: &Value) -> Self {
        let list = |key: &str| v[key].as_array().cloned().unwrap_or_default();
        let saved = list("saved").iter().take(MAX_SAVED).filter_map(ChestTool::from_json).collect();
        let recent = list("recent")
            .iter()
            .filter_map(|s| s.as_str())
            .filter(|s| !s.is_empty() && s.len() <= MAX_NAME)
            .take(MAX_RECENT)
            .map(str::to_string)
            .collect();
        let workspaces = list("workspaces")
            .iter()
            .take(MAX_WORKSPACES)
            .filter_map(|w| {
                let name = clean_name(w["name"].as_str()?);
                (!name.is_empty() && w["layout"].is_object()).then(|| (name, w["layout"].clone()))
            })
            .collect();
        ToolChest { saved, recent, workspaces, ..Default::default() }
    }

    /// Note the tool in use (called every frame); a newly picked tool goes to the top of the
    /// recent list. Select and Hand are not listed.
    pub fn note(&mut self, tool: QuickTool) {
        if self.last == Some(tool) {
            return;
        }
        self.last = Some(tool);
        if matches!(tool, QuickTool::Select | QuickTool::Hand) {
            return;
        }
        let id = tool.id();
        self.recent.retain(|r| *r != id);
        self.recent.insert(0, id);
        self.recent.truncate(MAX_RECENT);
    }
}

/// A tool's name as shown (translated).
pub fn tool_name(app: &PdfCraftApp, id: &str) -> String {
    let tl_str = |s: &'static str| tl!(s).to_string();
    if let Some(name) = id.strip_prefix("draft-").and_then(crate::draft::DraftTool::from_name).map(|t| t.label()) {
        // The label's usage note in brackets is left off in the list.
        let name = tl_str(name);
        return name.split(" (").next().unwrap_or(&name).to_string();
    }
    if let Some(m) = id.strip_prefix("measure-") {
        let label = match m {
            "distance" => "Distance",
            "perimeter" => "Perimeter",
            "area" => "Area",
            "rectangle" => "Rectangle area",
            "angle" => "Angle",
            "radius" => "Radius",
            "cable" => "Cable run",
            _ => "Measure",
        };
        return format!("{} · {}", tl_str("Measure"), tl_str(label));
    }
    match id {
        "symbol" => format!("{} · {}", tl_str("Symbol"), tl_str(app.symbols.symbol.name())),
        "photo-pin" => tl_str("Photo pin"),
        "photo-callout" => tl_str("Photo callout"),
        "crop" => tl_str("Crop"),
        "redact" => tl_str("Redact"),
        "add-text" => tl_str("Add text"),
        "edit-text" => tl_str("Edit text"),
        "link" => tl_str("Link"),
        "snapshot" => tl_str("Snapshot"),
        other => match crate::comments::CommentTool::from_command(&format!("comment.{other}")) {
            Some(t) => tl_str(t.label()),
            None => other.to_string(),
        },
    }
}

/// The icon a tool shows in the chest.
fn paint_icon(ui: &mut egui::Ui, id: &str, selected: bool, tip: &str) -> egui::Response {
    if let Some(t) = id.strip_prefix("draft-").and_then(crate::draft::DraftTool::from_name) {
        return crate::cad_icons::button(ui, t.icon(), 28.0, selected, tip);
    }
    let icon = match id {
        "symbol" => "pentagon",
        "photo-pin" => "camera",
        "photo-callout" => "message-square-text",
        m if m.starts_with("measure-") => "ruler",
        other => crate::comments::CommentTool::from_command(&format!("comment.{other}")).map_or("wrench", |t| t.icon()),
    };
    icons::button(ui, icon, 28.0, selected, tip)
}

impl PdfCraftApp {
    /// The tool in use with its settings as they are now.
    fn snapshot_tool(&self, name: String) -> Option<ChestTool> {
        let tool = self.quick_tool;
        if matches!(tool, QuickTool::Select | QuickTool::Hand) {
            return None;
        }
        let style = match tool {
            QuickTool::Comment(t) => Some(self.comment_prefs.style(t)),
            _ => None,
        };
        let line_width = matches!(tool, QuickTool::Draft(_)).then_some(self.draft.line_width);
        let symbol = (tool == QuickTool::Symbol).then(|| {
            let s = &self.symbols;
            SymbolSnapshot {
                symbol: s.symbol.id().to_string(),
                label: s.label.clone(),
                real_size: s.real_size,
                real_inches: s.real_inches,
                paper_inches: s.paper_inches,
            }
        });
        Some(ChestTool { name, tool: tool.id(), style, line_width, symbol })
    }

    /// Pick up a saved tool with its settings.
    pub(crate) fn use_chest_tool(&mut self, t: &ChestTool) {
        let Ok(tool) = self.quick_tool_from_id(&t.tool) else {
            self.notify(tl!("That tool isn't available in this version.").to_string());
            return;
        };
        if let (QuickTool::Comment(c), Some(style)) = (tool, t.style.clone()) {
            self.comment_prefs.set_style(c, style);
        }
        if let Some(w) = t.line_width {
            self.draft.line_width = w;
            self.draft.by_layer = false;
        }
        if let Some(s) = &t.symbol
            && let Some(symbol) = pdfcraft_engine::symbols::Symbol::from_id(&s.symbol)
        {
            self.symbols.symbol = symbol;
            self.symbols.label = s.label.clone();
            self.symbols.real_size = s.real_size;
            self.symbols.real_inches = s.real_inches;
            self.symbols.paper_inches = s.paper_inches;
        }
        self.quick_tool = tool;
    }

    /// The current dock layout as a workspace.
    fn workspace_now(&self) -> Value {
        json!({ "dock": self.dock_layout(), "draft_collapsed": self.draft.panel_collapsed })
    }

    pub(crate) fn switch_workspace(&mut self, layout: &Value) {
        if layout["dock"].is_object() {
            self.restore_dock_layout(&layout["dock"]);
        }
        if let Some(c) = layout["draft_collapsed"].as_bool() {
            self.draft.panel_collapsed = c;
        }
    }
}

/// The ready-made workspaces offered before any are saved: (name, panels open, Properties).
const PRESETS: [(&str, &[&str], bool); 3] =
    [("Markup", &["Markups", "Layers"], true), ("Drafting", &["ToolChest", "Layers"], true), ("Review", &["Comments", "Markups"], false)];

fn preset(panels: &[&str], properties: bool) -> Value {
    json!({ "dock": { "panels": panels, "order": [], "collapsed": [], "floating": [], "properties": properties } })
}

/// View ▸ Workspaces (and the rail's Workspaces button).
pub fn workspaces_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let mut switch: Option<Value> = None;
    let mut delete: Option<usize> = None;
    if app.chest.workspaces.is_empty() {
        for (name, panels, properties) in PRESETS {
            if ui.button(tl!(name)).clicked() {
                switch = Some(preset(panels, properties));
                ui.close();
            }
        }
    }
    for (i, (name, layout)) in app.chest.workspaces.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.button(name.as_str()).clicked() {
                switch = Some(layout.clone());
                ui.close();
            }
            if icons::button(ui, "trash-2", 22.0, false, tl!("Delete this workspace")).clicked() {
                delete = Some(i);
            }
        });
    }
    ui.separator();
    match app.chest.workspace_naming.as_mut() {
        None => {
            if ui.button(tl!("Save the current layout…")).clicked() {
                app.chest.workspace_naming = Some(String::new());
            }
        }
        Some(name) => {
            let mut save = false;
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(name).desired_width(140.0).hint_text(tl!("Workspace name")));
                save = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                save |= ui.button(tl!("Save")).clicked();
            });
            if save {
                let name = clean_name(name);
                app.chest.workspace_naming = None;
                if !name.is_empty() {
                    let layout = app.workspace_now();
                    let w = &mut app.chest.workspaces;
                    if let Some(slot) = w.iter_mut().find(|(n, _)| *n == name) {
                        slot.1 = layout;
                    } else if w.len() < MAX_WORKSPACES {
                        w.push((name, layout));
                    }
                }
            }
        }
    }
    if let Some(i) = delete
        && i < app.chest.workspaces.len()
    {
        app.chest.workspaces.remove(i);
    }
    if let Some(layout) = switch {
        app.switch_workspace(&layout);
    }
}

/// The Tool chest panel.
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let current = app.quick_tool.id();
    let mut pick: Option<ChestTool> = None;
    let mut pick_recent: Option<String> = None;
    let mut remove: Option<usize> = None;
    egui::ScrollArea::vertical().id_salt("tool-chest").auto_shrink([false, false]).show(ui, |ui| {
        ui.label(RichText::new(tl!("My tools")).font(theme::semibold(13.0)));
        if app.chest.saved.is_empty() {
            ui.label(
                RichText::new(tl!("Pick a tool and set it up, then save it here to use it again with the same settings."))
                    .color(t.text_muted)
                    .small(),
            );
        }
        for (i, tool) in app.chest.saved.iter().enumerate() {
            ui.horizontal(|ui| {
                let selected = tool.tool == current;
                if paint_icon(ui, &tool.tool, selected, &tool.name).clicked() {
                    pick = Some(tool.clone());
                }
                if let Some(s) = &tool.style {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 5.0, crate::revu::color32(s.color));
                }
                if ui.selectable_label(selected, tool.name.as_str()).clicked() {
                    pick = Some(tool.clone());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, "trash-2", 22.0, false, tl!("Remove from the tool chest")).clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        ui.add_space(4.0);
        let can_save = !matches!(app.quick_tool, QuickTool::Select | QuickTool::Hand);
        match app.chest.naming.as_mut() {
            None => {
                if ui.add_enabled_ui(can_save, |ui| widgets::pill_button(ui, tl!("Save current tool"), false)).inner.clicked() {
                    app.chest.naming = Some(tool_name(app, &current));
                }
                if !can_save {
                    ui.label(RichText::new(tl!("Choose a tool first.")).color(t.text_faint).small());
                }
            }
            Some(_) => {
                let mut save = false;
                let mut cancel = false;
                if let Some(name) = app.chest.naming.as_mut() {
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(name).desired_width(130.0));
                        save = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        save |= ui.button(tl!("Save")).clicked();
                        cancel = ui.button(tl!("Cancel")).clicked();
                    });
                }
                if save {
                    let name = clean_name(app.chest.naming.as_deref().unwrap_or_default());
                    if !name.is_empty()
                        && app.chest.saved.len() < MAX_SAVED
                        && let Some(tool) = app.snapshot_tool(name)
                    {
                        app.chest.saved.push(tool);
                    }
                    app.chest.naming = None;
                } else if cancel {
                    app.chest.naming = None;
                }
            }
        }
        ui.add_space(10.0);
        ui.label(RichText::new(tl!("Recent tools")).font(theme::semibold(13.0)));
        if app.chest.recent.is_empty() {
            ui.label(RichText::new(tl!("Tools you use show up here.")).color(t.text_muted).small());
        }
        let recent = app.chest.recent.clone();
        for id in &recent {
            let name = tool_name(app, id);
            ui.horizontal(|ui| {
                let selected = *id == current;
                if paint_icon(ui, id, selected, &name).clicked() || ui.selectable_label(selected, name.as_str()).clicked() {
                    pick_recent = Some(id.clone());
                }
            });
        }
    });
    if let Some(i) = remove
        && i < app.chest.saved.len()
    {
        app.chest.saved.remove(i);
    }
    if let Some(tool) = pick {
        app.use_chest_tool(&tool);
    }
    if let Some(id) = pick_recent
        && let Ok(tool) = app.quick_tool_from_id(&id)
    {
        app.quick_tool = tool;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chest_round_trips_and_skips_bad_entries() {
        let chest = ToolChest {
            saved: vec![
                ChestTool {
                    name: "Yellow highlight".into(),
                    tool: "highlight".into(),
                    style: Some(Style { color: [1.0, 0.9, 0.0], opacity: 0.5, width: 1.0, fill: None }),
                    line_width: None,
                    symbol: None,
                },
                ChestTool {
                    name: "Exterior wall".into(),
                    tool: "draft-polyline".into(),
                    style: None,
                    line_width: Some(LineWidth::Real(6.0)),
                    symbol: None,
                },
            ],
            recent: vec!["draft-line".into(), "measure-area".into()],
            workspaces: vec![("Mine".into(), json!({ "dock": { "panels": ["Layers"] } }))],
            ..Default::default()
        };
        let back = ToolChest::from_json(&chest.to_json());
        assert_eq!((back.saved, back.recent, back.workspaces), (chest.saved.clone(), chest.recent.clone(), chest.workspaces.clone()));
        let junk = json!({ "saved": [{ "name": "", "tool": "x" }, { "tool": 3 }, "nope"], "recent": [1, ""], "workspaces": [{ "name": "W" }] });
        let back = ToolChest::from_json(&junk);
        assert!(back.saved.is_empty() && back.recent.is_empty() && back.workspaces.is_empty());
    }

    #[test]
    fn recent_tools_put_the_newest_first_once() {
        let mut chest = ToolChest::default();
        for tool in [QuickTool::Select, QuickTool::Symbol, QuickTool::PhotoPin, QuickTool::Symbol, QuickTool::Symbol, QuickTool::Hand] {
            chest.note(tool);
        }
        assert_eq!(chest.recent, ["symbol", "photo-pin"]);
    }
}
