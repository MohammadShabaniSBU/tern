//! Global keyboard handling (S0-T06 spike).
//!
//! FUTURE KEYMAP ENGINE (Sprint 2): this module is where it plugs in. Instead of the hard-coded
//! `Ctrl+K` check in [`message_for_key`], the engine will translate `(mode, key, modifiers)` into a
//! named `Action` using the user's keymap (defaults in `docs/KEYBINDINGS.md`, overrides in the
//! `settings` table), handle leader/chord sequences, and the command palette will list the same
//! actions. Mode-dependent routing (Normal / Insert / Palette) also happens there.

use iced::keyboard::{self, Key, Modifiers};
use iced::{Event, Subscription, event};

use crate::app::Message;

/// Map a key press to an app message. Pure, so it can be unit-tested without a window.
pub fn message_for_key(key: &Key, modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Character(c) if modifiers == Modifiers::CTRL && c.eq_ignore_ascii_case("k") => {
            Some(Message::TogglePalette)
        }
        _ => None,
    }
}

/// Subscription feeding every key press to [`message_for_key`].
///
/// The event status is ignored on purpose: global chords must work even when a widget has focus.
pub fn subscription() -> Subscription<Message> {
    event::listen_with(|event, _status, _window| match event {
        Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            message_for_key(&key, modifiers)
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn ctrl_k_toggles_palette() {
        assert_eq!(
            message_for_key(&ch("k"), Modifiers::CTRL),
            Some(Message::TogglePalette)
        );
        assert_eq!(
            message_for_key(&ch("K"), Modifiers::CTRL),
            Some(Message::TogglePalette)
        );
    }

    #[test]
    fn other_chords_are_ignored() {
        assert_eq!(message_for_key(&ch("k"), Modifiers::empty()), None);
        assert_eq!(
            message_for_key(&ch("k"), Modifiers::CTRL | Modifiers::SHIFT),
            None
        );
        assert_eq!(message_for_key(&ch("j"), Modifiers::CTRL), None);
    }
}
