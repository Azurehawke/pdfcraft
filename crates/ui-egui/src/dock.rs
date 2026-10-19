//! RevuCraft's panel dock: tool options, Properties and the side panels (comments, layers,
//! bookmarks…) as separate boxes stacked on the right, each with a title bar that collapses and
//! closes it, the way Illustrator and InDesign stack their panels. Several can be open at once;
//! open boxes share the dock's height. Drag a box's title to reorder it, or drop it over the
//! drawing to float it as its own window; a floating box docks again with its Dock button or by
//! dropping it on the dock. The layout (boxes, order, collapsed, floating) is remembered. The rail
//! beside it opens and closes the boxes. PdfCraft's left tool panel is not used: tools come from
//! the ribbon and the menus, and their options open here.

use egui::{Align, CornerRadius, Layout, Pos2, Rect, Sense, Stroke, vec2};

use crate::theme::{self, Tokens};
use crate::{LeftPanel, PdfCraftApp, RightPanel, icons};

/// A box in the dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    /// A tool group's options (the old left panel: measuring, stamps, editing…).
    Tool(&'static str),
    /// The selected markup's properties.
    Properties,
    Panel(RightPanel),
}

impl Item {
    fn key(self) -> String {
        match self {
            Item::Tool(id) => format!("tool:{id}"),
            Item::Properties => "properties".to_string(),
            Item::Panel(p) => format!("panel:{p:?}"),
        }
    }
}

/// Every side panel, for reading saved layouts.
const PANELS: [RightPanel; 13] = [
    RightPanel::Comments,
    RightPanel::Bookmarks,
    RightPanel::Pages,
    RightPanel::Fields,
    RightPanel::Layers,
    RightPanel::Attachments,
    RightPanel::Signatures,
    RightPanel::Accessibility,
    RightPanel::Search,
    RightPanel::Compare,
    RightPanel::Markups,
    RightPanel::ToolChest,
    RightPanel::Links,
];

/// A side panel by its saved name (`Comments`, `Layers`…).
pub(crate) fn panel_by_name(name: &str) -> Option<RightPanel> {
    PANELS.into_iter().find(|p| format!("{p:?}") == name)
}

const HEADER: f32 = 30.0;
/// The least height an open box gets; below that the dock scrolls.
const MIN_BODY: f32 = 140.0;
/// Saved layouts are untrusted: at most this many entries are read.
const MAX_SAVED: usize = 64;

impl PdfCraftApp {
    /// Whether `panel` is open (docked or floating).
    pub(crate) fn is_docked(&self, panel: RightPanel) -> bool {
        self.dock.contains(&panel)
    }

    /// Open `panel`, or close it if it is open (the rail buttons).
    pub(crate) fn toggle_docked(&mut self, panel: RightPanel) {
        if self.is_docked(panel) {
            self.undock(panel);
        } else {
            self.dock.push(panel);
            self.dock_collapsed.retain(|k| *k != Item::Panel(panel).key());
            self.choose_right_panel(Some(panel));
        }
    }

    /// Show or hide the Properties box.
    pub(crate) fn toggle_properties(&mut self) {
        self.dock_properties = !self.dock_properties;
        if self.dock_properties {
            self.dock_collapsed.retain(|k| *k != Item::Properties.key());
        }
    }

    fn undock(&mut self, panel: RightPanel) {
        self.dock.retain(|p| *p != panel);
        self.dock_floating.retain(|k| *k != Item::Panel(panel).key());
        if self.right == Some(panel) {
            // Closing Comments this way also keeps comment tools from reopening it.
            self.choose_right_panel(None);
            self.right = self.dock.last().copied();
        }
    }

    fn close_item(&mut self, item: Item) {
        match item {
            Item::Tool(_) => self.left_open = false,
            Item::Properties => self.dock_properties = false,
            Item::Panel(p) => self.undock(p),
        }
        self.dock_floating.retain(|k| *k != item.key());
    }

    /// The open boxes, in the user's order.
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
        if self.dock_properties {
            items.push(Item::Properties);
        }
        items.extend(self.dock.iter().map(|p| Item::Panel(*p)));
        for item in &items {
            let key = item.key();
            if !self.dock_order.contains(&key) {
                // New boxes: tool options at the top, everything else at the bottom.
                if matches!(item, Item::Tool(_)) {
                    self.dock_order.insert(0, key);
                } else {
                    self.dock_order.push(key);
                }
            }
        }
        let order = &self.dock_order;
        items.sort_by_key(|i| order.iter().position(|k| *k == i.key()).unwrap_or(usize::MAX));
        items
    }

    /// The dock layout, for the settings file.
    pub(crate) fn dock_layout(&self) -> serde_json::Value {
        serde_json::json!({
            "panels": self.dock.iter().map(|p| format!("{p:?}")).collect::<Vec<_>>(),
            "order": self.dock_order,
            "collapsed": self.dock_collapsed,
            "floating": self.dock_floating,
            "properties": self.dock_properties,
        })
    }

    /// Restore the dock layout saved by [`Self::dock_layout`] (anything unreadable is skipped).
    pub(crate) fn restore_dock_layout(&mut self, v: &serde_json::Value) {
        let strings = |key: &str| -> Vec<String> {
            v[key].as_array().map(|a| a.iter().filter_map(|s| s.as_str()).take(MAX_SAVED).map(str::to_string).collect()).unwrap_or_default()
        };
        self.dock = strings("panels").iter().filter_map(|n| panel_by_name(n)).collect();
        self.dock.dedup();
        self.dock_order = strings("order");
        self.dock_collapsed = strings("collapsed");
        self.dock_floating = strings("floating");
        self.dock_properties = v["properties"].as_bool().unwrap_or(false);
    }
}

fn title(item: Item) -> &'static str {
    match item {
        Item::Tool(id) => pdfcraft_engine::catalog::group(id).map_or("Tools", |g| g.label),
        Item::Properties => "Properties",
        Item::Panel(p) => crate::panels::panel_title(p),
    }
}

fn body(app: &mut PdfCraftApp, ui: &mut egui::Ui, item: Item) {
    match item {
        Item::Tool(id) => crate::panels::tool_options(app, ui, id),
        Item::Properties => crate::props_box::show(app, ui),
        Item::Panel(p) => crate::panels::docked_panel(app, ui, p),
    }
}

/// What the user did to a box this frame.
enum Act {
    Toggle(Item),
    Close(Item),
    Float(Item, Pos2),
    Dock(Item),
    /// Move the box before the one at this position in the docked list (or to the end).
    Move(Item, usize),
}

/// Draw the dock and the floating boxes (nothing when no box is open).
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    if app.active.is_none() {
        return;
    }
    let items = app.dock_items();
    let (floating, docked): (Vec<Item>, Vec<Item>) = items.iter().partition(|i| app.dock_floating.contains(&i.key()));
    let t = Tokens::get(ui.ctx());
    let mut acts: Vec<Act> = Vec::new();
    if docked.is_empty() {
        app.dock_rect = None;
    } else {
        let panel = egui::Panel::right("dock")
            .resizable(true)
            .default_size(330.0)
            .size_range(260.0..=560.0)
            .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::same(6)).stroke(Stroke::new(1.0, t.divider)))
            .show(ui, |ui| docked_boxes(app, ui, &t, &docked, &mut acts));
        app.dock_rect = Some(panel.response.rect);
    }
    for item in &floating {
        floating_box(app, ui.ctx(), &t, *item, &mut acts);
    }
    for act in acts {
        match act {
            Act::Toggle(item) => {
                let key = item.key();
                if app.dock_collapsed.contains(&key) {
                    app.dock_collapsed.retain(|k| *k != key);
                } else {
                    app.dock_collapsed.push(key);
                }
            }
            Act::Close(item) => app.close_item(item),
            Act::Float(item, at) => {
                let key = item.key();
                if !app.dock_floating.contains(&key) {
                    app.dock_floating.push(key.clone());
                }
                app.dock_collapsed.retain(|k| *k != key);
                // The window opens where the box was dropped.
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("dock-float-at", key)), at));
            }
            Act::Dock(item) => app.dock_floating.retain(|k| *k != item.key()),
            Act::Move(item, before) => {
                let key = item.key();
                let target = docked.get(before).map(|i| i.key());
                app.dock_order.retain(|k| *k != key);
                let at = target.and_then(|t| app.dock_order.iter().position(|k| *k == t)).unwrap_or(app.dock_order.len());
                app.dock_order.insert(at, key);
            }
        }
    }
}

fn docked_boxes(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, items: &[Item], acts: &mut Vec<Act>) {
    let dock_rect = ui.max_rect();
    let open = items.iter().filter(|i| !app.dock_collapsed.contains(&i.key())).count();
    let gaps = 6.0 * items.len() as f32;
    let free = ui.available_height() - HEADER * items.len() as f32 - gaps;
    let body_h = if open == 0 { 0.0 } else { (free / open as f32).max(MIN_BODY) };
    let dragging = ui.ctx().data(|d| d.get_temp::<String>(egui::Id::new("dock-drag")));
    let pointer = ui.ctx().pointer_latest_pos();
    let mut rects: Vec<Rect> = Vec::new();
    egui::ScrollArea::vertical().id_salt("dock-scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        for item in items {
            let key = item.key();
            let collapsed = app.dock_collapsed.contains(&key);
            let width = ui.available_width();
            let height = HEADER + if collapsed { 0.0 } else { body_h };
            let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            rects.push(rect);
            let being_dragged = dragging.as_deref() == Some(key.as_str());
            let stroke = if being_dragged { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.divider) };
            ui.painter().rect(rect, CornerRadius::same(6), t.panel, stroke, egui::StrokeKind::Inside);
            let bar = Rect::from_min_size(rect.min, vec2(width, HEADER));
            if let Some(act) = title_bar(ui, t, bar, *item, collapsed, true) {
                acts.push(act);
            }
            // Dragging the title: reorder within the dock, or float when dropped elsewhere.
            // The title area between the collapse button and the float and close buttons.
            let grab = Rect::from_min_max(egui::pos2(bar.left() + 32.0, bar.top()), egui::pos2(bar.right() - 64.0, bar.bottom()));
            let drag = ui.interact(grab, egui::Id::new(("dock-title", &key)), Sense::click_and_drag());
            if drag.drag_started() {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("dock-drag"), key.clone()));
            }
            if drag.double_clicked() {
                acts.push(Act::Toggle(*item));
            }
            if drag.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if drag.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if drag.drag_stopped() {
                ui.ctx().data_mut(|d| d.remove::<String>(egui::Id::new("dock-drag")));
                if let Some(p) = pointer {
                    if dock_rect.contains(p) {
                        acts.push(Act::Move(*item, usize::MAX));
                    } else {
                        acts.push(Act::Float(*item, p));
                    }
                }
            }
            if collapsed {
                continue;
            }
            ui.painter().hline(rect.x_range().shrink(1.0), bar.bottom(), Stroke::new(1.0, t.divider));
            let inner = Rect::from_min_max(egui::pos2(rect.left() + 10.0, bar.bottom() + 6.0), egui::pos2(rect.right() - 8.0, rect.bottom() - 6.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)).id_salt(&key));
            child.set_clip_rect(inner.intersect(ui.clip_rect()));
            body(app, &mut child, *item);
        }
    });
    // While a box is dragged over the dock, show where it will go; a drop there moves it.
    if let (Some(key), Some(p)) = (dragging.as_deref(), pointer)
        && dock_rect.contains(p)
    {
        let before = rects.iter().position(|r| p.y < r.center().y).unwrap_or(rects.len());
        let y = rects.get(before).map_or_else(|| rects.last().map_or(dock_rect.top(), |r| r.bottom() + 3.0), |r| r.top() - 3.0);
        ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dock-drop"))).hline(
            dock_rect.x_range().shrink(6.0),
            y,
            Stroke::new(3.0, t.accent),
        );
        // Resolve the pending move (pushed with a placeholder index above) to the drop slot.
        for act in acts.iter_mut() {
            if let Act::Move(item, slot) = act
                && item.key() == key
            {
                *slot = before;
            }
        }
    }
}

/// A box's title bar: collapse, title, float or dock, close.
fn title_bar(ui: &mut egui::Ui, t: &Tokens, bar: Rect, item: Item, collapsed: bool, docked: bool) -> Option<Act> {
    let mut act = None;
    let mut head = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(4.0, 2.0))).layout(Layout::left_to_right(Align::Center)));
    if docked {
        let chevron = if collapsed { "chevron-right" } else { "chevron-down" };
        let tip = if collapsed { tl!("Expand panel") } else { tl!("Collapse panel") };
        if icons::button(&mut head, chevron, 24.0, false, tip).clicked() {
            act = Some(Act::Toggle(item));
        }
        head.add(
            egui::Label::new(egui::RichText::new(crate::i18n::t(title(item))).font(theme::semibold(13.0)).color(t.text)).truncate().selectable(false),
        );
    }
    head.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if docked && icons::button(ui, "x", 24.0, false, tl!("Close panel")).clicked() {
            act = Some(Act::Close(item));
        }
        if docked {
            if icons::button(ui, "external-link", 24.0, false, tl!("Float this panel")).clicked() {
                let at = ui.ctx().content_rect().center() - vec2(160.0, 200.0);
                act = Some(Act::Float(item, at));
            }
        } else if icons::button(ui, "panel-right", 24.0, false, tl!("Dock this panel")).clicked() {
            act = Some(Act::Dock(item));
        }
    });
    act
}

fn floating_box(app: &mut PdfCraftApp, ctx: &egui::Context, t: &Tokens, item: Item, acts: &mut Vec<Act>) {
    let key = item.key();
    let at = ctx.data(|d| d.get_temp::<Pos2>(egui::Id::new(("dock-float-at", key.clone()))));
    let mut open = true;
    let mut window = egui::Window::new(crate::i18n::t(title(item)))
        .id(egui::Id::new(("dock-float", &key)))
        .default_size([320.0, 420.0])
        .resizable(true)
        .collapsible(true)
        .open(&mut open);
    if let Some(at) = at {
        window = window.current_pos(at);
        ctx.data_mut(|d| d.remove::<Pos2>(egui::Id::new(("dock-float-at", key.clone()))));
    }
    let shown = window.show(ctx, |ui| {
        let bar = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 26.0));
        ui.allocate_rect(bar, Sense::hover());
        if let Some(act) = title_bar(ui, t, bar, item, false, false) {
            acts.push(act);
        }
        body(app, ui, item);
    });
    if !open {
        acts.push(Act::Close(item));
    }
    // Dropped on the dock: dock it again.
    if let (Some(shown), Some(dock)) = (shown, app.dock_rect)
        && !ctx.input(|i| i.pointer.any_down())
        && ctx.input(|i| i.pointer.any_released())
        && dock.contains(shown.response.rect.center())
    {
        acts.push(Act::Dock(item));
    }
}
