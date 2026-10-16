//! RevuCraft's Draft tools panel, on the left while the Draft tab is open: the drafting tools in
//! groups (Draw, Modify, Annotate), as in CADCraft's Tool Sets. It collapses to one column of
//! icons, one per group, showing the group's last-used tool; the small arrow on a button (or a
//! long press, or a right click) flies out the rest of the group.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::cad_icons::{self, CadIcon};
use crate::draft::DraftTool;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, QuickTool, icons};

/// One button: a drafting tool, or a commenting command used for drafting (text, callouts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Tool(DraftTool),
    Command { id: &'static str, icon: CadIcon, label: &'static str },
}

impl Entry {
    fn icon(self) -> CadIcon {
        match self {
            Entry::Tool(t) => t.icon(),
            Entry::Command { icon, .. } => icon,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Entry::Tool(t) => t.label(),
            Entry::Command { label, .. } => label,
        }
    }

    fn active(self, app: &PdfCraftApp) -> bool {
        match self {
            Entry::Tool(t) => app.quick_tool == QuickTool::Draft(t),
            Entry::Command { id, .. } => crate::comments::CommentTool::from_command(id).is_some_and(|c| app.quick_tool == QuickTool::Comment(c)),
        }
    }
}

use DraftTool as D;

const TEXT: Entry = Entry::Command { id: "comment.freetext", icon: CadIcon::Text, label: "Text (a text box on the drawing)" };
const CALLOUT: Entry = Entry::Command { id: "comment.callout", icon: CadIcon::Callout, label: "Callout (text with a leader line)" };

/// The fly-out groups: (caption, section, tools).
pub const GROUPS: [(&str, &str, &[Entry]); 9] = [
    ("Lines", "Draw", &[Entry::Tool(D::Line), Entry::Tool(D::Polyline), Entry::Tool(D::Spline)]),
    (
        "Shapes",
        "Draw",
        &[
            Entry::Tool(D::Rectangle),
            Entry::Tool(D::Circle),
            Entry::Tool(D::Arc),
            Entry::Tool(D::Ellipse),
            Entry::Tool(D::Polygon),
            Entry::Tool(D::Cloud),
        ],
    ),
    ("Walls", "Draw", &[Entry::Tool(D::Wall)]),
    ("Transform", "Modify", &[Entry::Tool(D::Move), Entry::Tool(D::Copy), Entry::Tool(D::Rotate), Entry::Tool(D::Mirror), Entry::Tool(D::Scale)]),
    ("Arrays", "Modify", &[Entry::Tool(D::ArrayRect), Entry::Tool(D::ArrayPolar)]),
    ("Edit", "Modify", &[Entry::Tool(D::Offset), Entry::Tool(D::Trim), Entry::Tool(D::Extend), Entry::Tool(D::Fillet), Entry::Tool(D::Chamfer)]),
    ("Break and join", "Modify", &[Entry::Tool(D::Break), Entry::Tool(D::Join), Entry::Tool(D::Explode), Entry::Tool(D::Erase)]),
    ("Dimensions", "Annotate", &[Entry::Tool(D::Dimension), Entry::Tool(D::DimLinear), Entry::Tool(D::DimRadius), Entry::Tool(D::DimDiameter)]),
    ("Text", "Annotate", &[TEXT, CALLOUT]),
];

const SECTIONS: [&str; 3] = ["Draw", "Modify", "Annotate"];
const BUTTON: f32 = 34.0;
const EXPANDED_WIDTH: f32 = 214.0;
const COLLAPSED_WIDTH: f32 = 48.0;

/// The tool a group shows when collapsed: the last one used from it.
fn shown(app: &PdfCraftApp, group: usize) -> Entry {
    let Some((_, _, entries)) = GROUPS.get(group) else { return Entry::Tool(D::Line) };
    let first = entries.first().copied().unwrap_or(Entry::Tool(D::Line));
    if let Some(e) = entries.iter().copied().find(|e| e.active(app)) {
        return e;
    }
    app.draft.group_tool.get(group).and_then(|t| entries.iter().copied().find(|e| *e == Entry::Tool(*t))).unwrap_or(first)
}

/// Pick `entry` (or put it down again when it is already in use).
fn choose(app: &mut PdfCraftApp, group: usize, entry: Entry, run: &mut Option<&'static str>) {
    match entry {
        Entry::Tool(t) => {
            let selected = app.quick_tool == QuickTool::Draft(t);
            app.quick_tool = if selected { QuickTool::Select } else { QuickTool::Draft(t) };
            if app.draft.group_tool.len() < GROUPS.len() {
                app.draft.group_tool.resize(GROUPS.len(), D::Line);
            }
            if let Some(slot) = app.draft.group_tool.get_mut(group) {
                *slot = t;
            }
        }
        Entry::Command { id, .. } => *run = Some(id),
    }
}

/// The small triangle in a button's lower-right corner that says it flies out.
fn flyout_mark(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.right_bottom() - vec2(4.0, 4.0);
    painter.add(egui::Shape::convex_polygon(vec![c + vec2(-5.0, 0.0), c, c + vec2(0.0, -5.0)], color, Stroke::NONE));
}

/// Draw the panel (only on the Draft tab, with a document open).
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let allowed = app.active_ids().and_then(|(_, id)| app.session.get(id)).is_some_and(|d| d.allows_annotation() && d.read_only_reason.is_none());
    let collapsed = app.draft.panel_collapsed;
    let mut run: Option<&'static str> = None;
    egui::Panel::left("draft_tools")
        .resizable(false)
        .exact_size(if collapsed { COLLAPSED_WIDTH } else { EXPANDED_WIDTH })
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(6, 6)).stroke(Stroke::new(1.0, t.divider)))
        .show(ui, |ui| {
            // Header: the title and the collapse button.
            ui.horizontal(|ui| {
                if !collapsed {
                    ui.label(egui::RichText::new(tl!("Draft tools")).font(theme::semibold(13.0)));
                    ui.add_space((ui.available_width() - 28.0).max(0.0));
                }
                let (icon, tip) = if collapsed { ("chevrons-right", tl!("Show tool names")) } else { ("chevrons-left", tl!("Collapse to icons")) };
                if icons::button(ui, icon, 28.0, false, tip).clicked() {
                    app.draft.panel_collapsed = !collapsed;
                }
            });
            ui.add_space(2.0);
            egui::ScrollArea::vertical().id_salt("draft-tools").auto_shrink([false, false]).show(ui, |ui| {
                ui.add_enabled_ui(allowed, |ui| {
                    if collapsed {
                        icon_column(app, ui, &t, &mut run);
                    } else {
                        sections(app, ui, &t, &mut run);
                    }
                });
            });
        });
    if let Some(id) = run {
        app.execute(id);
    }
}

/// Expanded: each section's groups, every tool a button, in rows.
fn sections(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, run: &mut Option<&'static str>) {
    for section in SECTIONS {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tl!(section)).font(theme::medium(12.0)).color(t.text_muted));
        let y = ui.cursor().top();
        ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, t.divider));
        ui.add_space(4.0);
        for (g, (caption, sec, entries)) in GROUPS.iter().enumerate() {
            if *sec != section {
                continue;
            }
            ui.label(egui::RichText::new(tl!(caption)).font(theme::regular(11.0)).color(t.text_faint));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                for e in entries.iter().copied() {
                    if cad_icons::button(ui, e.icon(), BUTTON, e.active(app), tl!(e.label())).clicked() {
                        choose(app, g, e, run);
                    }
                }
            });
            ui.add_space(2.0);
        }
    }
}

/// Collapsed: one button per group; its arrow (or a long press, or a right click) flies out the
/// group.
fn icon_column(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, run: &mut Option<&'static str>) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let mut last_section = "";
    for (g, (caption, section, entries)) in GROUPS.iter().enumerate() {
        if *section != last_section {
            if !last_section.is_empty() {
                ui.add_space(3.0);
                let y = ui.cursor().top();
                ui.painter().hline(ui.max_rect().x_range().shrink(4.0), y, Stroke::new(1.0, t.divider));
                ui.add_space(4.0);
            }
            last_section = section;
        }
        let entry = shown(app, g);
        let many = entries.len() > 1;
        let resp = cad_icons::button(ui, entry.icon(), BUTTON, entry.active(app), tl!(entry.label()));
        let rect = resp.rect;
        if many {
            flyout_mark(ui.painter(), rect, if ui.is_enabled() { t.text_muted } else { t.text_faint });
        }
        let popup_id = ui.id().with(("draft-flyout", g));
        // The arrow corner, a long press or a right click open the group.
        let corner = Rect::from_min_max(rect.right_bottom() - vec2(13.0, 13.0), rect.right_bottom());
        let on_arrow = resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| corner.contains(p));
        if many && (on_arrow || resp.secondary_clicked() || resp.long_touched()) {
            egui::Popup::open_id(ui.ctx(), popup_id);
        } else if resp.clicked() {
            choose(app, g, entry, run);
        }
        if many {
            let mut picked: Option<Entry> = None;
            egui::Popup::new(popup_id, ui.ctx().clone(), egui::PopupAnchor::ParentRect(rect), ui.layer_id())
                .open_memory(None)
                .align(egui::RectAlign::RIGHT_START)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                .show(|ui| {
                    ui.label(egui::RichText::new(tl!(caption)).font(theme::medium(12.0)).color(t.text_muted));
                    for e in entries.iter().copied() {
                        if flyout_row(ui, t, e, e.active(app)).clicked() {
                            picked = Some(e);
                        }
                    }
                });
            if let Some(e) = picked {
                choose(app, g, e, run);
            }
        }
    }
}

/// A fly-out row: the icon and the tool's name.
fn flyout_row(ui: &mut egui::Ui, t: &Tokens, e: Entry, selected: bool) -> egui::Response {
    let label = tl!(e.label());
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::regular(13.0), t.text);
    let size = Vec2::new((galley.size().x + 44.0).max(180.0), 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if selected {
        ui.painter().rect_filled(rect, t.radius, t.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(20.0));
    cad_icons::paint(ui.painter(), icon, e.icon(), if selected { t.accent_text } else { t.icon }, t.accent);
    ui.painter().galley(Pos2::new(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0), galley, t.text);
    resp
}
