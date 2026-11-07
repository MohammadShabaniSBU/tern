//! Modal stack and confirm dialogs (S2-T06).

use iced::Element;
use iced::widget::{column, row, space, text};

use crate::action::ModeMask;
use crate::app::Message;
use crate::cheatsheet;
use crate::keymap::Keymap;
use crate::theme::tokens::*;

/// What happens when a confirm dialog is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    DeleteSelected,
    /// Close the tab with this session id (its session is still live).
    CloseTab(crate::sessions::SessionId),
    /// Paste this multi-line text into the focused terminal.
    Paste(String),
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
    /// Server host key needs a decision (S4-T03).
    HostKey {
        host: String,
        port: u16,
        key_type: String,
        fingerprint: String,
        /// Previously trusted fingerprint when the key changed.
        changed_from: Option<String>,
    },
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
        Modal::HostKey {
            host,
            port,
            key_type,
            fingerprint,
            changed_from,
        } => host_key_view(host, *port, key_type, fingerprint, changed_from.as_deref()),
    }
}

fn host_key_view<'a>(
    host: &'a str,
    port: u16,
    key_type: &'a str,
    fingerprint: &'a str,
    changed_from: Option<&'a str>,
) -> Element<'a, Message> {
    let mut body = column![].spacing(SPACE_8);
    if let Some(old) = changed_from {
        body = body
            .push(
                text("WARNING: HOST KEY HAS CHANGED")
                    .size(TEXT_DISPLAY)
                    .style(crate::theme::style::text_error),
            )
            .push(
                text(format!(
                    "The key for {host}:{port} is different from the one you trusted. \
                     Someone may be intercepting the connection, or the server was reinstalled."
                ))
                .size(TEXT_LABEL),
            )
            .push(text(format!("Old: {old}")).size(TEXT_BODY))
            .push(text(format!("New: {fingerprint}  ({key_type})")).size(TEXT_BODY))
            .push(space().height(8))
            .push(text("[n] / Esc  Abort (recommended)").size(TEXT_BODY))
            .push(text("[Shift+R]  Replace the stored key and connect").size(TEXT_BODY));
    } else {
        body = body
            .push(text("Unknown host").size(TEXT_TITLE))
            .push(text(format!("First connection to {host}:{port}.")).size(TEXT_LABEL))
            .push(text(format!("{key_type}  {fingerprint}")).size(TEXT_BODY))
            .push(space().height(8))
            .push(text("[y] / Enter  Trust and save").size(TEXT_BODY))
            .push(text("[n] / Esc  Abort").size(TEXT_BODY));
    }
    crate::theme::widgets::dialog(body, 560.0)
}

fn confirm_view<'a>(title: &'a str, body: &'a str) -> Element<'a, Message> {
    crate::theme::widgets::dialog(
        column![
            text(title)
                .size(TEXT_HEADING)
                .font(crate::theme::UI_SEMIBOLD),
            text(body)
                .size(TEXT_BODY)
                .style(crate::theme::style::text_secondary),
            space().height(SPACE_8),
            row![
                text("y / Enter  Yes")
                    .size(TEXT_SMALL)
                    .font(crate::theme::MONO),
                text("n / Esc  No")
                    .size(TEXT_SMALL)
                    .font(crate::theme::MONO),
            ]
            .spacing(SPACE_24),
        ]
        .spacing(SPACE_8),
        crate::theme::tokens::CONFIRM_W,
    )
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
