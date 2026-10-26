//! Pen-only drawing on tablets.
//!
//! The Android shell knows which kind of contact drives egui's pointer (the S Pen, a finger, a
//! palm, a mouse) and says so here every frame. With pen-only drawing on, a finger on the page
//! scrolls it as the Hand tool would, whatever tool is chosen, so only the pen (or a mouse) draws,
//! highlights or places anything. Elsewhere nothing sets these and the page behaves as before.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

/// What drives the pointer this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerSource {
    /// Not known (desktops, the web): every tool works as usual.
    Unknown,
    Finger,
    Pen,
    Mouse,
}

static SOURCE: AtomicU8 = AtomicU8::new(0);
/// The pen's pressure as f32 bits; `NO_PRESSURE` when none is reported.
static PRESSURE: AtomicU32 = AtomicU32::new(NO_PRESSURE);
const NO_PRESSURE: u32 = u32::MAX;

/// The pen's current pressure (0–1), or `None` when the pointer reports none (a finger, a mouse,
/// a desktop). Set by the shell as touches arrive; the Draw tool records it with each point.
pub fn set_pressure(pressure: Option<f32>) {
    let bits = pressure.filter(|p| p.is_finite()).map_or(NO_PRESSURE, |p| p.clamp(0.0, 1.0).to_bits());
    PRESSURE.store(bits, Ordering::Relaxed);
}

pub fn pressure() -> Option<f64> {
    let bits = PRESSURE.load(Ordering::Relaxed);
    (bits != NO_PRESSURE).then(|| f64::from(f32::from_bits(bits)))
}
static PEN_ONLY: AtomicBool = AtomicBool::new(false);
/// A pen has touched the screen this session. Pen-only drawing waits for one: on a phone (or a
/// tablet used without its pen) fingers draw and place things, as they must. From RevuCraft.
static PEN_SEEN: AtomicBool = AtomicBool::new(false);

pub fn set_pointer_source(source: PointerSource) {
    let code = match source {
        PointerSource::Unknown => 0,
        PointerSource::Finger => 1,
        PointerSource::Pen => 2,
        PointerSource::Mouse => 3,
    };
    if source == PointerSource::Pen {
        PEN_SEEN.store(true, Ordering::Relaxed);
    }
    SOURCE.store(code, Ordering::Relaxed);
}

pub fn pointer_source() -> PointerSource {
    match SOURCE.load(Ordering::Relaxed) {
        1 => PointerSource::Finger,
        2 => PointerSource::Pen,
        3 => PointerSource::Mouse,
        _ => PointerSource::Unknown,
    }
}

/// Turn pen-only drawing on or off (the Android shell turns it on).
pub fn set_pen_only_drawing(on: bool) {
    PEN_ONLY.store(on, Ordering::Relaxed);
}

/// Pen-only drawing is in force: turned on, and a pen has been used.
pub fn pen_only_drawing() -> bool {
    pen_only_enabled() && PEN_SEEN.load(Ordering::Relaxed)
}

/// Pen-only drawing is turned on (it takes effect once a pen touches the screen).
pub fn pen_only_enabled() -> bool {
    PEN_ONLY.load(Ordering::Relaxed)
}

/// A finger drives the pointer while pen-only drawing is on: the page treats it as the Hand tool.
pub(crate) fn finger_pans() -> bool {
    pen_only_drawing() && pointer_source() == PointerSource::Finger
}

/// The pen drives the pointer while pen-only drawing is on: dragging it never scrolls the page.
pub(crate) fn pen_draws() -> bool {
    pen_only_drawing() && pointer_source() == PointerSource::Pen
}
