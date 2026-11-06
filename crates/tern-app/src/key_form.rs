//! Generate / import / paste SSH key form (S3-T04).

use iced::widget::{column, container, pick_list, row, text, text_editor, text_input};
use iced::{Color, Element, Fill, Theme};
use tern_ssh::keys::KeyKind;

use crate::app::Message;
use crate::auth::Sensitive;

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
    row![text(label).size(13).width(100), input]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

pub fn view(form: &KeyForm) -> Element<'_, Message> {
    let mut body = column![text(form.title()).size(18)].spacing(8);
    if form.mode == KeyMode::ImportFile {
        body = body.push(field(
            "File",
            text_input("~/.ssh/id_ed25519", &form.path)
                .id(PATH_ID)
                .on_input(Message::KeyPathChanged)
                .on_submit(Message::KeySubmit)
                .padding(4)
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
        .id(LABEL_ID)
        .on_input(Message::KeyLabelChanged)
        .on_submit(Message::KeySubmit)
        .padding(4)
        .into(),
    ));
    if form.mode == KeyMode::Add {
        body = body.push(field(
            "Private key",
            text_editor(&form.private)
                .placeholder("-----BEGIN OPENSSH PRIVATE KEY-----")
                .on_action(Message::KeyPrivateEdit)
                .height(140)
                .size(12)
                .font(crate::app::MONO)
                .into(),
        ));
        body = body.push(field(
            "Public key",
            text_input("(optional, derived from the private key)", &form.public)
                .id(PUBLIC_ID)
                .on_input(Message::KeyPublicChanged)
                .on_submit(Message::KeySubmit)
                .padding(4)
                .into(),
        ));
    }
    if form.mode == KeyMode::Generate {
        body = body.push(field(
            "Type",
            pick_list(KeyKind::ALL, Some(form.kind), Message::KeyKindPicked)
                .width(Fill)
                .into(),
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
        .secure(true)
        .on_input(|v| Message::KeyPassphraseChanged(Sensitive::new(v)))
        .on_submit(Message::KeySubmit)
        .padding(4)
        .into(),
    ));
    if form.busy {
        body = body.push(text("Working… (RSA 4096 can take a while)").size(13));
    }
    if let Some(e) = &form.error {
        body = body.push(text(e.as_str()).size(13).style(text::danger));
    }
    body = body.push(text("Ctrl+S save   Tab next field   Esc cancel").size(12));
    let dialog = container(body)
        .padding(20)
        .max_width(520)
        .style(|theme: &Theme| {
            let mut s = container::bordered_box(theme);
            s.background = Some(theme.extended_palette().background.base.color.into());
            s
        });
    container(dialog)
        .center(Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
            ..container::Style::default()
        })
        .into()
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
