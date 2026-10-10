//! What a tablet needs around the desktop UI: room for the system bars, and pen-first input.
//!
//! - **System bars.** NativeActivity draws under the status bar and the taskbar. Android reports
//!   the part of the window they leave free (the content rect); the app is laid out inside it,
//!   and egui is told the same insets so dialogs and menus stay clear too.
//! - **Pen-first input.** vendor/winit tags each touch with its tool (finger, S Pen, eraser,
//!   mouse, palm). egui-winit would turn whichever touch came first into the pointer, so a palm
//!   resting on the screen before the pen landed would take the pointer. Here the pointer is
//!   rebuilt from the touches instead: the pen always wins, a palm never counts, and fingers
//!   only count while no pen is down (and then pan the page; see `pdfcraft_ui_egui::pen`).

use std::collections::{HashMap, HashSet};

use egui::{Event, Modifiers, Pos2, TouchPhase};
use pdfcraft_ui_egui::PdfCraftApp;
use pdfcraft_ui_egui::pen::{self, PointerSource};
use winit::platform::android::activity::AndroidApp;

pub struct TabletApp {
    pub app: PdfCraftApp,
    android: AndroidApp,
    touches: TouchRouter,
    /// The insets last applied, in points.
    insets: egui::epaint::MarginF32,
}

impl TabletApp {
    pub fn new(app: PdfCraftApp, android: AndroidApp) -> Self {
        Self { app, android, touches: TouchRouter::default(), insets: egui::epaint::MarginF32::default() }
    }

    /// With a keyboard cover or Bluetooth keyboard attached, typing arrives as key presses, so a
    /// text field doesn't ask for the on-screen keyboard: Samsung shows its keyboard toolbar for
    /// that request, which takes room from the app (the content rect shrinks) and comes and goes
    /// as fields gain and lose focus. Without one, fields ask as usual.
    fn keep_soft_keyboard_away(&self, ctx: &egui::Context) {
        if matches!(self.android.config().keys_hidden(), ndk::configuration::KeysHidden::No) {
            ctx.output_mut(|o| o.ime = None);
        }
    }

    /// The window's edges covered by system bars, in points, from Android's content rect.
    fn system_bar_insets(&self, ctx: &egui::Context, raw: &egui::RawInput) -> Option<egui::epaint::MarginF32> {
        let ppp = ctx.pixels_per_point();
        let screen = raw.screen_rect?;
        if !(ppp.is_finite() && ppp > 0.0) {
            return None;
        }
        let r = self.android.content_rect();
        if r.right <= r.left || r.bottom <= r.top {
            return None;
        }
        let (w, h) = (screen.width() * ppp, screen.height() * ppp);
        let inset = |px: f32| (px / ppp).clamp(0.0, 400.0);
        Some(egui::epaint::MarginF32 {
            left: inset(r.left as f32),
            top: inset(r.top as f32),
            right: inset(w - r.right as f32),
            bottom: inset(h - r.bottom as f32),
        })
    }
}

impl eframe::App for TabletApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(insets) = self.system_bar_insets(ctx, raw) {
            if insets != self.insets {
                log::info!("system bar insets (points): {insets:?}");
            }
            self.insets = insets;
            raw.safe_area_insets = Some(egui::SafeAreaInsets(insets));
        }
        self.touches.route(&mut raw.events);
        self.app.raw_input_hook(ctx, raw);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let full = ui.max_rect();
        let m = self.insets;
        let inner = egui::Rect::from_min_max(full.min + egui::vec2(m.left, m.top), full.max - egui::vec2(m.right, m.bottom));
        if inner.width() < 100.0 || inner.height() < 100.0 {
            self.app.ui(ui, frame);
            self.keep_soft_keyboard_away(ui.ctx());
            return;
        }
        // Behind the status bar: the tab strip's colour, so the bar reads as part of the app.
        let bar = pdfcraft_ui_egui::theme::Tokens::get(ui.ctx()).titlebar;
        ui.painter().rect_filled(full, 0.0, bar);
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.set_clip_rect(inner);
            self.app.ui(ui, frame);
        });
        self.keep_soft_keyboard_away(ui.ctx());
    }
}

/// Which tool a touch is, from the bits vendor/winit puts above the pointer id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Finger,
    Pen,
    Mouse,
    Palm,
}

fn tool_of(id: u64) -> Tool {
    match (id >> 32) & 0xff {
        2 | 4 => Tool::Pen, // stylus, eraser end
        3 => Tool::Mouse,
        5 => Tool::Palm,
        _ => Tool::Finger, // finger, or unknown
    }
}

/// A touch that is down.
#[derive(Clone, Copy, Debug)]
struct Down {
    tool: Tool,
    device: egui::TouchDeviceId,
    pos: Pos2,
}

#[derive(Default)]
struct TouchRouter {
    /// Touches down now, by id (hidden ones too).
    down: HashMap<u64, Down>,
    /// Touches whose events egui doesn't see (a palm; fingers while the pen is down).
    hidden: HashSet<u64>,
    /// The touch that drives egui's pointer.
    owner: Option<u64>,
    /// Modifier keys as last reported (pointer events carry them).
    modifiers: Modifiers,
}

impl TouchRouter {
    fn pen_down(&self) -> bool {
        self.down.values().any(|d| d.tool == Tool::Pen)
    }

    fn press(&self, out: &mut Vec<Event>, pos: Pos2, pressed: bool) {
        out.push(Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: self.modifiers });
    }

    /// Stop egui seeing a touch it has seen start: it ends as cancelled.
    fn hide(&mut self, id: u64, out: &mut Vec<Event>) {
        if !self.hidden.insert(id) {
            return;
        }
        if let Some(d) = self.down.get(&id) {
            out.push(Event::Touch { device_id: d.device, id: egui::TouchId(id), phase: TouchPhase::Cancel, pos: d.pos, force: None });
        }
    }

    /// Replace egui-winit's touch-to-pointer translation with a pen-first one (module docs).
    fn route(&mut self, events: &mut Vec<Event>) {
        if !pen::pen_only_drawing() {
            return;
        }
        let mut out = Vec::with_capacity(events.len());
        for event in std::mem::take(events) {
            match event {
                Event::Key { modifiers, .. } => {
                    self.modifiers = modifiers;
                    out.push(event);
                }
                Event::PointerButton { modifiers, .. } => {
                    // Rebuilt from the touches below; only its modifiers are kept.
                    self.modifiers = modifiers;
                }
                // On Android every pointer event comes from a touch: rebuilt below.
                Event::PointerMoved(_) | Event::PointerGone => {}
                Event::Touch { device_id, id, phase, pos, force } => {
                    self.touch(device_id, id.0, phase, pos, force, &mut out);
                    let ended = matches!(phase, TouchPhase::End | TouchPhase::Cancel);
                    let hidden = if ended { self.hidden.remove(&id.0) } else { self.hidden.contains(&id.0) };
                    if !hidden {
                        out.push(event);
                    }
                }
                other => out.push(other),
            }
        }
        *events = out;
    }

    fn touch(&mut self, device: egui::TouchDeviceId, id: u64, phase: TouchPhase, pos: Pos2, force: Option<f32>, out: &mut Vec<Event>) {
        let tool = tool_of(id);
        match phase {
            TouchPhase::Start => {
                let pen_was_down = self.pen_down();
                self.down.insert(id, Down { tool, device, pos });
                if tool == Tool::Palm || (tool == Tool::Finger && pen_was_down) {
                    // Not shown to egui at all: nothing to cancel.
                    self.hidden.insert(id);
                    return;
                }
                if tool == Tool::Pen {
                    // The pen takes over from a finger or palm that got there first, and the
                    // fingers on the screen stop counting as a gesture.
                    if let Some(owner) = self.owner.take()
                        && let Some(d) = self.down.get(&owner).copied()
                    {
                        self.press(out, d.pos, false);
                        out.push(Event::PointerGone);
                    }
                    let fingers: Vec<u64> = self.down.iter().filter(|(_, d)| d.tool == Tool::Finger).map(|(id, _)| *id).collect();
                    for finger in fingers {
                        self.hide(finger, out);
                    }
                }
                if self.owner.is_none() {
                    self.owner = Some(id);
                    pen::set_pointer_source(match tool {
                        Tool::Pen => PointerSource::Pen,
                        Tool::Mouse => PointerSource::Mouse,
                        Tool::Finger | Tool::Palm => PointerSource::Finger,
                    });
                    // The Draw tool records the pen's pressure with each point (`pen::pressure`).
                    pen::set_pressure(if tool == Tool::Pen { force } else { None });
                    out.push(Event::PointerMoved(pos));
                    self.press(out, pos, true);
                }
            }
            TouchPhase::Move => {
                if let Some(d) = self.down.get_mut(&id) {
                    d.pos = pos;
                }
                if self.owner == Some(id) {
                    if tool == Tool::Pen {
                        pen::set_pressure(force);
                    }
                    out.push(Event::PointerMoved(pos));
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                self.down.remove(&id);
                if self.owner == Some(id) {
                    self.owner = None;
                    self.press(out, pos, false);
                    out.push(Event::PointerGone);
                }
            }
        }
    }
}
