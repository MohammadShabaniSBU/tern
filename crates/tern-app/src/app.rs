//! The iced application: state, messages, update and view.

use std::time::Instant;

use iced::widget::{column, container, row, space, text, text_input};
use iced::{Element, Fill, Subscription, Task, Theme};
use tern_store::Store;

use crate::action::ModeMask;
use crate::action::{Action, Section};
use crate::auth::{self, AuthFailure, AuthForm, CONFIRM_ID, PASSWORD_ID, Sensitive, retry_delay};
use crate::keymap::{self, Chord, Context, Keymap, Matcher, Outcome};
use crate::list::{self, ListState};
use crate::modal::{self, Modal, ModalStack, Pending};
use crate::toast::{ToastKind, Toasts};

/// Which part of the app owns the keyboard. See `docs/KEYBINDINGS.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    /// A text input (list filter, later forms and terminals) owns plain keys.
    Insert,
    Palette,
    /// A modal dialog owns the keyboard.
    Modal,
}

impl Mode {
    /// The single `ModeMask` bit for this mode.
    pub fn mask(self) -> ModeMask {
        match self {
            Mode::Normal => ModeMask::NORMAL,
            Mode::Insert => ModeMask::INSERT,
            Mode::Palette => ModeMask::PALETTE,
            Mode::Modal => ModeMask::MODAL,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Palette => "PALETTE",
            Mode::Modal => "MODAL",
        }
    }
}

/// Which pane has keyboard focus. Exactly one always does, so something is always highlighted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    Sidebar,
    #[default]
    List,
    #[allow(dead_code)] // reachable once detail views exist (Sprint 3)
    Detail,
    #[allow(dead_code)] // reachable once terminal tabs exist (Sprint 5)
    Tab,
}

impl Pane {
    pub fn label(self) -> &'static str {
        match self {
            Pane::Sidebar => "sidebar",
            Pane::List => "list",
            Pane::Detail => "detail",
            Pane::Tab => "tab",
        }
    }
}

/// Widget id of the list filter input.
pub const FILTER_ID: &str = "list-filter";

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
    pub pane: Pane,
    lists: [ListState; 4],
    modals: ModalStack,
    toasts: Toasts,
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
    ToastTick(Instant),
    FilterChanged(String),
    /// `(offset_y, viewport_height)` reported by the list's scrollable.
    ListScrolled(f32, f32),
    /// Enter in the filter input: keep the filter, return to Normal mode.
    FilterSubmitted,
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
            pane: Pane::default(),
            modals: ModalStack::default(),
            toasts: Toasts::default(),
            lists: Section::ALL.map(|sec| ListState::new(list::dummy_items(sec))),
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
                if let Err(e) = &result {
                    tracing::error!(error = %e, "could not load hosts");
                    self.toast(ToastKind::Error, "Could not load hosts");
                }
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
                        let problems = self.keymap.warnings.len() + self.keymap.conflicts.len();
                        if problems > 0 {
                            self.toast(
                                ToastKind::Error,
                                format!("Keymap has {problems} problem(s); see the log"),
                            );
                        }
                    }
                    Err(e) => tracing::error!(error = %e, "could not load keymap settings"),
                }
                Task::none()
            }
            Message::ToastTick(now) => {
                self.toasts.expire(now);
                Task::none()
            }
            Message::FilterChanged(text) => {
                self.list_mut().set_filter(text);
                self.scroll_list(true)
            }
            Message::ListScrolled(y, h) => {
                self.list_mut().viewport_changed(y, h);
                Task::none()
            }
            Message::FilterSubmitted => self.leave_insert(),
            Message::Locked(result) => {
                if let Err(e) = result {
                    tracing::error!(error = %e, "lock failed");
                }
                // Even if the store reported an error, leave the main screen.
                self.screen = Screen::Unlock;
                self.mode = Mode::Normal;
                self.host_count = None;
                self.modals.clear();
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
            return self.scroll_list(false);
        }
        match action {
            Action::Back => self.back(),
            Action::Delete => {
                if let Some(item) = self.list().selected_item() {
                    let body = if item.group.is_some() {
                        format!("Delete group '{}' and everything in it?", item.label)
                    } else {
                        format!("Delete '{}'?", item.label)
                    };
                    self.modals.push(Modal::Confirm {
                        title: "Confirm delete".into(),
                        body,
                        then: Pending::DeleteSelected,
                    });
                    self.mode = Mode::Modal;
                }
                Task::none()
            }
            Action::Confirm | Action::Cancel => {
                let modal = self.modals.pop();
                self.sync_modal_mode();
                match (action, modal) {
                    (Action::Confirm, Some(Modal::Confirm { then, .. })) => self.run_pending(then),
                    _ => Task::none(),
                }
            }
            Action::Filter => {
                self.mode = Mode::Insert;
                self.pane = Pane::List;
                iced::widget::operation::focus(FILTER_ID)
            }
            Action::Collapse => {
                if self.pane == Pane::List && !self.list_mut().collapse() {
                    self.pane = Pane::Sidebar;
                }
                self.scroll_list(false)
            }
            Action::Expand => {
                if self.pane == Pane::Sidebar {
                    self.pane = Pane::List;
                } else {
                    self.list_mut().expand();
                }
                self.scroll_list(false)
            }
            Action::MoveDown | Action::MoveUp if self.pane == Pane::Sidebar => {
                let n = Section::ALL.len() as isize;
                let i = Section::ALL
                    .iter()
                    .position(|&s| s == self.section)
                    .unwrap_or(0) as isize;
                let step = if action == Action::MoveDown { 1 } else { -1 };
                self.section = Section::ALL[(i + step).clamp(0, n - 1) as usize];
                self.scroll_list(false)
            }
            Action::MoveDown
            | Action::MoveUp
            | Action::First
            | Action::Last
            | Action::PageDown
            | Action::PageUp => {
                let list = self.list_mut();
                match action {
                    Action::MoveDown => list.move_by(1),
                    Action::MoveUp => list.move_by(-1),
                    Action::First => list.first(),
                    Action::Last => list.last(),
                    Action::PageDown => list.page(true),
                    _ => list.page(false),
                }
                self.scroll_list(false)
            }
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
            other => {
                self.toast(
                    ToastKind::Info,
                    format!("Not available yet: {}", other.meta().title),
                );
                Task::none()
            }
        }
    }

    /// `Esc`: move one step outwards. Modal -> palette -> insert -> clear the filter.
    fn back(&mut self) -> Task<Message> {
        match self.mode {
            Mode::Modal => {
                self.modals.pop();
                self.sync_modal_mode();
                Task::none()
            }
            Mode::Palette => {
                self.mode = Mode::Normal;
                Task::none()
            }
            Mode::Insert => self.leave_insert(),
            Mode::Normal => {
                if self.list().filter.is_empty() {
                    Task::none()
                } else {
                    self.list_mut().set_filter(String::new());
                    self.scroll_list(true)
                }
            }
        }
    }

    fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(kind, text, Instant::now());
    }

    fn sync_modal_mode(&mut self) {
        self.mode = if self.modals.is_empty() {
            Mode::Normal
        } else {
            Mode::Modal
        };
        self.matcher.reset();
    }

    fn run_pending(&mut self, pending: Pending) -> Task<Message> {
        match pending {
            Pending::DeleteSelected => {
                // Dummy data only; real deletion arrives with the host/key screens (Sprint 3).
                if let Some(label) = self.list_mut().remove_selected() {
                    self.toast(ToastKind::Success, format!("Deleted '{label}'"));
                }
                self.scroll_list(false)
            }
        }
    }

    fn list(&self) -> &ListState {
        &self.lists[self.section_index()]
    }

    fn list_mut(&mut self) -> &mut ListState {
        let i = self.section_index();
        &mut self.lists[i]
    }

    fn section_index(&self) -> usize {
        Section::ALL
            .iter()
            .position(|&s| s == self.section)
            .unwrap_or(0)
    }

    /// Scroll the shared scrollable so the active list's selection is visible. `reset` jumps to
    /// the top first (filter changes).
    fn scroll_list(&mut self, reset: bool) -> Task<Message> {
        let list = self.list_mut();
        if reset {
            list.set_offset(0.0);
        }
        let target = list.reveal_offset();
        if let Some(y) = target {
            list.set_offset(y);
        }
        let y = if reset || target.is_some() {
            target.unwrap_or(0.0)
        } else {
            // Switching lists: restore this list's remembered position.
            list.offset()
        };
        iced::widget::operation::scroll_to(
            list::LIST_ID,
            iced::widget::scrollable::AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
    }

    fn leave_insert(&mut self) -> Task<Message> {
        self.mode = Mode::Normal;
        self.matcher.reset();
        // Drop widget focus so the keys we now handle are not also typed into the field.
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
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
        let mut nav = column![text("tern").size(20), space().height(12)].spacing(6);
        for section in Section::ALL {
            let label = if section == self.section {
                format!("> {}", section.label())
            } else {
                format!("  {}", section.label())
            };
            nav = nav.push(text(label));
        }
        let sidebar = container(nav)
            .padding(12)
            .width(200)
            .height(Fill)
            .style(container::bordered_box);

        let main_text = match self.mode {
            Mode::Palette => "Command palette (placeholder)".to_owned(),
            _ => match self.host_count {
                Some(n) => format!("Vault unlocked — {n} host(s) stored"),
                None => "Vault unlocked".to_owned(),
            },
        };
        let filter = text_input("Filter (/)", &self.list().filter)
            .id(FILTER_ID)
            .on_input(Message::FilterChanged)
            .on_submit(Message::FilterSubmitted);
        let list = list::view(
            self.list(),
            self.pane == Pane::List && self.mode == Mode::Normal,
        );
        let main_area = container(column![filter, text(main_text).size(14), list].spacing(12))
            .padding(12)
            .width(Fill)
            .height(Fill);

        let mut status = row![
            text(format!(
                "-- {} --   {}",
                self.mode.label(),
                self.pane.label()
            ))
            .size(14),
            space().width(Fill),
        ];
        if let Some(toast) = self.toasts.current() {
            let style: fn(&Theme) -> text::Style = match toast.kind {
                ToastKind::Info => text::base,
                ToastKind::Success => text::success,
                ToastKind::Error => text::danger,
            };
            status = status.push(text(toast.text.as_str()).size(14).style(style));
        }
        let status_bar = container(status)
            .padding([4, 12])
            .width(Fill)
            .style(container::bordered_box);

        let base = column![row![sidebar, main_area].height(Fill), status_bar];
        match self.modals.top() {
            Some(m) => iced::widget::stack![base, modal::view(m)].into(),
            None => base.into(),
        }
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
        if !self.toasts.is_empty() {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(500)).map(Message::ToastTick),
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
    fn esc_moves_one_step_outwards() {
        let mut s = main_state();
        s.list_mut().set_filter("prod".into());

        // Insert -> Normal keeps the filter; the next Esc clears it.
        let _ = s.update(Message::Dispatch(Action::Filter));
        assert_eq!((s.mode, s.pane), (Mode::Insert, Pane::List));
        let _ = s.update(Message::Dispatch(Action::Back));
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.list().filter, "prod");
        let _ = s.update(Message::Dispatch(Action::Back));
        assert!(s.list().filter.is_empty());

        // Palette -> Normal.
        let _ = s.update(Message::Dispatch(Action::OpenPalette));
        assert_eq!(s.mode, Mode::Palette);
        let _ = s.update(Message::Dispatch(Action::Back));
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn submitting_the_filter_leaves_insert_mode() {
        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::Filter));
        let _ = s.update(Message::FilterChanged("x".into()));
        let _ = s.update(Message::FilterSubmitted);
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.list().filter, "x");
    }

    #[test]
    fn pane_focus_always_has_a_value_and_h_l_move_it() {
        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::GoKeys)); // flat list: nothing to collapse
        assert_eq!(s.pane, Pane::List);
        let _ = s.update(Message::Dispatch(Action::Collapse));
        assert_eq!(s.pane, Pane::Sidebar);
        let _ = s.update(Message::Dispatch(Action::Expand));
        assert_eq!(s.pane, Pane::List);
    }

    #[test]
    fn jk_move_the_selection_and_gg_and_capital_g_jump() {
        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::GoKeys));
        for _ in 0..3 {
            let _ = s.update(Message::Dispatch(Action::MoveDown));
        }
        assert_eq!(s.list().selected_row(), Some(3));
        let _ = s.update(Message::Dispatch(Action::MoveUp));
        assert_eq!(s.list().selected_row(), Some(2));
        let _ = s.update(Message::Dispatch(Action::Last));
        assert_eq!(s.list().selected_row(), Some(999));
        let _ = s.update(Message::Dispatch(Action::First));
        assert_eq!(s.list().selected_row(), Some(0));
    }

    #[test]
    fn jk_in_the_sidebar_switch_sections() {
        let mut s = main_state();
        s.pane = Pane::Sidebar;
        let _ = s.update(Message::Dispatch(Action::MoveDown));
        assert_eq!(s.section, Section::Keys);
        let _ = s.update(Message::Dispatch(Action::MoveUp));
        let _ = s.update(Message::Dispatch(Action::MoveUp));
        assert_eq!(s.section, Section::Hosts);
    }

    #[test]
    fn dd_opens_a_keyboard_operable_confirm_and_delete_removes_the_item() {
        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::GoIdentities));
        let before = s.list().len_visible();

        // Delete -> Modal; Cancel keeps the item.
        let _ = s.update(Message::Dispatch(Action::Delete));
        assert_eq!(s.mode, Mode::Modal);
        let _ = s.update(Message::Dispatch(Action::Cancel));
        assert_eq!((s.mode, s.list().len_visible()), (Mode::Normal, before));

        // Esc also cancels.
        let _ = s.update(Message::Dispatch(Action::Delete));
        let _ = s.update(Message::Dispatch(Action::Back));
        assert_eq!((s.mode, s.list().len_visible()), (Mode::Normal, before));

        // Confirm deletes.
        let _ = s.update(Message::Dispatch(Action::Delete));
        let _ = s.update(Message::Dispatch(Action::Confirm));
        assert_eq!((s.mode, s.list().len_visible()), (Mode::Normal, before - 1));
        assert!(
            s.toasts
                .current()
                .is_some_and(|t| t.text.starts_with("Deleted"))
        );
    }

    #[test]
    fn locking_closes_modals() {
        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::Delete));
        let _ = s.update(Message::Locked(Ok(())));
        assert!(s.modals.is_empty());
        assert_eq!(s.mode, Mode::Normal);
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
