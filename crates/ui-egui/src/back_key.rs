//! Android's Back key. winit reports it as `BrowserBack`, which nothing in the app read, so Back
//! did nothing: a note, a dialog or a menu stayed open. Those close on Escape, so Back is
//! delivered as Escape.

use egui::{Event, Key};

/// Turns each Back press and release into an Escape press and release, in place. Other events
/// are left as they are.
pub fn back_as_escape(events: &mut [Event]) {
    for event in events.iter_mut() {
        if let Event::Key { key, .. } = event
            && *key == Key::BrowserBack
        {
            *key = Key::Escape;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, pressed: bool) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn back_becomes_escape_and_other_input_is_left_alone() {
        let mut events = vec![key(Key::BrowserBack, true), key(Key::BrowserBack, false), key(Key::Enter, true), Event::Text("a".into())];
        back_as_escape(&mut events);
        assert_eq!(events[0], key(Key::Escape, true));
        assert_eq!(events[1], key(Key::Escape, false));
        assert_eq!(events[2], key(Key::Enter, true));
        assert_eq!(events[3], Event::Text("a".into()));
    }
}
