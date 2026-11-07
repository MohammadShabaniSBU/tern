//! Re-authentication gate for sensitive actions (S3-T08).

use std::time::{Duration, Instant};

use iced::Element;
use iced::widget::{column, text, text_input};
use tern_core::{HostId, KeyId};

use crate::app::Message;
use crate::auth::Sensitive;
use crate::theme::tokens::*;

pub const REAUTH_ID: &str = "reauth-input";
pub const DEFAULT_MINUTES: u64 = 5;
pub const SETTING: &str = "reauth_minutes";

/// Tracks when the master password was last proven.
#[derive(Debug, Clone, Copy)]
pub struct Gate {
    verified_at: Option<Instant>,
    max_age: Duration,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            verified_at: None,
            max_age: Duration::from_secs(DEFAULT_MINUTES * 60),
        }
    }
}

impl Gate {
    /// Parse the stored setting (JSON number of minutes); `0` means "always ask".
    pub fn set_minutes_from_setting(&mut self, raw: Option<&str>) {
        if let Some(m) = raw.and_then(|r| r.trim().parse::<u64>().ok()) {
            self.max_age = Duration::from_secs(m.saturating_mul(60));
        }
    }

    pub fn mark_verified(&mut self, now: Instant) {
        self.verified_at = Some(now);
    }

    pub fn reset(&mut self) {
        self.verified_at = None;
    }

    pub fn needs_password(&self, now: Instant) -> bool {
        match self.verified_at {
            Some(t) => now.saturating_duration_since(t) >= self.max_age,
            None => true,
        }
    }
}

/// The sensitive operation waiting for the password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SensitiveOp {
    RevealPassword(HostId),
    ExportKey(KeyId),
    OpenIo(crate::io_form::IoKind),
}

#[derive(Debug, Clone)]
pub struct ReauthPrompt {
    pub then: SensitiveOp,
    pub password: Sensitive,
    pub error: Option<String>,
    pub busy: bool,
}

impl ReauthPrompt {
    pub fn new(then: SensitiveOp) -> Self {
        Self {
            then,
            password: Sensitive::default(),
            error: None,
            busy: false,
        }
    }

    fn what(&self) -> &'static str {
        match self.then {
            SensitiveOp::RevealPassword(_) => "reveal a stored password",
            SensitiveOp::ExportKey(_) => "export a private key",
            SensitiveOp::OpenIo(_) => "export all your data",
        }
    }
}

pub fn view(p: &ReauthPrompt) -> Element<'_, Message> {
    let mut body = column![
        text("Confirm master password").size(TEXT_TITLE),
        text(format!("Needed to {}.", p.what())).size(TEXT_BODY),
        text_input("Master password", p.password.as_str())
            .style(crate::theme::style::input(false))
            .id(REAUTH_ID)
            .secure(true)
            .on_input(|v| Message::ReauthPassword(Sensitive::new(v)))
            .on_submit(Message::ReauthSubmit)
            .padding(SPACE_4),
    ]
    .spacing(SPACE_8);
    if p.busy {
        body = body.push(text("Checking…").size(TEXT_BODY));
    }
    if let Some(e) = &p.error {
        body = body.push(
            text(e.as_str())
                .size(TEXT_BODY)
                .style(crate::theme::style::text_error),
        );
    }
    body = body.push(text("Enter confirm   Esc cancel").size(TEXT_SMALL));
    crate::theme::widgets::dialog(body, 420.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_until_verified_then_again_after_the_window() {
        let t0 = Instant::now();
        let mut g = Gate::default();
        assert!(g.needs_password(t0));
        g.mark_verified(t0);
        assert!(!g.needs_password(t0 + Duration::from_secs(299)));
        assert!(g.needs_password(t0 + Duration::from_secs(300)));
        g.reset();
        assert!(g.needs_password(t0));
    }

    #[test]
    fn window_is_configurable_and_zero_always_asks() {
        let t0 = Instant::now();
        let mut g = Gate::default();
        g.set_minutes_from_setting(Some("10"));
        g.mark_verified(t0);
        assert!(!g.needs_password(t0 + Duration::from_secs(599)));
        g.set_minutes_from_setting(Some("0"));
        assert!(g.needs_password(t0));
        // Garbage keeps the previous value.
        g.set_minutes_from_setting(Some("soon"));
        assert!(g.needs_password(t0));
    }
}
