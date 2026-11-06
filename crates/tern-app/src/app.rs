//! The iced application: state, messages, update and view.

use std::time::Instant;

use iced::widget::{column, container, mouse_area, row, space, text, text_input};
use iced::{Border, Element, Fill, Font, Subscription, Task, Theme};
use tern_store::Store;

use crate::action::ModeMask;
use crate::action::{Action, Section};
use crate::auth::{self, AuthFailure, AuthForm, CONFIRM_ID, PASSWORD_ID, Sensitive, retry_delay};
use crate::host_form::{self, Choice, Field, HostForm, Picker};
use crate::hosts_view::{HostSort, HostsData};
use crate::identity_form::{self, IdField, IdentitiesData, IdentityForm};
use crate::key_form::{self, KeyForm, KeyMode};
use crate::keymap::{self, Chord, Context, Keymap, Matcher, Outcome};
use crate::keys_view::KeysData;
use crate::list::{self, ListState};
use crate::modal::{self, Modal, ModalStack, Pending};
use crate::palette::{self, HostRef, Palette, Target};
use crate::prompt::{self, Prompt, PromptKind, PromptOutcome};
use crate::reauth::{self, Gate, ReauthPrompt, SensitiveOp};
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
    dark: bool,
    pub pane: Pane,
    lists: [ListState; 4],
    modals: ModalStack,
    toasts: Toasts,
    form: AuthForm,
    keymap: Keymap,
    matcher: Matcher,
    /// Host labels for the palette's "Connect: …" entries.
    hosts: Vec<HostRef>,
    host_data: HostsData,
    host_sort: HostSort,
    host_form: Option<HostForm>,
    keys_data: KeysData,
    key_form: Option<KeyForm>,
    ids_data: IdentitiesData,
    identity_form: Option<IdentityForm>,
    /// Incremented per password copy so only the latest copy's timer clears the clipboard.
    clip_token: u64,
    gate: Gate,
    reauth: Option<ReauthPrompt>,
    /// A stored identity password the user explicitly revealed: `(identity id, password)`.
    revealed: Option<(String, Sensitive)>,
    prompt: Option<Prompt>,
    palette: Palette,
}

/// Settings read from the `settings` table after unlocking.
#[derive(Debug, Clone)]
pub struct LoadedSettings {
    leader: Option<String>,
    keymap: Option<String>,
    recent: Option<String>,
    theme: Option<String>,
    reauth: Option<String>,
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
    HostsLoaded(Result<HostsData, String>),
    SpinnerTick,
    /// Enter pressed anywhere; only meaningful on the recovery warning.
    Acknowledge,
    FocusNext,
    FocusPrevious,
    /// Run an action; the single entry point shared by keymap and palette.
    Dispatch(Action),
    /// A key press on the main screen, routed through the keymap engine.
    Key(Chord),
    /// `(leader, keymap overrides)` JSON from the settings table.
    SettingsLoaded(Result<LoadedSettings, String>),
    PaletteQueryChanged(String),
    SettingSaved(Result<(), String>),
    Locked(Result<(), String>),
    ToastTick(Instant),
    FilterChanged(String),
    /// `(offset_y, viewport_height)` reported by the list's scrollable.
    ListScrolled(f32, f32),
    /// Enter in the filter input: keep the filter, return to Normal mode.
    FilterSubmitted,
    /// A host or group was deleted from the store (label), or the attempt failed.
    ItemDeleted(Result<String, String>),
    FormEdit(Field, String),
    FormPick(Picker, Choice),
    /// Enter in a form field.
    FormSave,
    HostSaved(Result<String, String>),
    KeysLoaded(Result<KeysData, String>),
    KeyPathChanged(String),
    KeyLabelChanged(String),
    KeyPassphraseChanged(Sensitive),
    KeyKindPicked(tern_ssh::keys::KeyKind),
    KeyPrivateEdit(iced::widget::text_editor::Action),
    KeyPublicChanged(String),
    KeySubmit,
    KeySaved(Result<String, String>),
    ClipboardRead(Option<String>),
    IdentitiesLoaded(Result<IdentitiesData, String>),
    IdentityEdit(IdField, String),
    IdentityPassword(Sensitive),
    IdentityKey(Choice),
    IdentityClearPassword(bool),
    IdentitySave,
    IdentitySaved(Result<String, String>),
    ClipboardClear(u64),
    ReauthPassword(Sensitive),
    ReauthSubmit,
    ReauthDone(Result<(), AuthFailure>),
    PasswordRevealed(Result<Option<Sensitive>, String>),
    PromptChanged(String),
    PromptSubmit,
    /// A group/move operation finished; the string is a toast message.
    GroupsChanged(Result<String, String>),
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
            dark: true,
            pane: Pane::default(),
            modals: ModalStack::default(),
            toasts: Toasts::default(),
            lists: Section::ALL.map(|sec| ListState::new(list::dummy_items(sec))),
            form: AuthForm::default(),
            keymap: Keymap::defaults(),
            matcher: Matcher::default(),
            hosts: Vec::new(),
            host_data: HostsData::default(),
            host_sort: HostSort::default(),
            host_form: None,
            keys_data: KeysData::default(),
            key_form: None,
            ids_data: IdentitiesData::default(),
            identity_form: None,
            clip_token: 0,
            gate: Gate::default(),
            reauth: None,
            revealed: None,
            prompt: None,
            palette: Palette::default(),
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
                self.host_data = result.unwrap_or_default();
                self.hosts = self.host_data.host_refs();
                self.rebuild_hosts_list();
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
            Message::SettingSaved(result) => {
                if let Err(e) = result {
                    tracing::warn!(error = %e, "could not save palette recents");
                }
                Task::none()
            }
            Message::PaletteQueryChanged(q) => {
                self.palette.set_query(q);
                Task::none()
            }
            Message::SettingsLoaded(result) => {
                match result {
                    Ok(loaded) => {
                        let recent: Vec<String> = loaded
                            .recent
                            .and_then(|json| serde_json::from_str(&json).ok())
                            .unwrap_or_default();
                        self.palette.set_recent(recent);
                        // Stored as JSON text; anything but "light" keeps the dark default.
                        self.dark = loaded.theme.as_deref().map(str::trim) != Some("\"light\"");
                        self.keymap =
                            Keymap::build(loaded.leader.as_deref(), loaded.keymap.as_deref());
                        self.gate.set_minutes_from_setting(loaded.reauth.as_deref());
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
            Message::FormEdit(field, value) => {
                if let Some(form) = &mut self.host_form {
                    form.set(field, value);
                }
                Task::none()
            }
            Message::FormPick(picker, choice) => {
                if let Some(form) = &mut self.host_form {
                    form.pick(picker, choice);
                }
                Task::none()
            }
            Message::FormSave => self.save_host_form(),
            Message::ReauthPassword(v) => {
                if let Some(r) = &mut self.reauth {
                    r.password = v;
                    r.error = None;
                }
                Task::none()
            }
            Message::ReauthSubmit => {
                let Some(r) = &mut self.reauth else {
                    return Task::none();
                };
                if r.busy || r.password.is_empty() {
                    return Task::none();
                }
                r.busy = true;
                let password = r.password.take_secret();
                let store = self.store.clone();
                Task::perform(
                    async move {
                        store
                            .verify_password(password)
                            .await
                            .map_err(|e| AuthFailure::from_store(&e))
                    },
                    Message::ReauthDone,
                )
            }
            Message::ReauthDone(Ok(())) => {
                let Some(r) = self.reauth.take() else {
                    return Task::none();
                };
                self.gate.mark_verified(Instant::now());
                self.mode = Mode::Normal;
                self.matcher.reset();
                self.run_sensitive(r.then)
            }
            Message::ReauthDone(Err(f)) => {
                if let Some(r) = &mut self.reauth {
                    r.busy = false;
                    r.password.clear();
                    r.error = Some(f.message());
                }
                iced::widget::operation::focus(reauth::REAUTH_ID)
            }
            Message::PasswordRevealed(Ok(Some(pw))) => {
                if let Some((id, _)) = &self.revealed {
                    let id = id.clone();
                    self.revealed = Some((id, pw));
                }
                Task::none()
            }
            Message::PasswordRevealed(Ok(None)) => {
                self.revealed = None;
                self.toast(ToastKind::Info, "This identity has no stored password");
                Task::none()
            }
            Message::PasswordRevealed(Err(e)) => {
                self.revealed = None;
                tracing::error!(error = %e, "could not reveal password");
                self.toast(ToastKind::Error, "Could not read the password");
                Task::none()
            }
            Message::KeysLoaded(r) => {
                if let Err(e) = &r {
                    tracing::error!(error = %e, "could not load keys");
                    self.toast(ToastKind::Error, "Could not load keys");
                }
                self.keys_data = r.unwrap_or_default();
                let items = self.keys_data.items();
                self.lists[1].replace_items(items);
                let ids = self.ids_data.items(&self.keys_data.keys);
                self.lists[2].replace_items(ids);
                Task::none()
            }
            Message::IdentitiesLoaded(r) => {
                if let Err(e) = &r {
                    tracing::error!(error = %e, "could not load identities");
                    self.toast(ToastKind::Error, "Could not load identities");
                }
                self.ids_data = r.unwrap_or_default();
                let items = self.ids_data.items(&self.keys_data.keys);
                self.lists[2].replace_items(items);
                Task::none()
            }
            Message::IdentityEdit(f, v) => self.edit_identity_form(|form| form.set_text(f, v)),
            Message::IdentityPassword(v) => self.edit_identity_form(|form| {
                form.password = v;
                form.error = None;
            }),
            Message::IdentityKey(c) => self.edit_identity_form(|form| form.key = c),
            Message::IdentityClearPassword(b) => {
                self.edit_identity_form(|form| form.clear_password = b)
            }
            Message::IdentitySave => self.save_identity_form(),
            Message::IdentitySaved(Ok(label)) => {
                self.identity_form = None;
                self.mode = Mode::Normal;
                self.toast(ToastKind::Success, format!("Saved identity '{label}'"));
                Task::batch([self.reload_identities(), self.reload_hosts()])
            }
            Message::IdentitySaved(Err(e)) => {
                if let Some(f) = &mut self.identity_form {
                    f.error = Some(format!("Could not save: {e}"));
                }
                Task::none()
            }
            Message::ClipboardClear(token) => {
                if token == self.clip_token {
                    iced::clipboard::write(String::new())
                } else {
                    Task::none()
                }
            }
            Message::KeyPathChanged(v) => self.edit_key_form(|f| f.path = v),
            Message::KeyLabelChanged(v) => self.edit_key_form(|f| f.label = v),
            Message::KeyPassphraseChanged(v) => self.edit_key_form(|f| f.passphrase = v),
            Message::KeyPrivateEdit(a) => self.edit_key_form(|f| f.private.perform(a)),
            Message::KeyPublicChanged(v) => self.edit_key_form(|f| f.public = v),
            Message::KeyKindPicked(k) => self.edit_key_form(|f| f.kind = k),
            Message::KeySubmit => self.submit_key_form(),
            Message::KeySaved(Ok(label)) => {
                self.key_form = None;
                self.mode = Mode::Normal;
                self.toast(ToastKind::Success, format!("Saved key '{label}'"));
                self.reload_keys()
            }
            Message::KeySaved(Err(e)) => {
                if let Some(f) = &mut self.key_form {
                    f.busy = false;
                    f.error = Some(e);
                }
                Task::none()
            }
            Message::ClipboardRead(text) => match text.filter(|t| !t.trim().is_empty()) {
                Some(t) => {
                    let mut f = KeyForm::new(KeyMode::Add);
                    f.set_private(&t);
                    self.open_key_form(f)
                }
                None => {
                    self.toast(ToastKind::Info, "Clipboard is empty");
                    Task::none()
                }
            },
            Message::PromptChanged(v) => {
                if let Some(p) = &mut self.prompt {
                    p.input = v;
                    p.error = None;
                }
                Task::none()
            }
            Message::PromptSubmit => self.submit_prompt(),
            Message::GroupsChanged(Ok(msg)) => {
                self.toast(ToastKind::Success, msg);
                self.reload_hosts()
            }
            Message::GroupsChanged(Err(e)) => {
                tracing::error!(error = %e, "group operation failed");
                self.toast(ToastKind::Error, "Operation failed");
                Task::none()
            }
            Message::HostSaved(Ok(label)) => {
                self.host_form = None;
                self.mode = Mode::Normal;
                self.toast(ToastKind::Success, format!("Saved '{label}'"));
                self.reload_hosts()
            }
            Message::HostSaved(Err(e)) => {
                if let Some(form) = &mut self.host_form {
                    form.error = Some(format!("Could not save: {e}"));
                }
                Task::none()
            }
            Message::ItemDeleted(Ok(label)) => {
                self.toast(ToastKind::Success, format!("Deleted '{label}'"));
                Task::batch([
                    self.reload_hosts(),
                    self.reload_keys(),
                    self.reload_identities(),
                ])
            }
            Message::ItemDeleted(Err(e)) => {
                tracing::error!(error = %e, "delete failed");
                self.toast(ToastKind::Error, "Delete failed");
                Task::none()
            }
            Message::Locked(result) => {
                if let Err(e) = result {
                    tracing::error!(error = %e, "lock failed");
                }
                // Even if the store reported an error, leave the main screen.
                self.screen = Screen::Unlock;
                self.mode = Mode::Normal;
                self.hosts.clear();
                self.host_data = HostsData::default();
                self.host_form = None;
                self.prompt = None;
                self.key_form = None;
                self.identity_form = None;
                self.reauth = None;
                self.revealed = None;
                self.gate.reset();
                self.ids_data = IdentitiesData::default();
                self.keys_data = KeysData::default();
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
        // A revealed password must not follow the selection around.
        if self.revealed.is_some()
            && (action.section().is_some()
                || matches!(
                    action,
                    Action::MoveDown
                        | Action::MoveUp
                        | Action::First
                        | Action::Last
                        | Action::PageDown
                        | Action::PageUp
                        | Action::Filter
                        | Action::LockVault
                ))
        {
            self.revealed = None;
        }
        if let Some(section) = action.section() {
            self.section = section;
            return self.scroll_list(false);
        }
        match action {
            Action::Back => self.back(),
            Action::ToggleTheme => {
                self.dark = !self.dark;
                let json = if self.dark { "\"dark\"" } else { "\"light\"" };
                let store = self.store.clone();
                Task::perform(
                    async move {
                        store
                            .query(move |db| db.settings().set("theme", json))
                            .await
                    },
                    |r| Message::SettingSaved(r.map_err(|e| e.to_string())),
                )
            }
            Action::Edit if self.selected_group().is_some() => {
                let Some((id, name)) = self.selected_group() else {
                    return Task::none();
                };
                self.open_prompt(Prompt::new(PromptKind::RenameGroup(id), name))
            }
            Action::NewGroup if self.section == Section::Hosts => {
                let parent = self.selected_group().map(|(id, _)| id);
                self.open_prompt(Prompt::new(PromptKind::NewGroup { parent }, ""))
            }
            Action::MoveToGroup if self.section == Section::Hosts => {
                let host = self
                    .list()
                    .selected_item()
                    .filter(|i| i.group.is_none())
                    .and_then(|i| i.id.as_deref())
                    .and_then(|id| self.host_data.host(id));
                match host {
                    Some(h) => {
                        let current = h
                            .group_id
                            .and_then(|g| self.host_data.groups.iter().find(|x| x.id == g))
                            .map(|g| g.name.clone())
                            .unwrap_or_default();
                        let id = h.id;
                        self.open_prompt(Prompt::new(PromptKind::MoveHost(id), current))
                    }
                    None => {
                        self.toast(ToastKind::Info, "Select a host to move");
                        Task::none()
                    }
                }
            }
            Action::NewGroup | Action::MoveToGroup => Task::none(),
            Action::ToggleMark if self.section == Section::Hosts => {
                if self.list_mut().toggle_mark() {
                    self.list_mut().move_by(1);
                }
                self.scroll_list(false)
            }
            Action::BulkTag if self.section == Section::Hosts => {
                let mut ids = self.list().marked_ids();
                if ids.is_empty() {
                    ids.extend(
                        self.list()
                            .selected_item()
                            .filter(|i| i.group.is_none())
                            .and_then(|i| i.id.clone()),
                    );
                }
                let hosts: Vec<tern_core::HostId> =
                    ids.iter().filter_map(|id| id.parse().ok()).collect();
                if hosts.is_empty() {
                    self.toast(ToastKind::Info, "Select or mark hosts first");
                    return Task::none();
                }
                self.open_prompt(Prompt::new(PromptKind::BulkTag(hosts), ""))
            }
            Action::Duplicate if self.section == Section::Hosts => {
                let host = self
                    .list()
                    .selected_item()
                    .filter(|i| i.group.is_none())
                    .and_then(|i| i.id.as_deref())
                    .and_then(|id| self.host_data.host(id))
                    .cloned();
                let Some(mut copy) = host else {
                    self.toast(ToastKind::Info, "Select a host to duplicate");
                    return Task::none();
                };
                copy.id = tern_core::HostId::new();
                copy.label = format!("{} (copy)", copy.label);
                let store = self.store.clone();
                Task::perform(
                    async move {
                        let label = copy.label.clone();
                        store
                            .insert_host(copy)
                            .await
                            .map(|()| format!("Created '{label}'"))
                            .map_err(|e| e.to_string())
                    },
                    Message::GroupsChanged,
                )
            }
            Action::RevealStored if self.section == Section::Identities => {
                let id = self
                    .list()
                    .selected_item()
                    .and_then(|i| i.id.as_deref())
                    .and_then(|id| id.parse().ok());
                match id {
                    Some(id) => self.request_sensitive(SensitiveOp::RevealPassword(id)),
                    None => Task::none(),
                }
            }
            Action::ExportKey if self.section == Section::Keys => {
                let id = self
                    .list()
                    .selected_item()
                    .and_then(|i| i.id.as_deref())
                    .and_then(|id| id.parse().ok());
                match id {
                    Some(id) => self.request_sensitive(SensitiveOp::ExportKey(id)),
                    None => Task::none(),
                }
            }
            Action::RevealStored | Action::ExportKey => Task::none(),
            Action::ToggleMark | Action::BulkTag | Action::Duplicate => Task::none(),
            Action::Add if self.section == Section::Keys => {
                self.open_key_form(KeyForm::new(KeyMode::Add))
            }
            Action::GenerateKey if self.section == Section::Keys => {
                self.open_key_form(KeyForm::new(KeyMode::Generate))
            }
            Action::GenerateKey => Task::none(),
            Action::ImportKey if self.section == Section::Keys => {
                self.open_key_form(KeyForm::new(KeyMode::ImportFile))
            }
            Action::PasteKey if self.section == Section::Keys => {
                iced::clipboard::read().map(Message::ClipboardRead)
            }
            Action::CopyPublicKey if self.section == Section::Keys => {
                let key = self
                    .list()
                    .selected_item()
                    .and_then(|i| i.id.as_deref())
                    .and_then(|id| self.keys_data.key(id));
                match key {
                    Some(k) => {
                        let public = k.public_key.clone();
                        self.toast(ToastKind::Success, "Copied public key");
                        iced::clipboard::write(public)
                    }
                    None => Task::none(),
                }
            }
            Action::ImportKey | Action::PasteKey | Action::CopyPublicKey => Task::none(),
            Action::Add | Action::Edit if self.section == Section::Identities => {
                let existing = if action == Action::Edit {
                    let found = self
                        .list()
                        .selected_item()
                        .and_then(|i| i.id.as_deref())
                        .and_then(|id| self.ids_data.identity(id));
                    match found {
                        Some(i) => Some((i.clone(), self.ids_data.with_password.contains(&i.id))),
                        None => {
                            self.toast(ToastKind::Info, "Select an identity to edit");
                            return Task::none();
                        }
                    }
                } else {
                    None
                };
                self.identity_form = Some(IdentityForm::new(
                    existing.as_ref().map(|(i, p)| (i, *p)),
                    &self.keys_data.keys,
                ));
                self.mode = Mode::Insert;
                self.matcher.reset();
                iced::widget::operation::focus(identity_form::LABEL_ID)
            }
            Action::FormReveal | Action::FormGenerate | Action::FormCopy => {
                self.identity_password_action(action)
            }
            Action::Add | Action::Edit => self.open_host_form(action == Action::Edit),
            Action::FormSave if self.key_form.is_some() => self.submit_key_form(),
            Action::FormSave if self.identity_form.is_some() => self.save_identity_form(),
            Action::FormSave => self.save_host_form(),
            Action::FormNext if self.any_form_open() => iced::widget::operation::focus_next(),
            Action::FormPrev if self.any_form_open() => iced::widget::operation::focus_previous(),
            Action::FormNext | Action::FormPrev => Task::none(),
            Action::SortToggle => {
                if self.section == Section::Hosts {
                    self.host_sort = self.host_sort.toggled();
                    self.rebuild_hosts_list();
                    self.toast(
                        ToastKind::Info,
                        format!("Sorted by {}", self.host_sort.label()),
                    );
                }
                Task::none()
            }
            Action::Delete => {
                if let Some(item) = self.list().selected_item() {
                    let body = if item.group.is_some() {
                        format!("Delete group '{}'? Its hosts become ungrouped.", item.label)
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
            Action::CheatSheet => {
                if matches!(self.modals.top(), Some(Modal::CheatSheet { .. })) {
                    self.modals.pop();
                } else {
                    // Insert and Modal-from-cheatsheet cannot nest; remember where we came from.
                    self.modals.push(Modal::CheatSheet {
                        mode: self.mode.mask(),
                    });
                }
                self.sync_modal_mode();
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
            Action::QuickConnect => {
                self.palette.open_hosts(&self.hosts);
                self.mode = Mode::Palette;
                self.matcher.reset();
                iced::widget::operation::focus(palette::PALETTE_ID)
            }
            Action::OpenPalette => match self.mode {
                Mode::Palette => {
                    self.mode = Mode::Normal;
                    Task::none()
                }
                _ => {
                    self.palette.open(&self.keymap, &self.hosts);
                    self.mode = Mode::Palette;
                    self.matcher.reset();
                    iced::widget::operation::focus(palette::PALETTE_ID)
                }
            },
            Action::PaletteDown => {
                self.palette.move_selection(1);
                Task::none()
            }
            Action::PaletteUp => {
                self.palette.move_selection(-1);
                Task::none()
            }
            Action::PaletteRun => self.run_palette_selection(),
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
        if self.reauth.take().is_some() {
            return self.leave_insert();
        }
        if self.revealed.take().is_some() {
            return Task::none();
        }
        if self.identity_form.take().is_some() {
            return self.leave_insert();
        }
        if self.key_form.as_ref().is_some_and(|f| !f.busy) {
            self.key_form = None;
            return self.leave_insert();
        }
        if self.prompt.take().is_some() {
            return self.leave_insert();
        }
        if let Some(form) = &mut self.host_form {
            if form.request_close() {
                self.host_form = None;
                return self.leave_insert();
            }
            return Task::none();
        }
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

    fn run_palette_selection(&mut self) -> Task<Message> {
        let Some(entry) = self.palette.selected_entry().cloned() else {
            return Task::none();
        };
        self.mode = Mode::Normal;
        self.palette.record_use(&entry.recent_key);
        let save = {
            let store = self.store.clone();
            let json = serde_json::to_string(self.palette.recent()).unwrap_or_default();
            Task::perform(
                async move {
                    store
                        .query(move |db| db.settings().set(palette::RECENT_SETTING, &json))
                        .await
                },
                |r| Message::SettingSaved(r.map_err(|e| e.to_string())),
            )
        };
        let run = match entry.target {
            Target::Action(action) => self.dispatch(action),
            Target::Connect(label) => {
                self.toast(
                    ToastKind::Info,
                    format!("Connecting to '{label}' arrives with the SSH engine"),
                );
                Task::none()
            }
        };
        Task::batch([save, run])
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
                let real = matches!(
                    self.section,
                    Section::Hosts | Section::Keys | Section::Identities
                )
                .then(|| self.list().selected_item())
                .flatten()
                .and_then(|i| {
                    i.id.clone()
                        .map(|id| (id, i.group.is_some(), i.label.clone()))
                });
                if let Some((id, is_group, label)) = real {
                    return self.delete_host_or_group(&id, is_group, label);
                }
                // Sections not backed by the store yet still use dummy rows.
                if let Some(label) = self.list_mut().remove_selected() {
                    self.toast(ToastKind::Success, format!("Deleted '{label}'"));
                }
                self.scroll_list(false)
            }
        }
    }

    /// Rebuild the Hosts list from `host_data`, keeping collapse state and selection.
    fn rebuild_hosts_list(&mut self) {
        let items = self.host_data.items(self.host_sort);
        self.lists[0].replace_items(items);
    }

    fn reload_hosts(&self) -> Task<Message> {
        let store = self.store.clone();
        Task::perform(async move { load_hosts(&store).await }, |r| {
            Message::HostsLoaded(r.map_err(|e| e.to_string()))
        })
    }

    /// `(id, name)` when the selected Hosts row is a group.
    fn selected_group(&self) -> Option<(tern_core::GroupId, String)> {
        if self.section != Section::Hosts {
            return None;
        }
        let item = self.list().selected_item().filter(|i| i.group.is_some())?;
        Some((item.id.as_deref()?.parse().ok()?, item.label.clone()))
    }

    fn open_prompt(&mut self, prompt: Prompt) -> Task<Message> {
        self.prompt = Some(prompt);
        self.mode = Mode::Insert;
        self.matcher.reset();
        iced::widget::operation::focus(prompt::PROMPT_ID)
    }

    fn submit_prompt(&mut self) -> Task<Message> {
        let Some(p) = &mut self.prompt else {
            return Task::none();
        };
        let outcome = match p.resolve(&self.host_data.groups) {
            Ok(o) => o,
            Err(e) => {
                p.error = Some(e);
                return Task::none();
            }
        };
        self.prompt = None;
        self.mode = Mode::Normal;
        self.matcher.reset();
        if let PromptOutcome::ExportKey(id, path) = outcome {
            let store = self.store.clone();
            return Task::perform(
                async move {
                    let pem = store
                        .get_key_private(id)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or("key has no private part")?;
                    let dest = expand_home(&path);
                    write_private_file(&dest, secrecy::ExposeSecret::expose_secret(&pem))
                        .map_err(|e| format!("{}: {e}", dest.display()))?;
                    Ok(format!("Exported to {}", dest.display()))
                },
                Message::GroupsChanged,
            );
        }
        if matches!(outcome, PromptOutcome::AddTags(..)) {
            self.lists[0].clear_marks();
        }
        let store = self.store.clone();
        Task::perform(
            async move {
                match outcome {
                    PromptOutcome::CreateGroup(g) => {
                        let name = g.name.clone();
                        store
                            .insert_group(g)
                            .await
                            .map(|()| format!("Created group '{name}'"))
                            .map_err(|e| e.to_string())
                    }
                    PromptOutcome::RenameGroup(id, name) => {
                        let groups = store.list_groups().await.map_err(|e| e.to_string())?;
                        let Some(mut g) = groups.into_iter().find(|g| g.id == id) else {
                            return Err("group no longer exists".to_owned());
                        };
                        g.name = name.clone();
                        store
                            .update_group(g)
                            .await
                            .map(|()| format!("Renamed to '{name}'"))
                            .map_err(|e| e.to_string())
                    }
                    PromptOutcome::AddTags(ids, tags) => {
                        let hosts = store.list_hosts().await.map_err(|e| e.to_string())?;
                        let mut n = 0;
                        for mut h in hosts.into_iter().filter(|h| ids.contains(&h.id)) {
                            for t in &tags {
                                if !h.tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                                    h.tags.push(t.clone());
                                }
                            }
                            store.update_host(h).await.map_err(|e| e.to_string())?;
                            n += 1;
                        }
                        Ok(format!("Tagged {n} host(s)"))
                    }
                    PromptOutcome::ExportKey(..) => Ok(String::new()),
                    PromptOutcome::MoveHost(host, group) => store
                        .set_host_group(host, group)
                        .await
                        .map(|()| "Moved host".to_owned())
                        .map_err(|e| e.to_string()),
                }
            },
            Message::GroupsChanged,
        )
    }

    /// Run `op` now if the master password was proven recently, else ask for it first.
    fn request_sensitive(&mut self, op: SensitiveOp) -> Task<Message> {
        if !self.gate.needs_password(Instant::now()) {
            return self.run_sensitive(op);
        }
        self.reauth = Some(ReauthPrompt::new(op));
        self.mode = Mode::Insert;
        self.matcher.reset();
        iced::widget::operation::focus(reauth::REAUTH_ID)
    }

    fn run_sensitive(&mut self, op: SensitiveOp) -> Task<Message> {
        match op {
            SensitiveOp::RevealPassword(id) => {
                // Placeholder until the password arrives; the detail pane shows it only for
                // this identity and it is dropped on Esc, lock, or selecting another row.
                self.revealed = Some((id.to_string(), Sensitive::default()));
                let store = self.store.clone();
                Task::perform(
                    async move {
                        store
                            .get_identity_password(id)
                            .await
                            .map(|p| {
                                p.map(|p| {
                                    Sensitive::new(
                                        secrecy::ExposeSecret::expose_secret(&p).to_owned(),
                                    )
                                })
                            })
                            .map_err(|e| e.to_string())
                    },
                    Message::PasswordRevealed,
                )
            }
            SensitiveOp::ExportKey(id) => {
                let label = self
                    .keys_data
                    .key(&id.to_string())
                    .map(|k| k.label.clone())
                    .unwrap_or_else(|| "key".into());
                self.open_prompt(Prompt::new(PromptKind::ExportKey(id), format!("~/{label}")))
            }
        }
    }

    fn any_form_open(&self) -> bool {
        self.host_form.is_some() || self.key_form.is_some() || self.identity_form.is_some()
    }

    fn edit_identity_form(&mut self, f: impl FnOnce(&mut IdentityForm)) -> Task<Message> {
        if let Some(form) = &mut self.identity_form {
            f(form);
        }
        Task::none()
    }

    fn identity_password_action(&mut self, action: Action) -> Task<Message> {
        let Some(form) = &mut self.identity_form else {
            return Task::none();
        };
        match action {
            Action::FormReveal => form.reveal = !form.reveal,
            Action::FormGenerate => form.generate(),
            _ => {
                if form.password.is_empty() {
                    self.toast(ToastKind::Info, "No password to copy");
                    return Task::none();
                }
                let pw = form.password.as_str().to_owned();
                self.clip_token += 1;
                let token = self.clip_token;
                self.toast(
                    ToastKind::Success,
                    "Copied password; clipboard clears in 30 s",
                );
                return Task::batch([
                    iced::clipboard::write(pw),
                    Task::perform(
                        tokio::time::sleep(std::time::Duration::from_secs(30)),
                        move |()| Message::ClipboardClear(token),
                    ),
                ]);
            }
        }
        Task::none()
    }

    fn reload_identities(&self) -> Task<Message> {
        let store = self.store.clone();
        Task::perform(async move { load_identities(&store).await }, |r| {
            Message::IdentitiesLoaded(r.map_err(|e| e.to_string()))
        })
    }

    fn save_identity_form(&mut self) -> Task<Message> {
        let Some(form) = &mut self.identity_form else {
            return Task::none();
        };
        let draft = match form.build() {
            Ok(d) => d,
            Err(e) => {
                form.error = Some(e);
                return Task::none();
            }
        };
        let store = self.store.clone();
        Task::perform(
            async move {
                let label = draft.identity.label.clone();
                let r = if draft.is_edit {
                    store.update_identity(draft.identity, draft.password).await
                } else {
                    store
                        .insert_identity(draft.identity, draft.password.flatten())
                        .await
                };
                r.map(|()| label).map_err(|e| e.to_string())
            },
            Message::IdentitySaved,
        )
    }

    fn open_key_form(&mut self, form: KeyForm) -> Task<Message> {
        let first = form.first_field();
        self.key_form = Some(form);
        self.mode = Mode::Insert;
        self.matcher.reset();
        iced::widget::operation::focus(first)
    }

    fn edit_key_form(&mut self, f: impl FnOnce(&mut KeyForm)) -> Task<Message> {
        if let Some(form) = &mut self.key_form {
            f(form);
            form.error = None;
        }
        Task::none()
    }

    fn reload_keys(&self) -> Task<Message> {
        let store = self.store.clone();
        Task::perform(async move { load_keys(&store).await }, |r| {
            Message::KeysLoaded(r.map_err(|e| e.to_string()))
        })
    }

    fn submit_key_form(&mut self) -> Task<Message> {
        use tern_ssh::keys;
        let Some(form) = &mut self.key_form else {
            return Task::none();
        };
        if form.busy {
            return Task::none();
        }
        let label = form.effective_label();
        if label.is_empty() {
            form.error = Some("label must not be empty".into());
            return Task::none();
        }
        if form.mode == KeyMode::Add && form.private.text().trim().is_empty() {
            form.error = Some("paste the private key".into());
            return Task::none();
        }
        form.busy = true;
        form.error = None;
        let mode = form.mode;
        let kind = form.kind;
        let path = form.path.trim().to_owned();
        let pasted = form.private.text();
        let public = form.public.clone();
        let passphrase =
            (!form.passphrase.is_empty()).then(|| form.passphrase.clone().take_secret());
        let store = self.store.clone();
        Task::perform(
            async move {
                let work =
                    tokio::task::spawn_blocking(move || -> Result<keys::KeyMaterial, String> {
                        let comment = "tern";
                        match mode {
                            KeyMode::Generate => keys::generate(kind, comment, passphrase.as_ref()),
                            KeyMode::ImportFile => {
                                let text = std::fs::read_to_string(expand_home(&path))
                                    .map_err(|e| format!("cannot read file: {e}"))?;
                                keys::import(&text, None, "", passphrase.as_ref())
                            }
                            KeyMode::Add => {
                                keys::import(&pasted, Some(&public), "", passphrase.as_ref())
                            }
                        }
                        .map_err(|e| e.to_string())
                    });
                let material = work.await.map_err(|e| e.to_string())??;
                let meta = tern_core::SshKeyMeta {
                    id: tern_core::KeyId::new(),
                    label: label.clone(),
                    key_type: material.key_type,
                    public_key: material.public_key,
                    fingerprint: material.fingerprint,
                };
                store
                    .insert_key(meta, material.private_pem, material.passphrase)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(label)
            },
            Message::KeySaved,
        )
    }

    fn open_host_form(&mut self, edit: bool) -> Task<Message> {
        if self.section != Section::Hosts {
            self.toast(ToastKind::Info, "Not available for this section yet");
            return Task::none();
        }
        let selected = self.list().selected_item().cloned();
        let host = if edit {
            let Some(item) = selected.as_ref().filter(|i| i.group.is_none()) else {
                self.toast(ToastKind::Info, "Select a host to edit");
                return Task::none();
            };
            let Some(host) = item.id.as_deref().and_then(|id| self.host_data.host(id)) else {
                return Task::none();
            };
            Some(host.clone())
        } else {
            None
        };
        // New hosts start in the selected group (or the selected host's group).
        let default_group = if host.is_none() {
            selected.and_then(|i| match i.group {
                Some(_) => i.id,
                None => {
                    i.id.as_deref()
                        .and_then(|id| self.host_data.host(id))
                        .and_then(|h| h.group_id.map(|g| g.to_string()))
                }
            })
        } else {
            None
        };
        self.host_form = Some(HostForm::new(&self.host_data, host.as_ref(), default_group));
        self.mode = Mode::Insert;
        self.matcher.reset();
        iced::widget::operation::focus(Field::FIRST_ID)
    }

    fn save_host_form(&mut self) -> Task<Message> {
        let Some(form) = &mut self.host_form else {
            return Task::none();
        };
        let host = match form.build() {
            Ok(h) => h,
            Err(e) => {
                form.error = Some(e);
                return Task::none();
            }
        };
        let is_edit = form.is_edit();
        let store = self.store.clone();
        Task::perform(
            async move {
                let label = host.label.clone();
                let r = if is_edit {
                    store.update_host(host).await
                } else {
                    store.insert_host(host).await
                };
                r.map(|()| label).map_err(|e| e.to_string())
            },
            Message::HostSaved,
        )
    }

    fn delete_host_or_group(&mut self, id: &str, is_group: bool, label: String) -> Task<Message> {
        let store = self.store.clone();
        let is_key = self.section == Section::Keys;
        let is_identity = self.section == Section::Identities;
        let id = id.to_owned();
        Task::perform(
            async move {
                if is_identity {
                    let iid = id.parse().map_err(|e| format!("{e}"))?;
                    store
                        .delete_identity(iid)
                        .await
                        .map_err(|e| e.to_string())?;
                } else if is_key {
                    let kid = id.parse().map_err(|e| format!("{e}"))?;
                    store.delete_key(kid).await.map_err(|e| e.to_string())?;
                } else if is_group {
                    let gid = id.parse().map_err(|e| format!("{e}"))?;
                    store.delete_group(gid).await.map_err(|e| e.to_string())?;
                } else {
                    let hid = id.parse().map_err(|e| format!("{e}"))?;
                    store.delete_host(hid).await.map_err(|e| e.to_string())?;
                }
                Ok(label)
            },
            Message::ItemDeleted,
        )
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
        self.gate.mark_verified(Instant::now());
        self.form = AuthForm::default();
        self.screen = Screen::Main;
        self.mode = Mode::Normal;
        let hosts = {
            let store = self.store.clone();
            Task::perform(async move { load_hosts(&store).await }, |r| {
                Message::HostsLoaded(r.map_err(|e| e.to_string()))
            })
        };
        let keymap = {
            let store = self.store.clone();
            Task::perform(
                async move {
                    store
                        .query(|db| {
                            let s = db.settings();
                            Ok(LoadedSettings {
                                leader: s.get("leader")?,
                                keymap: s.get("keymap")?,
                                recent: s.get(palette::RECENT_SETTING)?,
                                theme: s.get("theme")?,
                                reauth: s.get(reauth::SETTING)?,
                            })
                        })
                        .await
                },
                |r| Message::SettingsLoaded(r.map_err(|e| e.to_string())),
            )
        };
        Task::batch([hosts, keymap, self.reload_keys(), self.reload_identities()])
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
        let mut nav = column![text("tern").size(20), space().height(12)].spacing(4);
        for (n, section) in Section::ALL.into_iter().enumerate() {
            let active = section == self.section;
            let label = format!(
                "{} {}  {}",
                if active { ">" } else { " " },
                n + 1,
                section.label()
            );
            let goto = match section {
                Section::Hosts => Action::GoHosts,
                Section::Keys => Action::GoKeys,
                Section::Identities => Action::GoIdentities,
                Section::Tunnels => Action::GoTunnels,
            };
            nav = nav.push(
                mouse_area(
                    container(text(label).size(14))
                        .width(Fill)
                        .padding([2, 6])
                        .style(move |theme: &Theme| selected_row_style(theme, active)),
                )
                .on_press(Message::Dispatch(goto)),
            );
        }
        let sidebar = container(nav)
            .padding(12)
            .width(200)
            .height(Fill)
            .style(pane_frame(self.pane == Pane::Sidebar));

        let filter = text_input("Filter (/)", &self.list().filter)
            .id(FILTER_ID)
            .on_input(Message::FilterChanged)
            .on_submit(Message::FilterSubmitted);
        let list = list::view(self.list(), self.pane == Pane::List);
        let list_pane =
            container(column![text(self.section.label()).size(16), filter, list,].spacing(10))
                .padding(12)
                .width(Fill)
                .height(Fill)
                .style(pane_frame(self.pane == Pane::List));

        let detail_text = match self.list().selected_item() {
            Some(item) if self.section == Section::Hosts && item.id.is_some() => item
                .id
                .as_deref()
                .and_then(|id| self.host_data.host_detail(id))
                .unwrap_or_else(|| item.label.clone()),
            Some(item) if self.section == Section::Keys && item.id.is_some() => item
                .id
                .as_deref()
                .and_then(|id| self.keys_data.detail(id))
                .unwrap_or_else(|| item.label.clone()),
            Some(item) if self.section == Section::Identities && item.id.is_some() => item
                .id
                .as_deref()
                .and_then(|id| {
                    let mut t = self.ids_data.detail(id, &self.keys_data.keys)?;
                    if let Some((rid, pw)) = &self.revealed
                        && rid == id
                    {
                        t.push_str(&format!(
                            "\n\nRevealed password (Esc hides):\n{}",
                            pw.as_str()
                        ));
                    }
                    Some(t)
                })
                .unwrap_or_else(|| item.label.clone()),
            Some(item) => format!(
                "{}\n\n(details arrive with the {} screen, Sprint 3)",
                item.label,
                self.section.label().to_lowercase()
            ),
            None => "Nothing selected".to_owned(),
        };
        let detail = container(text(detail_text).size(14))
            .padding(12)
            .width(320)
            .height(Fill)
            .style(pane_frame(self.pane == Pane::Detail));

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

        let base = column![row![sidebar, list_pane, detail].height(Fill), status_bar];
        if let Some(form) = &self.host_form {
            return iced::widget::stack![base, host_form::view(form)].into();
        }
        if let Some(p) = &self.prompt {
            return iced::widget::stack![base, prompt::view(p)].into();
        }
        if let Some(r) = &self.reauth {
            return iced::widget::stack![base, reauth::view(r)].into();
        }
        if let Some(f) = &self.key_form {
            return iced::widget::stack![base, key_form::view(f)].into();
        }
        if let Some(f) = &self.identity_form {
            return iced::widget::stack![base, identity_form::view(f)].into();
        }
        match (self.modals.top(), self.mode) {
            (Some(m), _) => iced::widget::stack![base, modal::view(m, &self.keymap)].into(),
            (None, Mode::Palette) => {
                iced::widget::stack![base, palette::view(&self.palette)].into()
            }
            (None, _) => base.into(),
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
        if self.dark { Theme::Dark } else { Theme::Light }
    }
}

/// Container style for a pane: a visible focus ring while it has keyboard focus.
fn pane_frame(focused: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| {
        let palette = theme.extended_palette();
        container::Style {
            border: Border {
                color: if focused {
                    palette.primary.strong.color
                } else {
                    palette.background.strong.color
                },
                width: if focused { 2.0 } else { 1.0 },
                radius: 4.0.into(),
            },
            ..container::Style::default()
        }
    }
}

fn selected_row_style(theme: &Theme, selected: bool) -> container::Style {
    if !selected {
        return container::Style::default();
    }
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.primary.weak.color.into()),
        text_color: Some(palette.primary.weak.text),
        border: Border::default().rounded(4),
        ..container::Style::default()
    }
}

/// Bundled monospace font (JetBrains Mono, SIL OFL 1.1; licence in `assets/fonts/OFL.txt`).
pub const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
pub const MONO: Font = Font::with_name("JetBrains Mono");

/// Create `path` exclusively (never overwrites) with owner-only permissions.
fn write_private_file(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(contents.as_bytes())
}

/// `~/` -> home directory; anything else unchanged.
fn expand_home(path: &str) -> std::path::PathBuf {
    match (path.strip_prefix("~/"), directories::UserDirs::new()) {
        (Some(rest), Some(dirs)) => dirs.home_dir().join(rest),
        _ => std::path::PathBuf::from(path),
    }
}

async fn load_identities(store: &Store) -> tern_store::Result<IdentitiesData> {
    let identities = store.list_identities().await?;
    let mut with_password = std::collections::HashSet::new();
    for i in &identities {
        if store.identity_has_password(i.id).await? {
            with_password.insert(i.id);
        }
    }
    Ok(IdentitiesData {
        identities,
        with_password,
    })
}

async fn load_keys(store: &Store) -> tern_store::Result<KeysData> {
    Ok(KeysData {
        keys: store.list_keys().await?,
        created: store.key_created_at().await?,
    })
}

/// Load everything the Hosts section shows.
async fn load_hosts(store: &Store) -> tern_store::Result<HostsData> {
    Ok(HostsData {
        hosts: store.list_hosts().await?,
        groups: store.list_groups().await?,
        identities: store.list_identities().await?,
        last_connected: store.host_last_connected().await?,
    })
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

    /// The exit demo: Ctrl+K, type "lock", Enter.
    #[test]
    fn palette_lock_flow() {
        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::OpenPalette));
        assert_eq!(s.mode, Mode::Palette);
        let _ = s.update(Message::PaletteQueryChanged("lock".into()));
        let _ = s.update(Message::Dispatch(Action::PaletteRun));
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.palette.recent(), ["vault.lock"]);
        // The lock itself completes asynchronously.
        let _ = s.update(Message::Locked(Ok(())));
        assert_eq!(s.screen, Screen::Unlock);
    }

    #[test]
    fn running_a_host_entry_does_not_crash_and_closes_the_palette() {
        let mut s = main_state();
        let host = tern_core::Host::new("prod-db", "db.example.com");
        let _ = s.update(Message::HostsLoaded(Ok(HostsData {
            hosts: vec![host],
            ..HostsData::default()
        })));
        let _ = s.update(Message::Dispatch(Action::OpenPalette));
        let _ = s.update(Message::PaletteQueryChanged("prod-db".into()));
        let _ = s.update(Message::Dispatch(Action::PaletteRun));
        assert_eq!(s.mode, Mode::Normal);
        assert!(s.toasts.current().is_some());
    }

    #[test]
    fn exported_key_files_are_private_and_never_overwritten() {
        let path = std::env::temp_dir().join(format!("tern-export-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        write_private_file(&path, "secret").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
        assert!(write_private_file(&path, "other").is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reauth_is_required_until_verified_and_esc_cancels() {
        let mut s = main_state();
        s.gate.reset();
        let _ = s.update(Message::Dispatch(Action::GoKeys));
        s.lists[1].replace_items(vec![
            crate::list::Item::leaf("k", 0).with_id(tern_core::KeyId::new().to_string()),
        ]);
        let _ = s.update(Message::Dispatch(Action::ExportKey));
        assert!(s.reauth.is_some() && s.mode == Mode::Insert);
        let _ = s.update(Message::Dispatch(Action::Back));
        assert!(s.reauth.is_none() && s.mode == Mode::Normal);
    }

    #[test]
    fn theme_toggles_between_dark_and_light() {
        let mut s = main_state();
        assert_eq!(s.theme(), Theme::Dark);
        let _ = s.update(Message::Dispatch(Action::ToggleTheme));
        assert_eq!(s.theme(), Theme::Light);
        let _ = s.update(Message::Dispatch(Action::ToggleTheme));
        assert_eq!(s.theme(), Theme::Dark);
    }

    #[test]
    fn f1_toggles_the_cheat_sheet() {
        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::CheatSheet));
        assert_eq!(s.mode, Mode::Modal);
        let _ = s.update(Message::Dispatch(Action::CheatSheet));
        assert_eq!(s.mode, Mode::Normal);
        let _ = s.update(Message::Dispatch(Action::CheatSheet));
        let _ = s.update(Message::Dispatch(Action::Back));
        assert_eq!(s.mode, Mode::Normal);
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
