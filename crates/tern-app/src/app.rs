//! The iced application: state, messages, update and view.

use iced::widget::{column, container, row, space, text};
use iced::{Element, Fill, Subscription, Theme};

use crate::keymap;

/// Which part of the app owns the keyboard. See `docs/KEYBINDINGS.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    // Insert is added when text inputs / terminals land (Sprint 2).
    Palette,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Palette => "PALETTE",
        }
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub mode: Mode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    TogglePalette,
}

impl State {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::TogglePalette => {
                self.mode = match self.mode {
                    Mode::Palette => Mode::Normal,
                    _ => Mode::Palette,
                };
                tracing::debug!(mode = self.mode.label(), "palette toggled");
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let sidebar = container(
            column![
                text("tern").size(20),
                space().height(12),
                text("Hosts"),
                text("Keys"),
                text("Identities"),
                text("Tunnels"),
            ]
            .spacing(6),
        )
        .padding(12)
        .width(200)
        .height(Fill)
        .style(container::bordered_box);

        let main_text = match self.mode {
            Mode::Palette => "Command palette (placeholder)",
            Mode::Normal => "Main area",
        };
        let main_area = container(text(main_text).size(18))
            .padding(12)
            .width(Fill)
            .height(Fill);

        let status_bar = container(text(self.mode.label()).size(14))
            .padding([4, 12])
            .width(Fill)
            .style(container::bordered_box);

        column![row![sidebar, main_area].height(Fill), status_bar].into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        keymap::subscription()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_in_normal_mode() {
        assert_eq!(State::default().mode, Mode::Normal);
        assert_eq!(State::default().mode.label(), "NORMAL");
    }

    #[test]
    fn toggle_palette_round_trips() {
        let mut state = State::default();
        state.update(Message::TogglePalette);
        assert_eq!(state.mode, Mode::Palette);
        state.update(Message::TogglePalette);
        assert_eq!(state.mode, Mode::Normal);
    }
}
