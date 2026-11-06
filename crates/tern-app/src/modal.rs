//! Modal stack and confirm dialogs (S2-T06).

use iced::widget::{column, container, row, space, text};
use iced::{Center, Color, Element, Fill, Theme};

use crate::action::ModeMask;
use crate::app::Message;
use crate::cheatsheet;
use crate::keymap::Keymap;

/// What happens when a confirm dialog is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    DeleteSelected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    Confirm {
        title: String,
        body: String,
        then: Pending,
    },
    /// Keybinding cheat sheet, filtered to the mode it was opened from.
    CheatSheet { mode: ModeMask },
}

/// Last-in-first-out stack; only the top modal receives keys.
#[derive(Debug, Default)]
pub struct ModalStack(Vec<Modal>);

impl ModalStack {
    pub fn push(&mut self, modal: Modal) {
        self.0.push(modal);
    }

    pub fn pop(&mut self) -> Option<Modal> {
        self.0.pop()
    }

    pub fn top(&self) -> Option<&Modal> {
        self.0.last()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// Dim backdrop with the dialog centred on it.
pub fn view<'a>(modal: &'a Modal, keymap: &Keymap) -> Element<'a, Message> {
    match modal {
        Modal::Confirm { title, body, .. } => confirm_view(title, body),
        Modal::CheatSheet { mode } => cheatsheet::view(keymap, *mode),
    }
}

fn confirm_view<'a>(title: &'a str, body: &'a str) -> Element<'a, Message> {
    let dialog = container(
        column![
            text(title).size(18),
            text(body).size(14),
            space().height(8),
            row![
                text("[y] / Enter  Yes").size(13),
                text("[n] / Esc  No").size(13)
            ]
            .spacing(24),
        ]
        .spacing(8),
    )
    .padding(20)
    .max_width(420)
    .style(|theme: &Theme| {
        let mut s = container::bordered_box(theme);
        s.background = Some(theme.extended_palette().background.base.color.into());
        s
    });
    container(dialog)
        .center(Fill)
        .align_x(Center)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confirm(t: &str) -> Modal {
        Modal::Confirm {
            title: t.into(),
            body: String::new(),
            then: Pending::DeleteSelected,
        }
    }

    #[test]
    fn stack_is_lifo() {
        let mut s = ModalStack::default();
        assert!(s.is_empty());
        s.push(confirm("a"));
        s.push(confirm("b"));
        assert_eq!(s.top(), Some(&confirm("b")));
        assert_eq!(s.pop(), Some(confirm("b")));
        assert_eq!(s.pop(), Some(confirm("a")));
        assert_eq!(s.pop(), None);
    }
}
