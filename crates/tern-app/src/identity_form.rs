//! Identities section data + add/edit form (S3-T05).

use std::collections::HashSet;

use iced::widget::{checkbox, column, pick_list, row, text, text_input};
use iced::{Element, Fill};
use rand::RngExt;
use tern_core::{Identity, IdentityId, SshKeyMeta};

use crate::app::Message;
use crate::auth::Sensitive;
use crate::host_form::Choice;
use crate::list::Item;
use crate::theme::tokens::*;

pub const LABEL_ID: &str = "if-label";
const GENERATED_LEN: usize = 24;
const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#%^*-_=+";

/// Random password drawn from an unambiguous character set.
pub fn generate_password() -> String {
    let mut rng = rand::rng();
    (0..GENERATED_LEN)
        .map(|_| char::from(CHARSET[rng.random_range(0..CHARSET.len())]))
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct IdentitiesData {
    pub identities: Vec<Identity>,
    pub with_password: HashSet<IdentityId>,
}

impl IdentitiesData {
    pub fn identity(&self, id: &str) -> Option<&Identity> {
        self.identities.iter().find(|i| i.id.to_string() == id)
    }

    fn key_label<'a>(keys: &'a [SshKeyMeta], i: &Identity) -> Option<&'a str> {
        let id = i.ssh_key_id?;
        keys.iter().find(|k| k.id == id).map(|k| k.label.as_str())
    }

    pub fn items(&self, keys: &[SshKeyMeta]) -> Vec<Item> {
        self.identities
            .iter()
            .map(|i| {
                let mut meta = i.username.clone();
                if self.with_password.contains(&i.id) {
                    meta.push_str("  password");
                }
                if let Some(k) = Self::key_label(keys, i) {
                    meta.push_str("  key:");
                    meta.push_str(k);
                }
                Item::leaf(i.label.clone(), 0)
                    .with_id(i.id.to_string())
                    .with_meta(meta)
            })
            .collect()
    }

    /// Never includes the password itself, only whether one is stored.
    pub fn detail(&self, id: &str, keys: &[SshKeyMeta]) -> Option<String> {
        let i = self.identity(id)?;
        Some(format!(
            "{}\n\nUsername  {}\nPassword  {}\nKey       {}",
            i.label,
            i.username,
            if self.with_password.contains(&i.id) {
                "••••••••"
            } else {
                "(none)"
            },
            Self::key_label(keys, i).unwrap_or("(none)")
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdField {
    Label,
    Username,
}

#[derive(Debug, Clone)]
pub struct IdentityForm {
    editing: Option<Identity>,
    had_password: bool,
    pub label: String,
    pub username: String,
    pub password: Sensitive,
    pub reveal: bool,
    /// Editing only: remove the stored password.
    pub clear_password: bool,
    pub key: Choice,
    keys: Vec<Choice>,
    pub error: Option<String>,
}

/// What to persist.
pub struct IdentityDraft {
    pub identity: Identity,
    /// Insert: the password, if any. Update: `None` keeps, `Some(None)` clears, `Some(Some)` sets.
    pub password: Option<Option<secrecy::SecretString>>,
    pub is_edit: bool,
}

impl IdentityForm {
    pub fn new(existing: Option<(&Identity, bool)>, keys: &[SshKeyMeta]) -> Self {
        let choices: Vec<Choice> = std::iter::once(Choice::none())
            .chain(keys.iter().map(|k| Choice {
                id: Some(k.id.to_string()),
                label: format!("{} ({})", k.label, k.key_type),
            }))
            .collect();
        let key = existing
            .and_then(|(i, _)| i.ssh_key_id)
            .and_then(|id| {
                choices
                    .iter()
                    .find(|c| c.id.as_deref() == Some(&id.to_string()))
            })
            .cloned()
            .unwrap_or_else(Choice::none);
        Self {
            editing: existing.map(|(i, _)| i.clone()),
            had_password: existing.is_some_and(|(_, p)| p),
            label: existing.map(|(i, _)| i.label.clone()).unwrap_or_default(),
            username: existing
                .map(|(i, _)| i.username.clone())
                .unwrap_or_default(),
            password: Sensitive::default(),
            reveal: false,
            clear_password: false,
            key,
            keys: choices,
            error: None,
        }
    }

    pub fn is_edit(&self) -> bool {
        self.editing.is_some()
    }

    pub fn set_text(&mut self, f: IdField, v: String) {
        match f {
            IdField::Label => self.label = v,
            IdField::Username => self.username = v,
        }
        self.error = None;
    }

    pub fn generate(&mut self) {
        self.password = Sensitive::new(generate_password());
        self.reveal = true;
        self.clear_password = false;
        self.error = None;
    }

    pub fn build(&mut self) -> Result<IdentityDraft, String> {
        let key_id = self
            .key
            .id
            .as_deref()
            .map(|s| s.parse().map_err(|_| "invalid key".to_owned()))
            .transpose()?;
        let mut identity = self.editing.clone().unwrap_or_else(|| Identity {
            id: IdentityId::new(),
            label: String::new(),
            username: String::new(),
            ssh_key_id: None,
        });
        identity.label = self.label.trim().to_owned();
        identity.username = self.username.trim().to_owned();
        identity.ssh_key_id = key_id;
        identity.validate().map_err(|e| e.to_string())?;
        let new_password = (!self.password.is_empty()).then(|| self.password.clone().take_secret());
        let password = if self.is_edit() {
            match (new_password, self.clear_password && self.had_password) {
                (Some(p), _) => Some(Some(p)),
                (None, true) => Some(None),
                (None, false) => None,
            }
        } else {
            new_password.map(Some)
        };
        Ok(IdentityDraft {
            identity,
            password,
            is_edit: self.is_edit(),
        })
    }
}

pub fn view(form: &IdentityForm) -> Element<'_, Message> {
    let field = |label: &'static str, input: Element<'static, Message>| {
        row![text(label).size(TEXT_BODY).width(100), input]
            .spacing(SPACE_8)
            .align_y(iced::Center)
    };
    let hint = if form.is_edit() && form.had_password {
        "(unchanged)"
    } else {
        ""
    };
    let mut body = column![
        text(if form.is_edit() {
            "Edit identity"
        } else {
            "New identity"
        })
        .size(TEXT_TITLE),
        field(
            "Label",
            text_input("", &form.label)
                .style(crate::theme::style::input(false))
                .id(LABEL_ID)
                .on_input(|v| Message::IdentityEdit(IdField::Label, v))
                .on_submit(Message::IdentitySave)
                .padding(SPACE_4)
                .into()
        ),
        field(
            "Username",
            text_input("", &form.username)
                .style(crate::theme::style::input(false))
                .on_input(|v| Message::IdentityEdit(IdField::Username, v))
                .on_submit(Message::IdentitySave)
                .padding(SPACE_4)
                .into()
        ),
        field(
            "Password",
            text_input(hint, form.password.as_str())
                .style(crate::theme::style::input(false))
                .secure(!form.reveal)
                .on_input(|v| Message::IdentityPassword(Sensitive::new(v)))
                .on_submit(Message::IdentitySave)
                .padding(SPACE_4)
                .into()
        ),
        field(
            "Key",
            pick_list(
                form.keys.clone(),
                Some(form.key.clone()),
                Message::IdentityKey
            )
            .width(Fill)
            .into()
        ),
    ]
    .spacing(SPACE_8);
    if form.is_edit() && form.had_password {
        body = body.push(
            checkbox(form.clear_password)
                .label("Remove stored password")
                .on_toggle(Message::IdentityClearPassword),
        );
    }
    if let Some(e) = &form.error {
        body = body.push(
            text(e.as_str())
                .size(TEXT_BODY)
                .style(crate::theme::style::text_error),
        );
    }
    body = body.push(
        text("Ctrl+R reveal   Ctrl+G generate   Ctrl+Shift+C copy (clears in 30 s)   Ctrl+S save   Esc cancel")
            .size(TEXT_SMALL),
    );
    crate::theme::widgets::dialog(body, 620.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn ident() -> Identity {
        Identity {
            id: IdentityId::new(),
            label: "deploy".into(),
            username: "ci".into(),
            ssh_key_id: None,
        }
    }

    #[test]
    fn generated_passwords_are_long_and_distinct() {
        let (a, b) = (generate_password(), generate_password());
        assert_eq!(a.len(), GENERATED_LEN);
        assert_ne!(a, b);
    }

    #[test]
    fn new_identity_validates_and_carries_password() {
        let mut f = IdentityForm::new(None, &[]);
        assert!(f.build().is_err());
        f.set_text(IdField::Label, "x".into());
        f.set_text(IdField::Username, "root".into());
        assert!(f.build().unwrap().password.is_none());
        f.password = Sensitive::new("pw".into());
        let d = f.build().unwrap();
        assert_eq!(
            d.password.flatten().map(|p| p.expose_secret().to_owned()),
            Some("pw".into())
        );
    }

    #[test]
    fn edit_keeps_clears_or_sets_password() {
        let i = ident();
        let mut f = IdentityForm::new(Some((&i, true)), &[]);
        assert!(f.build().unwrap().password.is_none());
        f.clear_password = true;
        assert!(matches!(f.build().unwrap().password, Some(None)));
        f.password = Sensitive::new("new".into());
        assert!(matches!(f.build().unwrap().password, Some(Some(_))));
    }

    #[test]
    fn rows_and_detail_never_show_the_password() {
        let i = ident();
        let d = IdentitiesData {
            with_password: [i.id].into_iter().collect(),
            identities: vec![i.clone()],
        };
        assert_eq!(d.items(&[])[0].meta, "ci  password");
        assert!(d.detail(&i.id.to_string(), &[]).unwrap().contains("••••"));
    }
}
