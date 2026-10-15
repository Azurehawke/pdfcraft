//! RevuCraft's panel dock: tool options and the side panels (comments, layers, bookmarks…) as
//! separate boxes stacked on the right, each with a title bar that collapses and closes it, the
//! way Illustrator and InDesign stack their panels. Several can be open at once; open boxes share
//! the dock's height. The rail beside it opens and closes them. PdfCraft's left tool panel is not
//! used: tools come from the ribbon and the menus, and their options open here.

use egui::{Align, CornerRadius, Layout, Sense, Stroke, vec2};

use crate::theme::{self, Tokens};
use crate::{LeftPanel, PdfCraftApp, RightPanel, icons};

/// A box in the dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    /// A tool group's options (the old left panel: measuring, stamps, editing…).
    Tool(&'static str),
    Panel(RightPanel),
}

impl Item {
    fn key(self) -> String {
        match self {
            Item::Tool(id) => format!("tool:{id}"),
            Item::Panel(p) => format!("panel:{p:?}"),
        }
    }
}

const HEADER: f32 = 30.0;
/// The least height an open box gets; below that the dock scrolls.
const MIN_BODY: f32 = 140.0;

impl PdfCraftApp {
    /// Whether `panel` is open in the dock.
    pub(crate) fn is_docked(&self, panel: RightPanel) -> bool {
        self.dock.contains(&panel)
    }

    /// Open `panel` in the dock, or close it if it is open (the rail buttons).
    pub(crate) fn toggle_docked(&mut self, panel: RightPanel) {
        if self.is_docked(panel) {
            self.undock(panel);
        } else {
            self.dock.push(panel);
            self.dock_collapsed.retain(|k| *k != Item::Panel(panel).key());
            self.choose_right_panel(Some(panel));
        }
    }

    fn undock(&mut self, panel: RightPanel) {
        self.dock.retain(|p| *p != panel);
        if self.right == Some(panel) {
            // Closing Comments this way also keeps comment tools from reopening it.
            self.choose_right_panel(None);
            self.right = self.dock.last().copied();
        }
    }

    /// The boxes to show: the open tool's options first, then the panels in the order opened.
    fn dock_items(&mut self) -> Vec<Item> {
        // Panels opened elsewhere (a comment tool opens Comments, a command opens Layers) join
        // the dock.
        if let Some(p) = self.right
            && !self.dock.contains(&p)
        {
            self.dock.push(p);
        }
        let mut items = Vec::new();
        match self.left {
            LeftPanel::Tool(id) if self.left_open && pdfcraft_engine::catalog::group(id).is_some() => items.push(Item::Tool(id)),
            // The full tool list lives in the All tools menu now.
            LeftPanel::AllTools => self.left_open = false,
            _ => {}
        }
        items.extend(self.dock.iter().map(|p| Item::Panel(*p)));
        items
    }
}

fn title(item: Item) -> &'static str {
    match item {
        Item::Tool(id) => pdfcraft_engine::catalog::group(id).map_or("Tools", |g| g.label),
        Item::Panel(p) => crate::panels::panel_title(p),
    }
}

/// Draw the dock (nothing when no box is open).
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    if app.active.is_none() {
        return;
    }
    let items = app.dock_items();
    if items.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let mut close: Option<Item> = None;
    let mut toggle: Option<Item> = None;
    egui::Panel::right("dock")
        .resizable(true)
        .default_size(330.0)
        .size_range(260.0..=560.0)
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::same(6)).stroke(Stroke::new(1.0, t.divider)))
        .show(ui, |ui| {
            let open = items.iter().filter(|i| !app.dock_collapsed.contains(&i.key())).count();
            let gaps = 6.0 * items.len() as f32;
            let free = ui.available_height() - HEADER * items.len() as f32 - gaps;
            let body_h = if open == 0 { 0.0 } else { (free / open as f32).max(MIN_BODY) };
            egui::ScrollArea::vertical().id_salt("dock-scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for item in &items {
                    let collapsed = app.dock_collapsed.contains(&item.key());
                    let width = ui.available_width();
                    let height = HEADER + if collapsed { 0.0 } else { body_h };
                    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                    ui.painter().rect(rect, CornerRadius::same(6), t.panel, Stroke::new(1.0, t.divider), egui::StrokeKind::Inside);
                    // Title bar: collapse, title, close.
                    let bar = egui::Rect::from_min_size(rect.min, vec2(width, HEADER));
                    let mut head =
                        ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(4.0, 2.0))).layout(Layout::left_to_right(Align::Center)));
                    let chevron = if collapsed { "chevron-right" } else { "chevron-down" };
                    let tip = if collapsed { tl!("Expand panel") } else { tl!("Collapse panel") };
                    if icons::button(&mut head, chevron, 24.0, false, tip).clicked() {
                        toggle = Some(*item);
                    }
                    let label = head.add(
                        egui::Label::new(egui::RichText::new(crate::i18n::t(title(*item))).font(theme::semibold(13.0)).color(t.text))
                            .truncate()
                            .sense(Sense::click()),
                    );
                    if label.double_clicked() {
                        toggle = Some(*item);
                    }
                    head.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(ui, "x", 24.0, false, tl!("Close panel")).clicked() {
                            close = Some(*item);
                        }
                    });
                    if collapsed {
                        continue;
                    }
                    ui.painter().hline(rect.x_range().shrink(1.0), bar.bottom(), Stroke::new(1.0, t.divider));
                    let inner = egui::Rect::from_min_max(
                        egui::pos2(rect.left() + 10.0, bar.bottom() + 6.0),
                        egui::pos2(rect.right() - 8.0, rect.bottom() - 6.0),
                    );
                    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)).id_salt(item.key()));
                    body.set_clip_rect(inner.intersect(ui.clip_rect()));
                    match item {
                        Item::Tool(id) => crate::panels::tool_options(app, &mut body, id),
                        Item::Panel(p) => crate::panels::docked_panel(app, &mut body, *p),
                    }
                }
            });
        });
    if let Some(item) = toggle {
        let key = item.key();
        if app.dock_collapsed.contains(&key) {
            app.dock_collapsed.retain(|k| *k != key);
        } else {
            app.dock_collapsed.push(key);
        }
    }
    match close {
        Some(Item::Tool(_)) => app.left_open = false,
        Some(Item::Panel(p)) => app.undock(p),
        None => {}
    }
}
