//! The iced application: state, messages, update and view.

use std::time::Instant;

use iced::widget::{column, container, row, space, text};
use iced::{Element, Fill, Subscription, Task, Theme};
use tern_store::Store;

use crate::action::ModeMask;
use crate::action::{Action, Section};
use crate::auth::{self, AuthFailure, AuthForm, CONFIRM_ID, PASSWORD_ID, Sensitive, retry_delay};
use crate::keymap::{self, Chord, Context, Keymap, Matcher, Outcome};

/// Which part of the app owns the keyboard. See `docs/KEYBINDINGS.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    // Insert is added when text inputs / terminals land (Sprint 2).
    Palette,
}

impl Mode {
    /// The single `ModeMask` bit for this mode.
    pub fn mask(self) -> ModeMask {
        match self {
            Mode::Normal => ModeMask::NORMAL,
            Mode::Palette => ModeMask::PALETTE,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Palette => "PALETTE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Checking whether a vault exists.
    Loading,
    /// First launch: choose a master password.
    Create,
    /// A vault exists: ask for the master password.
    Unlock,
    /// One-time warning shown right after the vault is created.
    Recovery,
    /// Unlocked.
    Main,
}

#[derive(Debug)]
pub struct State {
    store: Store,
    pub screen: Screen,
    pub mode: Mode,
    pub section: Section,
    form: AuthForm,
    keymap: Keymap,
    matcher: Matcher,
    host_count: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum Message {
    VaultChecked(Result<bool, String>),
    PasswordChanged(Sensitive),
    ConfirmChanged(Sensitive),
    /// Enter in the (first) password field.
    SubmitPassword,
    /// Enter in the confirm field, or the create button.
    SubmitConfirm,
    AuthDone(Result<(), AuthFailure>),
    HostsLoaded(Result<usize, String>),
    SpinnerTick,
    /// Enter pressed anywhere; only meaningful on the recovery warning.
    Acknowledge,
    FocusNext,
    FocusPrevious,
    /// Run an action; the single entry point shared by keymap and palette.
    #[allow(dead_code)] // constructed by the command palette (S2-T04)
    Dispatch(Action),
    /// A key press on the main screen, routed through the keymap engine.
    Key(Chord),
    /// `(leader, keymap overrides)` JSON from the settings table.
    KeymapLoaded(Result<(Option<String>, Option<String>), String>),
    Locked(Result<(), String>),
}

impl State {
    pub fn new(store: Store) -> (Self, Task<Message>) {
        let check = {
            let store = store.clone();
            Task::perform(async move { store.vault_exists().await }, |r| {
                Message::VaultChecked(r.map_err(|e| e.to_string()))
            })
        };
        let state = Self {
            store,
            screen: Screen::Loading,
            mode: Mode::Normal,
            section: Section::default(),
            form: AuthForm::default(),
            keymap: Keymap::defaults(),
            matcher: Matcher::default(),
            host_count: None,
        };
        (state, check)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::VaultChecked(Ok(exists)) => {
                self.screen = if exists {
                    Screen::Unlock
                } else {
                    Screen::Create
                };
                iced::widget::operation::focus(PASSWORD_ID)
            }
            Message::VaultChecked(Err(e)) => {
                tracing::error!(error = %e, "could not read vault state");
                self.form.error = Some(format!("Could not open the database: {e}"));
                Task::none()
            }
            Message::PasswordChanged(value) => {
                self.form.password = value;
                self.form.error = None;
                Task::none()
            }
            Message::ConfirmChanged(value) => {
                self.form.confirm = value;
                self.form.error = None;
                Task::none()
            }
            Message::SubmitPassword => match self.screen {
                Screen::Unlock => self.submit_unlock(),
                // Enter in the first field moves on to the confirmation.
                Screen::Create => iced::widget::operation::focus(CONFIRM_ID),
                _ => Task::none(),
            },
            Message::SubmitConfirm => match self.screen {
                Screen::Create => self.submit_create(),
                _ => Task::none(),
            },
            Message::AuthDone(Ok(())) => match self.screen {
                // A freshly created vault shows the no-recovery warning exactly once.
                Screen::Create => {
                    self.form = AuthForm::default();
                    self.screen = Screen::Recovery;
                    Task::none()
                }
                _ => self.enter_main(),
            },
            Message::Acknowledge => match self.screen {
                Screen::Recovery => self.enter_main(),
                _ => Task::none(),
            },
            Message::AuthDone(Err(failure)) => {
                self.form.busy = false;
                self.form.password.clear();
                self.form.confirm.clear();
                self.form.error = Some(failure.message());
                if failure == AuthFailure::WrongPassword {
                    self.form.failed_attempts += 1;
                }
                iced::widget::operation::focus(PASSWORD_ID)
            }
            Message::HostsLoaded(result) => {
                self.host_count = result.ok();
                Task::none()
            }
            Message::SpinnerTick => {
                self.form.spinner_frame = self.form.spinner_frame.wrapping_add(1);
                Task::none()
            }
            Message::FocusNext => iced::widget::operation::focus_next(),
            Message::FocusPrevious => iced::widget::operation::focus_previous(),
            Message::Dispatch(action) => self.dispatch(action),
            Message::Key(chord) => {
                if self.screen != Screen::Main {
                    return Task::none();
                }
                let ctx = Context {
                    mode: self.mode.mask(),
                    terminal: false,
                };
                match self.matcher.feed(&self.keymap, ctx, chord, Instant::now()) {
                    Outcome::Action(action) => self.dispatch(action),
                    Outcome::Pending | Outcome::Unhandled => Task::none(),
                }
            }
            Message::KeymapLoaded(result) => {
                match result {
                    Ok((leader, overrides)) => {
                        self.keymap = Keymap::build(leader.as_deref(), overrides.as_deref());
                        self.matcher.reset();
                    }
                    Err(e) => tracing::error!(error = %e, "could not load keymap settings"),
                }
                Task::none()
            }
            Message::Locked(result) => {
                if let Err(e) = result {
                    tracing::error!(error = %e, "lock failed");
                }
                // Even if the store reported an error, leave the main screen.
                self.screen = Screen::Unlock;
                self.mode = Mode::Normal;
                self.host_count = None;
                iced::widget::operation::focus(PASSWORD_ID)
            }
        }
    }

    /// Execute an action. Both the keymap and the command palette call this.
    pub fn dispatch(&mut self, action: Action) -> Task<Message> {
        // Everything except the auth screens' own handling needs the unlocked app.
        if self.screen != Screen::Main {
            return Task::none();
        }
        tracing::debug!(action = action.meta().id, "dispatch");
        if let Some(section) = action.section() {
            self.section = section;
            return Task::none();
        }
        match action {
            Action::OpenPalette => {
                self.mode = match self.mode {
                    Mode::Palette => Mode::Normal,
                    _ => Mode::Palette,
                };
                Task::none()
            }
            Action::LockVault => {
                let store = self.store.clone();
                Task::perform(async move { store.lock().await }, |r| {
                    Message::Locked(r.map_err(|e| e.to_string()))
                })
            }
            // Registered for the keymap/palette; implemented by later tasks and sprints.
            _ => Task::none(),
        }
    }

    fn submit_unlock(&mut self) -> Task<Message> {
        if self.form.busy {
            return Task::none();
        }
        if self.form.password.is_empty() {
            self.form.error = Some("Enter your master password.".into());
            return Task::none();
        }
        self.form.busy = true;
        self.form.error = None;
        let password = self.form.password.take_secret();
        let store = self.store.clone();
        // One more failure than we have recorded: the delay applies if this attempt fails.
        let delay = retry_delay(self.form.failed_attempts + 1);
        Task::perform(
            async move {
                // Argon2 runs on the store's worker thread, not the UI thread.
                let result = store.unlock(password).await;
                match result {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        let failure = AuthFailure::from_store(&e);
                        if failure == AuthFailure::WrongPassword {
                            tokio::time::sleep(delay).await;
                        }
                        Err(failure)
                    }
                }
            },
            Message::AuthDone,
        )
    }

    fn submit_create(&mut self) -> Task<Message> {
        if self.form.busy {
            return Task::none();
        }
        if self.form.password.is_empty() {
            self.form.error = Some("Choose a master password.".into());
            return iced::widget::operation::focus(PASSWORD_ID);
        }
        if self.form.password.as_str() != self.form.confirm.as_str() {
            self.form.error = Some("Passwords do not match.".into());
            self.form.confirm.clear();
            return iced::widget::operation::focus(CONFIRM_ID);
        }
        self.form.busy = true;
        self.form.error = None;
        let password = self.form.password.take_secret();
        self.form.confirm.clear();
        let store = self.store.clone();
        Task::perform(
            async move {
                store
                    .create_vault(password)
                    .await
                    .map_err(|e| AuthFailure::from_store(&e))
            },
            Message::AuthDone,
        )
    }

    fn enter_main(&mut self) -> Task<Message> {
        self.form = AuthForm::default();
        self.screen = Screen::Main;
        self.mode = Mode::Normal;
        let hosts = {
            let store = self.store.clone();
            Task::perform(
                async move { store.list_hosts().await.map(|h| h.len()) },
                |r| Message::HostsLoaded(r.map_err(|e| e.to_string())),
            )
        };
        let keymap = {
            let store = self.store.clone();
            Task::perform(
                async move {
                    store
                        .query(|db| {
                            let s = db.settings();
                            Ok((s.get("leader")?, s.get("keymap")?))
                        })
                        .await
                },
                |r| Message::KeymapLoaded(r.map_err(|e| e.to_string())),
            )
        };
        Task::batch([hosts, keymap])
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self.screen {
            Screen::Loading => match &self.form.error {
                Some(e) => container(text(e.clone()).style(text::danger))
                    .center(Fill)
                    .into(),
                None => container(text("Loading…")).center(Fill).into(),
            },
            Screen::Create => auth::create_view(&self.form),
            Screen::Unlock => auth::unlock_view(&self.form),
            Screen::Recovery => auth::recovery_view(),
            Screen::Main => self.main_view(),
        }
    }

    fn main_view(&self) -> Element<'_, Message> {
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
            Mode::Palette => "Command palette (placeholder)".to_owned(),
            Mode::Normal => match self.host_count {
                Some(n) => format!("Vault unlocked — {n} host(s) stored"),
                None => "Vault unlocked".to_owned(),
            },
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
        let mut subs = vec![match self.screen {
            Screen::Main => keymap::main_subscription(),
            _ => keymap::auth_subscription(),
        }];
        if self.form.busy {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(120))
                    .map(|_| Message::SpinnerTick),
            );
        }
        Subscription::batch(subs)
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

#[cfg(test)]
mod tests {
    use tern_vault::KdfParams;

    use super::*;

    fn state() -> State {
        let store = Store::open_in_memory().expect("in-memory store");
        State::new(store).0
    }

    fn main_state() -> State {
        let mut s = state();
        s.screen = Screen::Main;
        s
    }

    #[test]
    fn starts_loading_in_normal_mode() {
        let s = state();
        assert_eq!(s.screen, Screen::Loading);
        assert_eq!(s.mode.label(), "NORMAL");
    }

    #[test]
    fn vault_check_picks_the_screen() {
        let mut s = state();
        let _ = s.update(Message::VaultChecked(Ok(false)));
        assert_eq!(s.screen, Screen::Create);
        let _ = s.update(Message::VaultChecked(Ok(true)));
        assert_eq!(s.screen, Screen::Unlock);
    }

    #[test]
    fn palette_toggle_only_when_unlocked() {
        let mut s = state();
        let _ = s.update(Message::Dispatch(Action::OpenPalette));
        assert_eq!(s.mode, Mode::Normal);

        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::OpenPalette));
        assert_eq!(s.mode, Mode::Palette);
        let _ = s.update(Message::Dispatch(Action::OpenPalette));
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn dispatch_switches_sections_only_when_unlocked() {
        let mut s = state();
        let _ = s.update(Message::Dispatch(Action::GoKeys));
        assert_eq!(s.section, Section::Hosts);

        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::GoTunnels));
        assert_eq!(s.section, Section::Tunnels);
    }

    #[test]
    fn locked_returns_to_unlock_screen() {
        let mut s = main_state();
        let _ = s.update(Message::Locked(Ok(())));
        assert_eq!(s.screen, Screen::Unlock);
    }

    #[test]
    fn create_validates_before_doing_work() {
        let mut s = state();
        s.screen = Screen::Create;

        let _ = s.update(Message::SubmitConfirm);
        assert!(s.form.error.is_some() && !s.form.busy, "empty password");

        let _ = s.update(Message::PasswordChanged(Sensitive::new("abc".into())));
        let _ = s.update(Message::ConfirmChanged(Sensitive::new("abd".into())));
        let _ = s.update(Message::SubmitConfirm);
        assert_eq!(s.form.error.as_deref(), Some("Passwords do not match."));
        assert!(!s.form.busy);
        assert!(s.form.confirm.is_empty());
    }

    #[test]
    fn matching_passwords_start_creation_and_clear_the_fields() {
        let mut s = state();
        s.screen = Screen::Create;
        let _ = s.update(Message::PasswordChanged(Sensitive::new("abc".into())));
        let _ = s.update(Message::ConfirmChanged(Sensitive::new("abc".into())));
        let _ = s.update(Message::SubmitConfirm);
        assert!(s.form.busy);
        assert!(s.form.password.is_empty() && s.form.confirm.is_empty());
        // A second submit while busy is ignored.
        let _ = s.update(Message::SubmitConfirm);
        assert!(s.form.busy);
    }

    #[test]
    fn creating_a_vault_shows_the_recovery_warning_once() {
        let mut s = state();
        s.screen = Screen::Create;
        let _ = s.update(Message::AuthDone(Ok(())));
        assert_eq!(s.screen, Screen::Recovery);

        // Only Enter (Acknowledge) leaves it.
        let _ = s.update(Message::Dispatch(Action::OpenPalette));
        assert_eq!(s.screen, Screen::Recovery);
        let _ = s.update(Message::Acknowledge);
        assert_eq!(s.screen, Screen::Main);

        // Later unlocks go straight to the main screen.
        s.screen = Screen::Unlock;
        let _ = s.update(Message::AuthDone(Ok(())));
        assert_eq!(s.screen, Screen::Main);
    }

    #[test]
    fn acknowledge_is_ignored_elsewhere() {
        for screen in [Screen::Create, Screen::Unlock, Screen::Main] {
            let mut s = state();
            s.screen = screen;
            let _ = s.update(Message::Acknowledge);
            assert_eq!(s.screen, screen);
        }
    }

    #[test]
    fn unlock_requires_a_password() {
        let mut s = state();
        s.screen = Screen::Unlock;
        let _ = s.update(Message::SubmitPassword);
        assert!(s.form.error.is_some() && !s.form.busy);
    }

    #[test]
    fn failed_unlock_clears_password_and_counts_attempts() {
        let mut s = state();
        s.screen = Screen::Unlock;
        s.form.busy = true;
        s.form.password = Sensitive::new("leftover".into());
        let _ = s.update(Message::AuthDone(Err(AuthFailure::WrongPassword)));
        assert!(!s.form.busy && s.form.password.is_empty());
        assert_eq!(s.form.failed_attempts, 1);
        assert_eq!(s.form.error.as_deref(), Some("Wrong password."));
        assert_eq!(s.screen, Screen::Unlock);
    }

    #[test]
    fn successful_auth_enters_main_and_resets_the_form() {
        let mut s = state();
        s.screen = Screen::Unlock;
        s.form.failed_attempts = 3;
        let _ = s.update(Message::AuthDone(Ok(())));
        assert_eq!(s.screen, Screen::Main);
        assert_eq!(s.form.failed_attempts, 0);
    }

    #[test]
    fn debug_of_state_never_contains_the_password() {
        let mut s = state();
        let _ = s.update(Message::PasswordChanged(Sensitive::new("hunter2".into())));
        assert!(!format!("{s:?}").contains("hunter2"));
    }

    /// End to end through the real store: create, lock, wrong password, right password.
    #[tokio::test]
    async fn store_round_trip_behind_the_screens() {
        let store = Store::open_in_memory().unwrap();
        let params = KdfParams::new(8, 1, 1).unwrap();
        let pw = |s: &str| secrecy::SecretString::from(s);
        store
            .create_vault_with_params(pw("master"), params)
            .await
            .unwrap();
        store.lock().await.unwrap();
        let wrong = store.unlock(pw("nope")).await.unwrap_err();
        assert_eq!(AuthFailure::from_store(&wrong), AuthFailure::WrongPassword);
        store.unlock(pw("master")).await.unwrap();
    }
}
