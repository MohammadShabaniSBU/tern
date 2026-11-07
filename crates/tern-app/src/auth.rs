//! Create-vault / unlock screens (S1-T06) and their pure helpers.

use std::time::Duration;

use iced::widget::{button, column, container, text, text_input};
use iced::{Center, Element, Fill};
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::app::Message;
use crate::icons::{Icon, icon};
use crate::theme::tokens::*;

pub const PASSWORD_ID: &str = "auth-password";
pub const CONFIRM_ID: &str = "auth-confirm";

/// Text typed into a password field. Zeroized on drop; `Debug` is redacted so it can never leak
/// through logging of messages or state.
#[derive(Clone, Default)]
pub struct Sensitive(Zeroizing<String>);

impl Sensitive {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Move the text out into a [`SecretString`], leaving this field empty.
    pub fn take_secret(&mut self) -> SecretString {
        SecretString::from(std::mem::take(&mut *self.0))
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl std::fmt::Debug for Sensitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sensitive([REDACTED])")
    }
}

/// Why an unlock/create attempt failed. `Clone` so it can travel in a `Message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthFailure {
    WrongPassword,
    Other(String),
}

impl AuthFailure {
    pub fn from_store(error: &tern_store::Error) -> Self {
        match error {
            tern_store::Error::Vault(tern_vault::Error::WrongPassword) => Self::WrongPassword,
            other => Self::Other(other.user_message()),
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::WrongPassword => "Wrong password.".into(),
            Self::Other(e) => format!("Error: {e}"),
        }
    }
}

/// Rough password strength for the hint on the create screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strength {
    Empty,
    Weak,
    Fair,
    Strong,
}

impl Strength {
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::Weak => "Strength: weak",
            Self::Fair => "Strength: fair",
            Self::Strong => "Strength: strong",
        }
    }
}

/// A deliberately simple length/variety heuristic (no dictionary check). Prefer long passphrases.
pub fn strength(password: &str) -> Strength {
    let len = password.chars().count();
    if len == 0 {
        return Strength::Empty;
    }
    let classes = [
        password.chars().any(|c| c.is_lowercase()),
        password.chars().any(|c| c.is_uppercase()),
        password.chars().any(|c| c.is_ascii_digit()),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .into_iter()
    .filter(|&b| b)
    .count();
    match (len, classes) {
        (l, _) if l >= 20 => Strength::Strong,
        (l, c) if l >= 12 && c >= 3 => Strength::Strong,
        (l, c) if l >= 12 || (l >= 8 && c >= 3) => Strength::Fair,
        _ => Strength::Weak,
    }
}

/// Pause after the `n`th consecutive failed unlock (n >= 1): 0.5 s per failure, capped at 3 s.
pub fn retry_delay(failed_attempts: u32) -> Duration {
    Duration::from_millis(500 * u64::from(failed_attempts.min(6)))
}

/// State shared by both screens.
#[derive(Debug, Default)]
pub struct AuthForm {
    pub password: Sensitive,
    pub confirm: Sensitive,
    pub error: Option<String>,
    pub busy: bool,
    pub failed_attempts: u32,
    pub spinner_frame: usize,
}

const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];

impl AuthForm {
    fn spinner(&self, label: &str) -> String {
        format!("{} {label}", SPINNER[self.spinner_frame % SPINNER.len()])
    }
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        container(content)
            .padding(SPACE_32)
            .width(400)
            .style(crate::theme::style::modal_card),
    )
    .center_x(Fill)
    .center_y(Fill)
    .style(crate::theme::style::base)
    .into()
}

fn status<'a>(form: &AuthForm, busy_label: &str) -> Element<'a, Message> {
    if form.busy {
        text(form.spinner(busy_label)).into()
    } else if let Some(error) = &form.error {
        text(error.clone())
            .style(crate::theme::style::text_error)
            .into()
    } else {
        text("").into()
    }
}

pub fn unlock_view(form: &AuthForm) -> Element<'_, Message> {
    let mut input = text_input("Master password", form.password.as_str())
        .style(crate::theme::style::input(false))
        .id(PASSWORD_ID)
        .secure(true)
        .size(TEXT_LABEL)
        .padding([SPACE_8, SPACE_10]);
    let mut submit = button(container(text("Unlock").align_x(Center).width(Fill)).center_y(Fill))
        .height(INPUT_H_LG)
        .style(crate::theme::style::button(
            crate::theme::style::ButtonKind::Primary,
        ));
    if !form.busy {
        input = input
            .on_input(|s| Message::PasswordChanged(Sensitive::new(s)))
            .on_submit(Message::SubmitPassword);
        submit = submit.on_press(Message::SubmitPassword);
    }
    card(
        column![
            container(icon(Icon::Lock, 22.0, |p| p.accent))
                .padding(SPACE_12)
                .style(|t: &iced::Theme| {
                    let p = crate::theme::palette(t);
                    container::Style {
                        background: Some(p.bg_raised.into()),
                        border: iced::Border::default()
                            .rounded(RADIUS_MD)
                            .width(1.0)
                            .color(p.border_strong),
                        ..Default::default()
                    }
                }),
            text("Unlock tern")
                .size(TEXT_DISPLAY)
                .font(crate::theme::UI_SEMIBOLD),
            text("Enter your master password to unlock.")
                .size(TEXT_BODY)
                .style(crate::theme::style::text_secondary),
            input,
            submit,
            status(form, "Unlocking…"),
        ]
        .spacing(SPACE_12),
    )
}

pub fn create_view(form: &AuthForm) -> Element<'_, Message> {
    let mut password = text_input("Master password", form.password.as_str())
        .style(crate::theme::style::input(false))
        .id(PASSWORD_ID)
        .secure(true)
        .size(TEXT_LABEL)
        .padding([SPACE_8, SPACE_10]);
    let mut confirm = text_input("Confirm master password", form.confirm.as_str())
        .style(crate::theme::style::input(false))
        .id(CONFIRM_ID)
        .secure(true)
        .size(TEXT_LABEL)
        .padding([SPACE_8, SPACE_10]);
    let mut submit =
        button(container(text("Create vault").align_x(Center).width(Fill)).center_y(Fill))
            .height(INPUT_H_LG)
            .style(crate::theme::style::button(
                crate::theme::style::ButtonKind::Primary,
            ));
    if !form.busy {
        password = password
            .on_input(|s| Message::PasswordChanged(Sensitive::new(s)))
            .on_submit(Message::SubmitPassword);
        confirm = confirm
            .on_input(|s| Message::ConfirmChanged(Sensitive::new(s)))
            .on_submit(Message::SubmitConfirm);
        submit = submit.on_press(Message::SubmitConfirm);
    }
    card(
        column![
            text("Create your vault")
                .size(TEXT_DISPLAY)
                .font(crate::theme::UI_SEMIBOLD),
            text("Create a master password. It protects all stored passwords and keys."),
            password,
            text(strength(form.password.as_str()).label()).size(TEXT_LABEL),
            confirm,
            submit,
            status(form, "Creating vault…"),
        ]
        .spacing(SPACE_12),
    )
}

/// One-time warning after creating the vault. `Enter` acknowledges (see `keymap`).
pub fn recovery_view<'a>() -> Element<'a, Message> {
    card(
        column![
            text("Your vault is ready")
                .size(TEXT_DISPLAY)
                .font(crate::theme::UI_SEMIBOLD),
            text("There is no password recovery.")
                .style(crate::theme::style::text_error)
                .size(TEXT_TITLE),
            text(
                "Your master password is never stored and cannot be reset. If you forget it, \
                 every saved password and key is permanently lost."
            ),
            text(
                "Once you have added hosts, export an encrypted backup and keep it somewhere safe."
            ),
            button(text("I understand — Enter").align_x(Center).width(Fill))
                .padding(SPACE_8)
                .on_press(Message::Acknowledge),
        ]
        .spacing(SPACE_12),
    )
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn strength_levels() {
        assert_eq!(strength(""), Strength::Empty);
        assert_eq!(strength("abc"), Strength::Weak);
        assert_eq!(strength("password"), Strength::Weak);
        assert_eq!(strength("Passw0rd!"), Strength::Fair);
        assert_eq!(strength("correcthorsebatt"), Strength::Fair);
        assert_eq!(strength("Tr0ub4dor&3xyz!"), Strength::Strong);
        assert_eq!(strength("correct horse battery staple"), Strength::Strong);
    }

    #[test]
    fn retry_delay_grows_then_caps() {
        assert_eq!(retry_delay(0), Duration::ZERO);
        assert_eq!(retry_delay(1), Duration::from_millis(500));
        assert_eq!(retry_delay(2), Duration::from_millis(1000));
        assert_eq!(retry_delay(6), Duration::from_secs(3));
        assert_eq!(retry_delay(100), Duration::from_secs(3));
    }

    #[test]
    fn sensitive_debug_is_redacted() {
        let s = Sensitive::new("hunter2".into());
        assert!(!format!("{s:?}").contains("hunter2"));
        let msg = Message::PasswordChanged(s);
        assert!(!format!("{msg:?}").contains("hunter2"));
    }

    #[test]
    fn take_secret_empties_the_field() {
        let mut s = Sensitive::new("hunter2".into());
        let secret = s.take_secret();
        assert_eq!(secret.expose_secret(), "hunter2");
        assert!(s.is_empty());
    }

    #[test]
    fn failure_mapping() {
        let wrong = tern_store::Error::Vault(tern_vault::Error::WrongPassword);
        assert_eq!(AuthFailure::from_store(&wrong), AuthFailure::WrongPassword);
        assert!(matches!(
            AuthFailure::from_store(&tern_store::Error::NoVault),
            AuthFailure::Other(_)
        ));
    }
}
