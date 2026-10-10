//! Pen-only drawing on tablets.
//!
//! The Android shell knows which kind of contact drives egui's pointer (the S Pen, a finger, a
//! palm, a mouse) and says so here every frame. With pen-only drawing on, a finger on the page
//! scrolls it as the Hand tool would, whatever tool is chosen, so only the pen (or a mouse) draws,
//! highlights or places anything. Elsewhere nothing sets these and the page behaves as before.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

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
static PEN_ONLY: AtomicBool = AtomicBool::new(false);

pub fn set_pointer_source(source: PointerSource) {
    let code = match source {
        PointerSource::Unknown => 0,
        PointerSource::Finger => 1,
        PointerSource::Pen => 2,
        PointerSource::Mouse => 3,
    };
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

pub fn pen_only_drawing() -> bool {
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
