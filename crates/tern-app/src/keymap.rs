//! Global keyboard handling (S0-T06 spike).
//!
//! FUTURE KEYMAP ENGINE (Sprint 2): this module is where it plugs in. Instead of the hard-coded
//! `Ctrl+K` check in [`message_for_key`], the engine will translate `(mode, key, modifiers)` into a
//! named `Action` using the user's keymap (defaults in `docs/KEYBINDINGS.md`, overrides in the
//! `settings` table), handle leader/chord sequences, and the command palette will list the same
//! actions. Mode-dependent routing (Normal / Insert / Palette) also happens there.

use iced::keyboard::key::Named;
use iced::keyboard::{self, Key, Modifiers};
use iced::{Event, Subscription, event};

use crate::app::Message;

/// Map a key press to an app message. Pure, so it can be unit-tested without a window.
pub fn message_for_key(key: &Key, modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Character(c) if modifiers == Modifiers::CTRL && c.eq_ignore_ascii_case("k") => {
            Some(Message::TogglePalette)
        }
        // Tab / Shift+Tab move between form fields (docs/KEYBINDINGS.md, "Forms").
        Key::Named(Named::Tab) if modifiers == Modifiers::SHIFT => Some(Message::FocusPrevious),
        Key::Named(Named::Tab) if modifiers.is_empty() => Some(Message::FocusNext),
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
        for k in ["k", "K"] {
            assert!(matches!(
                message_for_key(&ch(k), Modifiers::CTRL),
                Some(Message::TogglePalette)
            ));
        }
    }

    #[test]
    fn tab_moves_focus() {
        let tab = Key::Named(Named::Tab);
        assert!(matches!(
            message_for_key(&tab, Modifiers::empty()),
            Some(Message::FocusNext)
        ));
        assert!(matches!(
            message_for_key(&tab, Modifiers::SHIFT),
            Some(Message::FocusPrevious)
        ));
        assert!(message_for_key(&tab, Modifiers::CTRL).is_none());
    }

    #[test]
    fn other_chords_are_ignored() {
        assert!(message_for_key(&ch("k"), Modifiers::empty()).is_none());
        assert!(message_for_key(&ch("k"), Modifiers::CTRL | Modifiers::SHIFT).is_none());
        assert!(message_for_key(&ch("j"), Modifiers::CTRL).is_none());
    }
}
