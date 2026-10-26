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

use crate::incoming::Incoming;
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
    /// Look for a PDF sent by another app (`incoming`): at start-up and when the app comes back
    /// to the front.
    check_incoming: bool,
    focused: bool,
    /// New builds from the GitHub repository the APK came from (`updater`).
    updater: crate::updater::Updater,
}

impl TabletApp {
    pub fn new(app: PdfCraftApp, android: AndroidApp) -> Self {
        Self {
            app,
            android,
            touches: TouchRouter::default(),
            insets: egui::epaint::MarginF32::default(),
            check_incoming: true,
            focused: true,
            updater: crate::updater::Updater::default(),
        }
    }

    /// Open the PDF another app sent, if there is one.
    fn open_incoming(&mut self) {
        match crate::incoming::take() {
            None => {}
            Some(Incoming::Path(path)) => self.app.open_recent(&path),
            Some(Incoming::Bytes { name, bytes }) => {
                if let Err(e) = self.app.open_bytes(&name, None, bytes) {
                    self.app.notify_fmt("Couldn't open {name}: {e}", &[("name", &name), ("e", &e)]);
                }
            }
            Some(Incoming::Failed(e)) => self.app.notify_error(e),
        }
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
        if raw.focused && !self.focused {
            // Back in front, perhaps with another PDF to open.
            self.check_incoming = true;
        }
        self.focused = raw.focused;
        self.touches.route(&mut raw.events);
        if self.touches.pending() {
            // A lift held back for the next frame (`TouchRouter::next`).
            ctx.request_repaint();
        }
        self.app.raw_input_hook(ctx, raw);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.check_incoming) {
            self.open_incoming();
        }
        // Documents from Google Drive and other apps: write back what PdfCraft saved.
        crate::cloud::sync(&mut self.app, ctx);
        // At start-up, then at most hourly (`updater`).
        self.updater.maybe_check(ctx.input(|i| i.time));
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
        self.updater.show(ui.ctx(), inner);
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
    /// Pointer events for the next frame, and for the one after. A quick tap often arrives in one
    /// frame (down, up), and egui finds what is under the pointer once per frame, after the
    /// frame's events: with the lift and the pointer's leaving in the same frame as the press,
    /// nothing was under it and the tap clicked nothing. So a lift waits for the next frame when
    /// its press came in this one, and the pointer leaves one frame after the lift.
    next: Vec<Event>,
    after: Vec<Event>,
    /// Touches that started in the frame being routed.
    started_now: HashSet<u64>,
}

impl TouchRouter {
    /// Pointer events wait for a later frame.
    fn pending(&self) -> bool {
        !self.next.is_empty() || !self.after.is_empty()
    }

    /// The owner's lift at `pos`: now (or next frame if it was pressed in this one), and the
    /// pointer leaves a frame later.
    fn lift(&mut self, id: u64, pos: Pos2, out: &mut Vec<Event>) {
        let release = Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: self.modifiers };
        if self.started_now.contains(&id) {
            self.next.push(release);
            self.after.push(Event::PointerGone);
        } else {
            out.push(release);
            self.next.push(Event::PointerGone);
        }
    }

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
        // What earlier frames held back comes first.
        let mut out = std::mem::take(&mut self.next);
        self.next = std::mem::take(&mut self.after);
        self.started_now.clear();
        // egui-winit puts a touch's pointer events right after its `Touch` event: those are
        // rebuilt here. Other pointer events come from a mouse (vendor/winit) and stay.
        let mut after_touch = false;
        for event in std::mem::take(events) {
            match event {
                Event::Touch { device_id, id, phase, pos, force } => {
                    after_touch = true;
                    self.touch(device_id, id.0, phase, pos, force, &mut out);
                    let ended = matches!(phase, TouchPhase::End | TouchPhase::Cancel);
                    let hidden = if ended { self.hidden.remove(&id.0) } else { self.hidden.contains(&id.0) };
                    if !hidden {
                        out.push(event);
                    }
                }
                Event::PointerMoved(_) | Event::PointerButton { .. } | Event::PointerGone if after_touch => {
                    if let Event::PointerButton { modifiers, .. } = event {
                        self.modifiers = modifiers;
                    }
                }
                other => {
                    after_touch = false;
                    match &other {
                        Event::Key { modifiers, .. } => self.modifiers = *modifiers,
                        // A mouse button: the mouse drives the pointer (it may draw).
                        Event::PointerButton { modifiers, pressed: true, .. } => {
                            self.modifiers = *modifiers;
                            pen::set_pointer_source(PointerSource::Mouse);
                            pen::set_pressure(None);
                        }
                        _ => {}
                    }
                    out.push(other);
                }
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
                    self.started_now.insert(id);
                    // A new press: the last touch's pointer doesn't leave after all.
                    out.retain(|e| !matches!(e, Event::PointerGone));
                    self.next.retain(|e| !matches!(e, Event::PointerGone));
                    self.after.retain(|e| !matches!(e, Event::PointerGone));
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
                    self.lift(id, pos, out);
                }
            }
        }
    }
}
