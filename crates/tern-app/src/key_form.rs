//! Generate / import / paste SSH key form (S3-T04).

use iced::Element;
use iced::widget::{column, row, text, text_editor, text_input};
use tern_ssh::keys::KeyKind;

use crate::app::Message;
use crate::auth::Sensitive;
use crate::select::{self, SelectSpec};
use crate::theme::tokens::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    Generate,
    ImportFile,
    /// Paste a private key (and optionally its public key), like Termius.
    Add,
}

pub const LABEL_ID: &str = "kf-label";
pub const PUBLIC_ID: &str = "kf-public";
pub const PATH_ID: &str = "kf-path";

#[derive(Debug)]
pub struct KeyForm {
    pub mode: KeyMode,
    pub label: String,
    pub kind: KeyKind,
    pub path: String,
    /// Pasted private key text (multi-line). Emptied once the form is submitted or closed.
    pub private: text_editor::Content,
    /// Optional; derived from the private key when blank.
    pub public: String,
    pub passphrase: Sensitive,
    pub error: Option<String>,
    pub busy: bool,
}

impl KeyForm {
    pub fn new(mode: KeyMode) -> Self {
        Self {
            mode,
            label: String::new(),
            kind: KeyKind::Ed25519,
            path: String::new(),
            private: text_editor::Content::new(),
            public: String::new(),
            passphrase: Sensitive::default(),
            error: None,
            busy: false,
        }
    }

    pub fn set_private(&mut self, text: &str) {
        self.private = text_editor::Content::with_text(text);
    }

    pub fn first_field(&self) -> &'static str {
        match self.mode {
            KeyMode::ImportFile => PATH_ID,
            _ => LABEL_ID,
        }
    }

    fn title(&self) -> &'static str {
        match self.mode {
            KeyMode::Generate => "Generate SSH key",
            KeyMode::ImportFile => "Import SSH key from file",
            KeyMode::Add => "Add SSH key",
        }
    }

    /// Label to store: the typed one, else a sensible default.
    pub fn effective_label(&self) -> String {
        let typed = self.label.trim();
        if !typed.is_empty() {
            return typed.to_owned();
        }
        match self.mode {
            KeyMode::Generate => String::new(),
            KeyMode::ImportFile => std::path::Path::new(self.path.trim())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            KeyMode::Add => String::new(),
        }
    }
}

fn field<'a>(label: &'static str, input: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).size(TEXT_BODY).width(100), input]
        .spacing(SPACE_8)
        .align_y(iced::Center)
        .into()
}

pub fn view(form: &KeyForm) -> Element<'_, Message> {
    let mut body = column![text(form.title()).size(TEXT_TITLE)].spacing(SPACE_8);
    if form.mode == KeyMode::ImportFile {
        body = body.push(field(
            "File",
            text_input("~/.ssh/id_ed25519", &form.path)
                .style(crate::theme::style::input(false))
                .id(PATH_ID)
                .on_input(Message::KeyPathChanged)
                .on_submit(Message::KeySubmit)
                .padding(SPACE_4)
                .into(),
        ));
    }
    body = body.push(field(
        "Label",
        text_input(
            if form.mode == KeyMode::ImportFile {
                "(file name)"
            } else {
                ""
            },
            &form.label,
        )
        .style(crate::theme::style::input(false))
        .id(LABEL_ID)
        .on_input(Message::KeyLabelChanged)
        .on_submit(Message::KeySubmit)
        .padding(SPACE_4)
        .into(),
    ));
    if form.mode == KeyMode::Add {
        body = body.push(field(
            "Private key",
            text_editor(&form.private)
                .placeholder("-----BEGIN OPENSSH PRIVATE KEY-----")
                .on_action(Message::KeyPrivateEdit)
                .height(140)
                .size(TEXT_SMALL)
                .font(crate::app::MONO)
                .into(),
        ));
        body = body.push(field(
            "Public key",
            text_input("(optional, derived from the private key)", &form.public)
                .style(crate::theme::style::input(false))
                .id(PUBLIC_ID)
                .on_input(Message::KeyPublicChanged)
                .on_submit(Message::KeySubmit)
                .padding(SPACE_4)
                .into(),
        ));
    }
    if form.mode == KeyMode::Generate {
        body = body.push(field(
            "Type",
            select::view(SelectSpec {
                id: "kf-kind",
                labels: KeyKind::ALL.iter().map(|k| k.to_string()).collect(),
                current: KeyKind::ALL
                    .iter()
                    .position(|k| *k == form.kind)
                    .unwrap_or(0),
                picks: KeyKind::ALL
                    .iter()
                    .map(|k| Message::KeyKindPicked(*k))
                    .collect(),
            }),
        ));
    }
    body = body.push(field(
        "Passphrase",
        text_input(
            if form.mode == KeyMode::Generate {
                "(optional)"
            } else {
                "(if the key is encrypted)"
            },
            form.passphrase.as_str(),
        )
        .style(crate::theme::style::input(false))
        .secure(true)
        .on_input(|v| Message::KeyPassphraseChanged(Sensitive::new(v)))
        .on_submit(Message::KeySubmit)
        .padding(SPACE_4)
        .into(),
    ));
    if form.busy {
        body = body.push(text("Working… (RSA 4096 can take a while)").size(TEXT_BODY));
    }
    if let Some(e) = &form.error {
        body = body.push(
            text(e.as_str())
                .size(TEXT_BODY)
                .style(crate::theme::style::text_error),
        );
    }
    body = body.push(text("Ctrl+S save   Tab next field   Esc cancel").size(TEXT_SMALL));
    crate::theme::widgets::dialog(body, 520.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_labels() {
        let mut f = KeyForm::new(KeyMode::ImportFile);
        f.path = "/home/me/.ssh/id_work".into();
        assert_eq!(f.effective_label(), "id_work");
        f.label = " mine ".into();
        assert_eq!(f.effective_label(), "mine");
        assert_eq!(KeyForm::new(KeyMode::Add).effective_label(), "");
        assert_eq!(KeyForm::new(KeyMode::Generate).effective_label(), "");
    }
}
