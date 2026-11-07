//! The iced application: state, messages, update and view.

use std::time::Instant;

use iced::widget::{column, container, text};
use iced::{Element, Fill, Font, Subscription, Task, Theme};
use tern_store::Store;

use crate::action::ModeMask;
use crate::action::{Action, Section};
use crate::auth::{self, AuthFailure, AuthForm, CONFIRM_ID, PASSWORD_ID, Sensitive, retry_delay};
use crate::autolock::{self, AutoLock};
use crate::host_form::{self, AuthKind, Choice, Field, HostForm, Picker};
use crate::hosts_view::{HostSort, HostsData};
use crate::io_form::{self, IoForm, IoKind, Preview};
use crate::key_form::{self, KeyForm, KeyMode};
use crate::keymap::{self, Chord, Context, Keymap, Matcher, Outcome};
use crate::keys_view::KeysData;
use crate::list::{self, ListState};
use crate::modal::{self, Modal, ModalStack, Pending};
use crate::palette::{self, HostRef, Palette, Target};
use crate::prompt::{self, Prompt, PromptKind, PromptOutcome};
use crate::reauth::{self, Gate, ReauthPrompt, SensitiveOp};
use crate::sessions::{self, AppKnownHosts, ConnTarget, Phase, SessionId, SessionView, Spawned};
use crate::settings::{self, SettingsMsg, SettingsScreen};
use crate::sftp_form::{self, SftpForm};
use crate::term_settings::{self, TermSettings};
use crate::toast::{ToastKind, Toasts};
use crate::tunnel_form::{self, TField, TunnelForm};
use crate::tunnel_run::{self, TunState, TunStatus, TunnelMgr};
use crate::tunnels_view::TunnelsData;
use tern_term::widget::TermEvent;
use tern_term::{Motion, SelectKind};

mod io_ops;
mod settings_ops;
mod sftp_ops;
mod shell;

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
    lists: [ListState; 3],
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
    /// A select drop-down was just closed by Esc: swallow the next Esc key press.
    select_esc: bool,
    /// A prompt was just submitted with Enter: swallow that same key press.
    swallow_enter: bool,
    keys_data: KeysData,
    key_form: Option<KeyForm>,
    io_form: Option<IoForm>,
    sftp_form: Option<SftpForm>,
    settings: Option<SettingsScreen>,
    /// User keymap overrides (`action id -> keys`), mirrored from the `keymap` setting.
    keymap_overrides: std::collections::BTreeMap<String, Vec<String>>,
    leader_setting: Option<String>,
    tunnels_data: TunnelsData,
    tunnel_form: Option<TunnelForm>,
    pool: tern_ssh::ConnectionPool,
    tunnel_mgr: Option<TunnelMgr>,
    tunnel_status: std::collections::HashMap<tern_core::TunnelId, TunStatus>,
    /// Set after unlock; cleared once the `auto_start` tunnels have been started.
    autostart_pending: bool,
    tunnels_loaded: bool,
    /// Incremented per password copy so only the latest copy's timer clears the clipboard.
    clip_token: u64,
    /// A copied password may still be on the clipboard (cleared on lock).
    secret_on_clipboard: bool,
    /// Process start (approximately), for the startup-time log line.
    started: Instant,
    autolock: AutoLock,
    gate: Gate,
    reauth: Option<ReauthPrompt>,
    /// A stored host password the user explicitly revealed: `(host id, password)`.
    revealed: Option<(String, Sensitive)>,
    prompt: Option<Prompt>,
    palette: Palette,
    /// Open terminal tabs; `active` is the focused one (`None` = the host list).
    tabs: Vec<SessionView>,
    active: Option<usize>,
    next_session: SessionId,
    term_settings: TermSettings,
    /// Grid size of the last terminal widget, used for new sessions' PTY.
    term_size: (usize, usize),
    /// Cursor blink phase.
    cursor_on: bool,
}

/// Settings read from the `settings` table after unlocking.
#[derive(Debug, Clone)]
pub struct LoadedSettings {
    leader: Option<String>,
    keymap: Option<String>,
    recent: Option<String>,
    theme: Option<String>,
    reauth: Option<String>,
    term: Vec<(String, String)>,
    lock: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Does nothing (inert widget callbacks).
    Noop,
    /// Esc closed a form select's drop-down; the Esc key press that follows must not also cancel the form.
    SelectEscaped,
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
    Key(Chord, keymap::RawKey),
    /// `(leader, keymap overrides)` JSON from the settings table.
    SettingsLoaded(Result<LoadedSettings, String>),
    PaletteQueryChanged(String),
    SettingSaved(Result<(), String>),
    /// Mouse click/scroll; only resets the idle timer.
    Activity,
    Settings(SettingsMsg),
    AutoLockTick(std::time::SystemTime),
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
    FormAuth(AuthKind),
    FormPassword(Sensitive),
    FormClearPassword(bool),
    /// Enter in a form field.
    FormSave,
    HostSaved(Result<String, String>),
    KeysLoaded(Result<KeysData, String>),
    TunnelsLoaded(Result<Vec<tern_core::Tunnel>, String>),
    TunnelMgrReady(TunnelMgr),
    TunnelResolved(
        tern_core::TunnelId,
        Result<Box<tern_ssh::ConnectParams>, String>,
    ),
    Tunnel(tern_ssh::manager::TunnelEvent),
    TunnelEdit(TField, String),
    TunnelHost(Choice),
    TunnelKind(Choice),
    TunnelAuto(bool),
    TunnelSave,
    TunnelSaved(Result<String, String>),
    KeyPathChanged(String),
    KeyLabelChanged(String),
    KeyPassphraseChanged(Sensitive),
    KeyKindPicked(tern_ssh::keys::KeyKind),
    KeyPrivateEdit(iced::widget::text_editor::Action),
    KeyPublicChanged(String),
    KeySubmit,
    KeySaved(Result<String, String>),
    IoPath(String),
    IoScope(String),
    IoPassword(Sensitive),
    IoConfirm(Sensitive),
    IoExtra(String),
    IoSubmit,
    IoPreviewed(Result<Preview, String>),
    IoDone(Result<String, String>),
    SftpRemote(String),
    SftpLocal(String),
    SftpDirection(crate::sftp_form::Direction),
    SftpSubmit,
    SftpDone(Result<String, String>),
    IoScheduleLoaded(String, bool, u32),
    AutoBackupDue(bool),
    ClipboardRead(Option<String>),
    ClipboardClear(u64),
    ReauthPassword(Sensitive),
    ReauthSubmit,
    ReauthDone(Result<(), AuthFailure>),
    PasswordRevealed(Result<Option<Sensitive>, String>),
    PromptChanged(String),
    PromptSubmit,
    /// A group/move operation finished; the string is a toast message.
    GroupsChanged(Result<String, String>),
    /// The session task for this id started (or could not, e.g. no username).
    SessionSpawned(SessionId, Result<Spawned, String>),
    /// An event from the session actor.
    Session(SessionId, tern_ssh::SessionEvent),
    SessionInput(Sensitive),
    /// Enter in the session input line.
    SessionSubmit,
    SessionTouched(Result<(), String>),
    /// Select a tab (`SelectHome` = the host list).
    SelectTab(usize),
    SelectHome,
    /// Event from a tab's terminal widget.
    Term(SessionId, TermEvent),
    /// Debounced PTY resize is due.
    ResizeFlush(SessionId, u64),
    /// Visual bell finished.
    FlashEnd(SessionId),
    BlinkTick,
    /// Clipboard contents for a terminal paste.
    Pasted(Option<String>),
    SearchQuery(String),
    SearchNext,
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
            select_esc: false,
            swallow_enter: false,
            keys_data: KeysData::default(),
            tunnels_data: TunnelsData::default(),
            tunnel_form: None,
            pool: tern_ssh::ConnectionPool::new(),
            tunnel_mgr: None,
            tunnel_status: std::collections::HashMap::new(),
            autostart_pending: false,
            tunnels_loaded: false,
            key_form: None,
            io_form: None,
            sftp_form: None,
            settings: None,
            keymap_overrides: std::collections::BTreeMap::new(),
            leader_setting: None,
            clip_token: 0,
            secret_on_clipboard: false,
            started: Instant::now(),
            autolock: AutoLock::new(std::time::SystemTime::now()),
            gate: Gate::default(),
            reauth: None,
            revealed: None,
            prompt: None,
            palette: Palette::default(),
            tabs: Vec::new(),
            active: None,
            next_session: 0,
            term_settings: TermSettings::default(),
            term_size: (80, 24),
            cursor_on: true,
        };
        (state, check)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if matches!(message, Message::Key(..) | Message::Activity) {
            self.autolock.touch(std::time::SystemTime::now());
        }
        match message {
            Message::Activity => Task::none(),
            Message::Settings(m) => self.on_settings(m),
            Message::AutoLockTick(now) => {
                if self.screen == Screen::Main && self.autolock.should_lock(now) {
                    tracing::info!("auto-lock: idle timeout");
                    return self.dispatch(Action::LockVault);
                }
                Task::none()
            }
            Message::VaultChecked(Ok(exists)) => {
                tracing::info!(
                    elapsed_ms = self.started.elapsed().as_millis() as u64,
                    "first screen ready"
                );
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
                self.tunnels_data.hosts = self.host_data.hosts.clone();
                self.rebuild_tunnels_list();
                Task::none()
            }
            Message::SpinnerTick => {
                self.form.spinner_frame = self.form.spinner_frame.wrapping_add(1);
                Task::none()
            }
            Message::FocusNext => iced::widget::operation::focus_next(),
            Message::FocusPrevious => iced::widget::operation::focus_previous(),
            Message::Dispatch(action) => self.dispatch(action),
            Message::Noop => Task::none(),
            Message::SelectEscaped => {
                self.select_esc = true;
                Task::none()
            }
            Message::Key(chord, raw) => {
                if self.screen != Screen::Main {
                    return Task::none();
                }
                if std::mem::take(&mut self.select_esc) && chord.to_text() == "esc" {
                    return Task::none();
                }
                if std::mem::take(&mut self.swallow_enter) && chord.to_text() == "enter" {
                    return Task::none();
                }
                if let Some(task) = self.settings_key(&chord) {
                    return task;
                }
                let terminal = self.terminal_focused();
                if terminal
                    && self.tab().is_some_and(|t| t.term.in_copy_mode())
                    && !is_app_chord(&chord)
                {
                    return self.copy_mode_key(&chord);
                }
                let ctx = Context {
                    mode: self.mode.mask(),
                    terminal,
                };
                match self.matcher.feed(&self.keymap, ctx, chord, Instant::now()) {
                    Outcome::Action(action) => self.dispatch(action),
                    Outcome::Pending => Task::none(),
                    Outcome::Unhandled => {
                        if terminal {
                            self.send_raw_key(&raw);
                        }
                        Task::none()
                    }
                }
            }
            Message::SettingSaved(result) => {
                if let Err(e) = result {
                    tracing::warn!(error = %e, "could not save a setting");
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
                        self.keymap_overrides = loaded
                            .keymap
                            .as_deref()
                            .and_then(|j| serde_json::from_str(j).ok())
                            .unwrap_or_default();
                        self.leader_setting = loaded.leader.clone();
                        self.keymap =
                            Keymap::build(loaded.leader.as_deref(), loaded.keymap.as_deref());
                        self.gate.set_minutes_from_setting(loaded.reauth.as_deref());
                        for (k, v) in &loaded.term {
                            self.term_settings.apply(k, v);
                        }
                        for (k, v) in &loaded.lock {
                            self.autolock.apply(k, v);
                        }
                        for t in &mut self.tabs {
                            t.term.set_scrollback(self.term_settings.scrollback);
                            t.refresh();
                        }
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
            Message::FormAuth(auth) => {
                if let Some(form) = &mut self.host_form {
                    form.set_auth(auth);
                }
                Task::none()
            }
            Message::FormPassword(pw) => {
                if let Some(form) = &mut self.host_form {
                    form.set_password(pw);
                }
                Task::none()
            }
            Message::FormClearPassword(b) => {
                if let Some(form) = &mut self.host_form {
                    form.set_clear_password(b);
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
                self.toast(ToastKind::Info, "This host has no stored password");
                Task::none()
            }
            Message::PasswordRevealed(Err(e)) => {
                self.revealed = None;
                tracing::error!(error = %e, "could not reveal password");
                self.toast(ToastKind::Error, "Could not read the password");
                Task::none()
            }
            Message::TunnelsLoaded(r) => {
                if let Err(e) = &r {
                    tracing::error!(error = %e, "could not load tunnels");
                    self.toast(ToastKind::Error, "Could not load tunnels");
                }
                self.tunnels_loaded = r.is_ok();
                self.tunnels_data.tunnels = r.unwrap_or_default();
                self.rebuild_tunnels_list();
                self.maybe_autostart()
            }
            Message::TunnelMgrReady(mgr) => {
                self.tunnel_mgr = Some(mgr);
                self.maybe_autostart()
            }
            Message::TunnelResolved(id, Ok(params)) => {
                self.send_tunnel_start(id, *params);
                Task::none()
            }
            Message::TunnelResolved(id, Err(e)) => {
                self.set_tunnel_error(id, e, false);
                Task::none()
            }
            Message::Tunnel(event) => self.on_tunnel_event(event),
            Message::TunnelEdit(field, value) => self.edit_tunnel_form(|f| f.set(field, value)),
            Message::TunnelHost(c) => self.edit_tunnel_form(|f| f.pick_host(c)),
            Message::TunnelKind(c) => self.edit_tunnel_form(|f| f.pick_kind(c)),
            Message::TunnelAuto(on) => self.edit_tunnel_form(|f| f.set_auto_start(on)),
            Message::TunnelSave => self.save_tunnel_form(),
            Message::TunnelSaved(Ok(label)) => {
                self.tunnel_form = None;
                self.mode = Mode::Normal;
                self.toast(ToastKind::Success, format!("Saved '{label}'"));
                self.reload_tunnels()
            }
            Message::TunnelSaved(Err(e)) => {
                if let Some(form) = &mut self.tunnel_form {
                    form.error = Some(format!("Could not save: {e}"));
                } else {
                    self.toast(ToastKind::Error, format!("Could not save: {e}"));
                }
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
                self.host_data.keys = self.keys_data.keys.clone();
                Task::none()
            }
            Message::ClipboardClear(token) => {
                if token == self.clip_token {
                    self.secret_on_clipboard = false;
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
            Message::IoPath(v) => self.edit_io(|f| f.path = v),
            Message::IoScope(v) => self.edit_io(|f| f.scope = v),
            Message::IoPassword(v) => self.edit_io(|f| f.password = v),
            Message::IoConfirm(v) => self.edit_io(|f| f.confirm = v),
            Message::IoExtra(v) => self.edit_io(|f| f.extra = v),
            Message::IoSubmit => self.submit_io(),
            Message::IoPreviewed(r) => self.io_previewed(r),
            Message::IoDone(r) => self.io_done(r),
            Message::SftpRemote(v) => self.edit_sftp(|f| f.remote = v),
            Message::SftpLocal(v) => self.edit_sftp(|f| f.local = v),
            Message::SftpDirection(v) => self.edit_sftp(|f| f.direction = v),
            Message::SftpSubmit => self.submit_sftp(),
            Message::SftpDone(r) => self.sftp_done(r),
            Message::IoScheduleLoaded(dir, on, keep) => self.edit_io(|f| {
                if f.kind == IoKind::BackupSchedule && f.path.is_empty() && f.extra.is_empty() {
                    f.path = dir;
                    f.scope = if on { "on" } else { "off" }.into();
                    f.extra = keep.to_string();
                }
            }),
            Message::AutoBackupDue(true) if !self.any_form_open() => {
                self.open_io(IoKind::AutoBackup)
            }
            Message::AutoBackupDue(_) => Task::none(),
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
                } else {
                    self.toast(ToastKind::Error, format!("Could not save: {e}"));
                }
                Task::none()
            }
            Message::ItemDeleted(Ok(label)) => {
                self.toast(ToastKind::Success, format!("Deleted '{label}'"));
                Task::batch([
                    self.reload_hosts(),
                    self.reload_keys(),
                    self.reload_tunnels(),
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
                self.tunnel_form = None;
                self.tunnels_data = TunnelsData::default();
                if !self.autolock.keep_sessions {
                    self.stop_all_tunnels();
                }
                self.tunnels_loaded = false;
                self.autostart_pending = false;
                self.prompt = None;
                self.key_form = None;
                self.io_form = None;
                self.sftp_form = None;
                self.settings = None;
                self.reauth = None;
                self.revealed = None;
                self.gate.reset();
                self.keys_data = KeysData::default();
                self.modals.clear();
                if self.autolock.keep_sessions {
                    // Sessions keep running behind the lock screen; only drop UI state.
                    self.mode = Mode::Normal;
                    self.matcher.reset();
                } else {
                    self.close_all_tabs();
                }
                iced::widget::operation::focus(PASSWORD_ID)
            }
            Message::SessionSpawned(id, result) => self.on_session_spawned(id, result),
            Message::Session(id, event) => self.on_session_event(id, event),
            Message::SessionInput(v) => {
                if let Some(s) = self.tab_mut() {
                    s.input = v;
                }
                Task::none()
            }
            Message::SelectTab(i) => self.activate(i),
            Message::SelectHome => self.go_home(),
            Message::Term(id, ev) => self.on_term_event(id, ev),
            Message::ResizeFlush(id, gen_) => {
                if let Some(s) = self
                    .tabs
                    .iter()
                    .find(|t| t.id == id && t.resize_gen == gen_)
                    && let Some(h) = &s.handle
                {
                    h.try_send(tern_ssh::SessionCommand::Resize {
                        cols: s.term.cols() as u32,
                        rows: s.term.rows() as u32,
                    });
                }
                Task::none()
            }
            Message::FlashEnd(id) => {
                if let Some(s) = self.tabs.iter_mut().find(|t| t.id == id) {
                    s.flash = false;
                    s.cache.clear();
                }
                Task::none()
            }
            Message::BlinkTick => {
                self.cursor_on = !self.cursor_on;
                Task::none()
            }
            Message::Pasted(text) => match text {
                Some(t) => self.paste_text(t),
                None => Task::none(),
            },
            Message::SearchQuery(q) => {
                if let Some(s) = self.tab_mut() {
                    s.term.search(&q);
                    s.search = Some(q);
                    s.refresh();
                }
                Task::none()
            }
            Message::SearchNext => self.search_step(true),
            Message::SessionSubmit => self.submit_session_input(),
            Message::SessionTouched(result) => {
                if let Err(e) = result {
                    tracing::warn!(error = %e, "could not record last connection time");
                }
                Task::none()
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
        if self.active.is_some() && !tab_action_allowed(action) {
            return Task::none();
        }
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
            Action::RevealStored if self.section == Section::Hosts => {
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
            Action::TermCopy | Action::FormCopy if self.terminal_tab() => self.term_copy(),
            Action::TermCopy => Task::none(),
            Action::FormReveal | Action::FormCopy => self.host_password_action(action),
            Action::Add | Action::Edit if self.section == Section::Tunnels => {
                self.open_tunnel_form(action == Action::Edit)
            }
            Action::Add | Action::Edit => self.open_host_form(action == Action::Edit),
            Action::Settings => self.open_settings(),
            Action::FormSave if self.settings.is_some() => self.on_settings(SettingsMsg::Save),
            Action::FormSave if self.tunnel_form.is_some() => self.save_tunnel_form(),
            Action::FormSave if self.key_form.is_some() => self.submit_key_form(),
            Action::FormSave if self.io_form.is_some() => self.submit_io(),
            Action::FormSave if self.sftp_form.is_some() => self.submit_sftp(),
            Action::CopyFile => self.open_sftp(),
            Action::ExportBackup => {
                self.request_sensitive(SensitiveOp::OpenIo(IoKind::ExportBackup))
            }
            Action::ExportPlain => self.request_sensitive(SensitiveOp::OpenIo(IoKind::ExportPlain)),
            Action::ImportBackup => self.open_io(IoKind::ImportBackup),
            Action::ImportSshConfig => self.open_io(IoKind::ImportSshConfig),
            Action::ImportSshKeys => self.open_io(IoKind::ImportSshKeys),
            Action::ExportSshConfig => self.open_io(IoKind::ExportSshConfig),
            Action::ScheduleBackups => self.open_io(IoKind::BackupSchedule),
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
            Action::Confirm | Action::Cancel | Action::ReplaceHostKey => {
                let modal = self.modals.pop();
                self.sync_modal_mode();
                match (action, modal) {
                    (Action::Confirm, Some(Modal::Confirm { then, .. })) => self.run_pending(then),
                    (
                        Action::Confirm | Action::Cancel | Action::ReplaceHostKey,
                        Some(modal @ Modal::HostKey { .. }),
                    ) => self.decide_host_key(action, modal),
                    _ => Task::none(),
                }
            }
            Action::Open if self.active.is_some() => {
                let Some(s) = self.tab() else {
                    return Task::none();
                };
                match s.phase {
                    Phase::Connected => {
                        self.mode = Mode::Insert;
                        self.matcher.reset();
                        Task::none()
                    }
                    Phase::Auth { .. } => {
                        self.mode = Mode::Insert;
                        self.matcher.reset();
                        iced::widget::operation::focus(sessions::SESSION_INPUT_ID)
                    }
                    _ => Task::none(),
                }
            }
            Action::ToggleTunnels => self.toggle_tunnels_action(),
            Action::Open if self.section == Section::Tunnels => self.toggle_tunnels_action(),
            Action::Open if self.section == Section::Hosts => {
                let Some(item) = self.list().selected_item() else {
                    return Task::none();
                };
                match (item.group, item.id.clone()) {
                    (Some(expanded), _) => {
                        if expanded {
                            self.list_mut().collapse();
                        } else {
                            self.list_mut().expand();
                        }
                        self.scroll_list(false)
                    }
                    (None, Some(id)) => self.start_session(ConnTarget::Saved(id)),
                    (None, None) => Task::none(),
                }
            }
            Action::CloseTab => self.request_close_tab(),
            Action::Reconnect => match self.tab() {
                Some(s) if !s.is_live() => {
                    let target = s.target.clone();
                    let name = s.custom_name.clone();
                    let idx = self.active;
                    self.start_session_at(target, idx, name)
                }
                Some(_) => Task::none(),
                None => {
                    self.toast(ToastKind::Info, "No session to reconnect");
                    Task::none()
                }
            },
            Action::NewTab => self.go_home(),
            Action::JumpHome => self.go_home(),
            Action::NextTab => self.cycle_tab(true),
            Action::PrevTab => self.cycle_tab(false),
            Action::JumpTab1
            | Action::JumpTab2
            | Action::JumpTab3
            | Action::JumpTab4
            | Action::JumpTab5
            | Action::JumpTab6
            | Action::JumpTab7
            | Action::JumpTab8
            | Action::JumpTab9 => match action.jump_index() {
                Some(i) if i < self.tabs.len() => self.activate(i),
                _ => Task::none(),
            },
            Action::RenameTab => match self.tab() {
                Some(t) => {
                    let (id, name) = (t.id, t.display_name().to_owned());
                    self.open_prompt(Prompt::new(PromptKind::RenameTab(id), name))
                }
                None => Task::none(),
            },
            Action::DuplicateTab => match self.tab() {
                Some(t) => {
                    let target = t.target.clone();
                    self.start_session_at(target, None, None)
                }
                None => Task::none(),
            },
            Action::TermPaste => {
                if self.terminal_tab() {
                    iced::clipboard::read().map(Message::Pasted)
                } else {
                    Task::none()
                }
            }
            Action::TermSearch => self.open_search(),
            Action::SearchNext => self.search_step(true),
            Action::SearchPrev => self.search_step(false),
            Action::CopyMode => {
                if self.terminal_tab() {
                    if let Some(t) = self.tab_mut() {
                        t.term.copy_enter();
                        t.refresh();
                    }
                    self.mode = Mode::Insert;
                }
                Task::none()
            }
            Action::ScrollPageUp | Action::ScrollPageDown => {
                let up = action == Action::ScrollPageUp;
                if let Some(t) = self.tab_mut() {
                    if t.snap.modes.alt_screen {
                        let bytes: &[u8] = if up { b"\x1b[5~" } else { b"\x1b[6~" };
                        if let Some(h) = &t.handle {
                            h.try_send(tern_ssh::SessionCommand::Write(bytes.to_vec().into()));
                        }
                    } else {
                        t.term.scroll_pages(if up { 1 } else { -1 });
                        t.refresh();
                    }
                }
                Task::none()
            }
            Action::FontBigger | Action::FontSmaller | Action::FontReset => {
                let size = &mut self.term_settings.font_size;
                *size = match action {
                    Action::FontBigger => *size + 1.0,
                    Action::FontSmaller => *size - 1.0,
                    _ => term_settings::DEFAULT_FONT_SIZE,
                }
                .clamp(term_settings::MIN_FONT_SIZE, term_settings::MAX_FONT_SIZE);
                self.term_settings_changed(term_settings::FONT_SIZE)
            }
            Action::CycleScheme => {
                self.term_settings.scheme = self.term_settings.scheme.next();
                self.toast(
                    ToastKind::Info,
                    format!("Terminal colors: {}", self.term_settings.scheme),
                );
                self.term_settings_changed(term_settings::SCHEME)
            }
            Action::CycleCursor => {
                self.term_settings.cursor = self.term_settings.cursor.next();
                self.toast(
                    ToastKind::Info,
                    format!("Cursor: {}", self.term_settings.cursor.id()),
                );
                self.term_settings_changed(term_settings::CURSOR)
            }
            Action::ToggleBell => {
                self.term_settings.visual_bell = !self.term_settings.visual_bell;
                let on = self.term_settings.visual_bell;
                self.toast(ToastKind::Info, format!("Visual bell {}", on_off(on)));
                self.term_settings_changed(term_settings::VISUAL_BELL)
            }
            Action::TogglePasteWarn => {
                self.term_settings.confirm_paste = !self.term_settings.confirm_paste;
                let on = self.term_settings.confirm_paste;
                self.toast(
                    ToastKind::Info,
                    format!("Multi-line paste warning {}", on_off(on)),
                );
                self.term_settings_changed(term_settings::CONFIRM_PASTE)
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
                    self.mode = self.resting_mode();
                    Task::none()
                }
                _ => {
                    self.palette.open(&self.keymap, &self.hosts);
                    self.palette.add_tunnels(&self.tunnel_refs());
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
                if !self.autolock.keep_sessions {
                    self.stop_all_tunnels();
                    self.close_all_tabs();
                }
                let store = self.store.clone();
                let lock = Task::perform(async move { store.lock().await }, |r| {
                    Message::Locked(r.map_err(|e| e.to_string()))
                });
                if self.secret_on_clipboard {
                    self.secret_on_clipboard = false;
                    return Task::batch([iced::clipboard::write(String::new()), lock]);
                }
                lock
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
        if let Some(form) = &mut self.tunnel_form {
            if form.request_close() {
                self.tunnel_form = None;
                return self.leave_insert();
            }
            return Task::none();
        }
        if self.io_form.as_ref().is_some_and(|f| !f.busy) {
            // From a preview, Esc goes back to the input stage; otherwise it closes the dialog.
            if let Some(f) = &mut self.io_form
                && f.preview.is_some()
            {
                f.preview = None;
                f.extra.clear();
                return Task::none();
            }
            self.io_form = None;
            return self.leave_insert();
        }
        if self.sftp_form.as_ref().is_some_and(|f| !f.busy) {
            self.sftp_form = None;
            return self.leave_insert();
        }
        if self.settings.is_some() {
            return self.settings_back();
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
        if self.active.is_some() && self.modals.is_empty() {
            return self.back_in_tab();
        }
        match self.mode {
            Mode::Modal => {
                let modal = self.modals.pop();
                self.sync_modal_mode();
                match modal {
                    Some(m @ Modal::HostKey { .. }) => self.decide_host_key(Action::Cancel, m),
                    _ => Task::none(),
                }
            }
            Mode::Palette => {
                self.mode = self.resting_mode();
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
        self.mode = self.resting_mode();
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
            Target::Connect(host_id) => self.start_session(ConnTarget::Saved(host_id)),
            Target::ToggleTunnel(id) => self.toggle_tunnel_ids(&[id]),
            Target::Quick(q) => self.start_session(ConnTarget::Quick(q)),
            Target::SaveQuick(q) => self.save_quick_host(q),
        };
        Task::batch([save, run])
    }

    /// Open a new tab and connect to `target`.
    fn start_session(&mut self, target: ConnTarget) -> Task<Message> {
        self.start_session_at(target, None, None)
    }

    /// Connect to `target` in a new tab, or replace tab `replace` (reconnect).
    fn start_session_at(
        &mut self,
        target: ConnTarget,
        replace: Option<usize>,
        name: Option<String>,
    ) -> Task<Message> {
        let label = match &target {
            ConnTarget::Saved(host_id) => self
                .host_data
                .host(host_id)
                .map(|h| h.label.clone())
                .unwrap_or_else(|| host_id.clone()),
            ConnTarget::Quick(q) => q.display(),
        };
        self.next_session += 1;
        let id = self.next_session;
        let mut view = SessionView::new(
            id,
            target.clone(),
            label,
            self.term_size,
            self.term_settings.scrollback,
        );
        view.custom_name = name;
        let idx = match replace.filter(|i| *i < self.tabs.len()) {
            Some(i) => {
                self.tabs[i] = view;
                i
            }
            None => {
                self.tabs.push(view);
                self.tabs.len() - 1
            }
        };
        self.active = Some(idx);
        self.mode = Mode::Normal;
        self.matcher.reset();
        let store = self.store.clone();
        let pool = self.pool.clone();
        let (cols, rows) = self.term_size;
        Task::perform(
            async move {
                let mut params = sessions::resolve(&store, &target).await?;
                params.cols = cols as u32;
                params.rows = rows as u32;
                let known = std::sync::Arc::new(AppKnownHosts(store));
                let pool_key = match &target {
                    ConnTarget::Saved(id) => Some((pool, id.clone())),
                    ConnTarget::Quick(_) => None,
                };
                let (handle, rx) = tern_ssh::spawn_pooled(params, known, pool_key);
                Ok(Spawned {
                    handle,
                    source: sessions::EventSource::new(id, rx),
                })
            },
            move |r| Message::SessionSpawned(id, r),
        )
    }

    fn on_session_spawned(
        &mut self,
        id: SessionId,
        result: Result<Spawned, String>,
    ) -> Task<Message> {
        match result {
            Ok(spawned) => match self.tabs.iter_mut().find(|t| t.id == id) {
                Some(s) => {
                    s.handle = Some(spawned.handle);
                    s.source = Some(spawned.source);
                }
                // Closed while starting: stop the orphaned actor.
                None => {
                    spawned.handle.try_send(tern_ssh::SessionCommand::Close);
                }
            },
            Err(e) => {
                if let Some(i) = self.tab_index(id) {
                    self.remove_tab(i);
                    self.toast(ToastKind::Error, e);
                }
            }
        }
        Task::none()
    }

    fn tab(&self) -> Option<&SessionView> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    fn tab_mut(&mut self) -> Option<&mut SessionView> {
        let i = self.active?;
        self.tabs.get_mut(i)
    }

    fn tab_index(&self, id: SessionId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// The focused tab is a connected terminal.
    fn terminal_tab(&self) -> bool {
        self.tab()
            .is_some_and(|t| matches!(t.phase, Phase::Connected))
    }

    /// Keys go to the remote: connected terminal tab in Insert mode, nothing else capturing keys.
    fn terminal_focused(&self) -> bool {
        self.mode == Mode::Insert
            && self.modals.is_empty()
            && self.prompt.is_none()
            && self.settings.is_none()
            && self
                .tab()
                .is_some_and(|t| matches!(t.phase, Phase::Connected) && t.search.is_none())
    }

    /// Mode to return to after an overlay (palette, modal) closes.
    fn resting_mode(&self) -> Mode {
        if self.modals.is_empty() && self.terminal_tab() {
            Mode::Insert
        } else {
            Mode::Normal
        }
    }

    /// Focus tab `i`: terminals take keys immediately, auth prompts focus their field.
    fn activate(&mut self, i: usize) -> Task<Message> {
        if i >= self.tabs.len() {
            return Task::none();
        }
        self.active = Some(i);
        self.matcher.reset();
        self.cursor_on = true;
        self.tabs[i].cache.clear();
        match self.tabs[i].phase {
            Phase::Connected => {
                self.mode = Mode::Insert;
                iced::advanced::widget::operate(
                    iced::advanced::widget::operation::focusable::unfocus(),
                )
            }
            Phase::Auth { .. } => {
                self.mode = Mode::Insert;
                iced::widget::operation::focus(sessions::SESSION_INPUT_ID)
            }
            _ => {
                self.mode = Mode::Normal;
                Task::none()
            }
        }
    }

    /// Show the host list (to start another tab); tabs keep running.
    fn go_home(&mut self) -> Task<Message> {
        self.active = None;
        self.mode = Mode::Normal;
        self.matcher.reset();
        Task::none()
    }

    fn cycle_tab(&mut self, forward: bool) -> Task<Message> {
        let n = self.tabs.len();
        if n == 0 {
            return Task::none();
        }
        let next = match (self.active, forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        };
        self.activate(next)
    }

    /// `Esc` inside a tab: cancel a connect, close search, leave the terminal, or go home.
    fn back_in_tab(&mut self) -> Task<Message> {
        let insert = self.mode == Mode::Insert;
        let Some(t) = self.tab_mut() else {
            return Task::none();
        };
        if t.search.take().is_some() {
            t.refresh();
            return iced::advanced::widget::operate(
                iced::advanced::widget::operation::focusable::unfocus(),
            );
        }
        match t.phase {
            Phase::Connecting | Phase::HostKey | Phase::Auth { .. } => {
                let Some(i) = self.active else {
                    return Task::none();
                };
                self.remove_tab(i);
                Task::none()
            }
            Phase::Connected if insert => self.leave_insert(),
            _ => self.go_home(),
        }
    }

    /// `Ctrl+Shift+W`: close the focused tab; ask first while the session is live.
    fn request_close_tab(&mut self) -> Task<Message> {
        let Some(t) = self.tab() else {
            return Task::none();
        };
        if matches!(t.phase, Phase::Connected) {
            let (id, name) = (t.id, t.display_name().to_owned());
            self.modals.push(Modal::Confirm {
                title: format!("Close '{name}'?"),
                body: "The session is still connected and will be disconnected.".into(),
                then: Pending::CloseTab(id),
            });
            self.sync_modal_mode();
            return Task::none();
        }
        if let Some(i) = self.active {
            self.remove_tab(i);
        }
        Task::none()
    }

    /// Disconnect and drop tab `i`, then focus a neighbour (or the host list).
    fn remove_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(i);
        if let Some(h) = tab.handle {
            h.try_send(tern_ssh::SessionCommand::Close);
        }
        self.active = match self.active {
            Some(a) if a == i => (!self.tabs.is_empty()).then(|| i.min(self.tabs.len() - 1)),
            Some(a) if a > i => Some(a - 1),
            other => other,
        };
        self.modals.clear();
        self.matcher.reset();
        self.mode = if self.terminal_tab() {
            Mode::Insert
        } else {
            Mode::Normal
        };
    }

    fn close_all_tabs(&mut self) {
        for t in self.tabs.drain(..) {
            if let Some(h) = t.handle {
                h.try_send(tern_ssh::SessionCommand::Close);
            }
        }
        self.active = None;
        self.modals.clear();
        self.mode = Mode::Normal;
        self.matcher.reset();
    }

    fn on_session_event(&mut self, id: SessionId, event: tern_ssh::SessionEvent) -> Task<Message> {
        use tern_ssh::SessionEvent as E;
        let Some(idx) = self.tab_index(id) else {
            return Task::none();
        };
        let is_active = self.active == Some(idx);
        let visual_bell = self.term_settings.visual_bell;
        let s = &mut self.tabs[idx];
        match event {
            E::Connecting => s.phase = Phase::Connecting,
            E::HostKeyPrompt {
                host,
                port,
                key_type,
                fingerprint,
                changed_from,
            } => {
                s.phase = Phase::HostKey;
                self.active = Some(idx);
                self.modals.push(Modal::HostKey {
                    host,
                    port,
                    key_type,
                    fingerprint,
                    changed_from,
                });
                self.sync_modal_mode();
            }
            E::AuthPrompt {
                name,
                instructions,
                prompts,
            } => {
                s.begin_auth(name, instructions, prompts);
                self.active = Some(idx);
                self.mode = Mode::Insert;
                self.matcher.reset();
                return iced::widget::operation::focus(sessions::SESSION_INPUT_ID);
            }
            E::Connected => {
                s.phase = Phase::Connected;
                s.cache.clear();
                let host_id = match &s.target {
                    ConnTarget::Saved(id) => id.parse::<tern_core::HostId>().ok(),
                    ConnTarget::Quick(_) => None,
                };
                let mut tasks = Vec::new();
                if is_active && self.modals.is_empty() {
                    self.mode = Mode::Insert;
                    self.matcher.reset();
                    tasks.push(iced::advanced::widget::operate(
                        iced::advanced::widget::operation::focusable::unfocus(),
                    ));
                }
                if let Some(host_id) = host_id {
                    let store = self.store.clone();
                    let touch = Task::perform(
                        async move { store.touch_last_connected(host_id).await },
                        |r| Message::SessionTouched(r.map_err(|e| e.to_string())),
                    );
                    let reload = self.reload_hosts();
                    tasks.push(touch.chain(reload));
                }
                return Task::batch(tasks);
            }
            E::Output(bytes) => {
                s.term.feed(&bytes);
                let replies = s.term.take_replies();
                if !replies.is_empty()
                    && let Some(h) = &s.handle
                {
                    h.try_send(tern_ssh::SessionCommand::Write(replies.into()));
                }
                let bell = s.term.take_bell() && visual_bell;
                if bell {
                    s.flash = true;
                }
                s.refresh();
                if bell {
                    return Task::perform(
                        async { tokio::time::sleep(std::time::Duration::from_millis(150)).await },
                        move |()| Message::FlashEnd(id),
                    );
                }
            }
            E::Exited(_) | E::Closed if !matches!(s.phase, Phase::Connected) => {
                // Already finished (Exited is followed by Closed): nothing more to show.
            }
            E::Exited(Some(code)) if code != 0 => {
                s.note(&format!("[exited with status {code}]"));
                s.finish(None);
                self.end_session_ui(is_active);
            }
            E::Exited(_) | E::Closed => {
                // Clean exit (`exit`, Ctrl+D): close the tab like a normal terminal.
                self.remove_tab(idx);
            }
            E::Error(e) => {
                let host = match &s.target {
                    ConnTarget::Saved(id) => self.host_data.host(id).map(|h| h.hostname.clone()),
                    ConnTarget::Quick(q) => Some(q.host.clone()),
                }
                .unwrap_or_else(|| s.label.clone());
                let msg = sessions::friendly_error(&e, &host);
                s.note(&format!("[{msg}]"));
                s.finish(Some(msg));
                self.end_session_ui(is_active);
            }
        }
        Task::none()
    }

    /// After a session ends: drop any pending host-key modal and leave Insert mode.
    fn end_session_ui(&mut self, was_active: bool) {
        if was_active {
            self.modals.clear();
            self.mode = Mode::Normal;
            self.matcher.reset();
        }
    }

    fn submit_session_input(&mut self) -> Task<Message> {
        let Some(s) = self.tab_mut() else {
            return Task::none();
        };
        let Some(handle) = s.handle.clone() else {
            return Task::none();
        };
        if matches!(s.phase, Phase::Auth { .. })
            && let Some(answers) = s.answer()
        {
            handle.try_send(tern_ssh::SessionCommand::AuthResponse(answers));
        }
        Task::none()
    }

    // ---- terminal input / events ----

    /// Encode a key press for the remote shell.
    fn send_raw_key(&mut self, raw: &keymap::RawKey) {
        let Some(t) = self.tab_mut() else { return };
        let Some(bytes) = tern_term::input::encode_key(
            &raw.key,
            raw.modifiers,
            raw.text.as_deref(),
            t.snap.modes.app_cursor,
        ) else {
            return;
        };
        if let Some(h) = &t.handle {
            h.try_send(tern_ssh::SessionCommand::Write(bytes.into()));
        }
        // Typing returns to the live screen and drops any selection.
        if t.snap.display_offset > 0 || t.term.has_selection() {
            t.term.scroll_to_bottom();
            t.term.clear_selection();
            t.refresh();
        }
    }

    fn write_to_tab(&self, bytes: Vec<u8>) {
        if let Some(h) = self.tab().and_then(|t| t.handle.as_ref()) {
            h.try_send(tern_ssh::SessionCommand::Write(bytes.into()));
        }
    }

    fn on_term_event(&mut self, id: SessionId, ev: TermEvent) -> Task<Message> {
        let Some(idx) = self.tab_index(id) else {
            return Task::none();
        };
        let s = &mut self.tabs[idx];
        match ev {
            TermEvent::Resized { cols, rows } => {
                s.term.resize(cols, rows);
                s.refresh();
                self.term_size = (cols, rows);
                s.resize_gen += 1;
                let gen_ = s.resize_gen;
                return Task::perform(
                    async { tokio::time::sleep(std::time::Duration::from_millis(50)).await },
                    move |()| Message::ResizeFlush(id, gen_),
                );
            }
            TermEvent::Scroll(n) => {
                if s.snap.modes.alt_screen && s.snap.modes.alternate_scroll {
                    // Full-screen apps without mouse support: the wheel acts as arrow keys.
                    let key: &[u8] = match (n > 0, s.snap.modes.app_cursor) {
                        (true, true) => b"\x1bOA",
                        (true, false) => b"\x1b[A",
                        (false, true) => b"\x1bOB",
                        (false, false) => b"\x1b[B",
                    };
                    let bytes = key.repeat(n.unsigned_abs() as usize);
                    if let Some(h) = &s.handle {
                        h.try_send(tern_ssh::SessionCommand::Write(bytes.into()));
                    }
                } else {
                    s.term.scroll(n);
                    s.refresh();
                }
            }
            TermEvent::SelectStart { kind, row, col } => {
                s.term.start_selection(kind, row, col);
                s.drag_moved = kind != SelectKind::Simple;
                s.refresh();
            }
            TermEvent::SelectDrag { row, col } => {
                s.term.update_selection(row, col);
                s.drag_moved = true;
                s.refresh();
            }
            TermEvent::SelectEnd => {
                if !s.drag_moved {
                    s.term.clear_selection();
                    s.refresh();
                }
            }
        }
        Task::none()
    }

    // ---- clipboard / search / copy mode ----

    fn term_copy(&mut self) -> Task<Message> {
        let Some(t) = self.tab_mut() else {
            return Task::none();
        };
        let text = if t.term.in_copy_mode() {
            t.term.copy_yank()
        } else {
            t.term.selection_text()
        };
        t.term.clear_selection();
        t.refresh();
        match text {
            Some(text) => {
                let n = text.chars().count();
                self.toast(ToastKind::Success, format!("Copied {n} characters"));
                iced::clipboard::write(text)
            }
            None => {
                self.toast(ToastKind::Info, "Nothing selected");
                Task::none()
            }
        }
    }

    fn paste_text(&mut self, text: String) -> Task<Message> {
        if !self.terminal_tab() || text.is_empty() {
            return Task::none();
        }
        let lines = text.trim_end_matches(['\n', '\r']).lines().count();
        if lines > 1 && self.term_settings.confirm_paste {
            let preview: Vec<&str> = text.lines().take(3).collect();
            self.modals.push(Modal::Confirm {
                title: format!("Paste {lines} lines?"),
                body: format!("{}…", preview.join("\n")),
                then: Pending::Paste(text),
            });
            self.sync_modal_mode();
            return Task::none();
        }
        self.send_paste(&text);
        Task::none()
    }

    fn send_paste(&mut self, text: &str) {
        let bracketed = self.tab().is_some_and(|t| t.snap.modes.bracketed_paste);
        self.write_to_tab(tern_term::input::encode_paste(text, bracketed));
        if let Some(t) = self.tab_mut() {
            t.term.scroll_to_bottom();
            t.refresh();
        }
    }

    fn open_search(&mut self) -> Task<Message> {
        let Some(t) = self.tab_mut() else {
            return Task::none();
        };
        if t.search.is_none() {
            t.search = Some(t.term.search_query().unwrap_or_default().to_owned());
        }
        self.mode = Mode::Insert;
        self.matcher.reset();
        iced::widget::operation::focus(sessions::SEARCH_INPUT_ID)
    }

    fn search_step(&mut self, forward: bool) -> Task<Message> {
        if let Some(t) = self.tab_mut() {
            t.term.search_step(forward);
            t.refresh();
        }
        Task::none()
    }

    /// A key press while keyboard copy mode is active (vim-like).
    fn copy_mode_key(&mut self, chord: &keymap::Chord) -> Task<Message> {
        use iced::keyboard::key::Named;
        let ctrl = chord.mods.control();
        let shift = chord.mods.shift();
        let motion = match &chord.key {
            keymap::KeyId::Char('h') => Some(Motion::Left),
            keymap::KeyId::Char('l') => Some(Motion::Right),
            keymap::KeyId::Char('j') => Some(Motion::Down),
            keymap::KeyId::Char('k') => Some(Motion::Up),
            keymap::KeyId::Named(Named::ArrowLeft) => Some(Motion::Left),
            keymap::KeyId::Named(Named::ArrowRight) => Some(Motion::Right),
            keymap::KeyId::Named(Named::ArrowDown) => Some(Motion::Down),
            keymap::KeyId::Named(Named::ArrowUp) => Some(Motion::Up),
            keymap::KeyId::Char('0') | keymap::KeyId::Named(Named::Home) => Some(Motion::LineStart),
            keymap::KeyId::Char('$') | keymap::KeyId::Named(Named::End) => Some(Motion::LineEnd),
            keymap::KeyId::Char('w') => Some(Motion::WordForward),
            keymap::KeyId::Char('b') => Some(Motion::WordBack),
            keymap::KeyId::Char('g') if shift => Some(Motion::Bottom),
            keymap::KeyId::Char('g') => Some(Motion::Top),
            keymap::KeyId::Char('u') if ctrl => Some(Motion::HalfPageUp),
            keymap::KeyId::Char('d') if ctrl => Some(Motion::HalfPageDown),
            keymap::KeyId::Named(Named::PageUp) => Some(Motion::HalfPageUp),
            keymap::KeyId::Named(Named::PageDown) => Some(Motion::HalfPageDown),
            _ => None,
        };
        if let Some(m) = motion {
            if let Some(t) = self.tab_mut() {
                t.term.copy_move(m);
                t.refresh();
            }
            return Task::none();
        }
        match &chord.key {
            keymap::KeyId::Char('v') => {
                if let Some(t) = self.tab_mut() {
                    t.term.copy_toggle_select();
                    t.refresh();
                }
                Task::none()
            }
            keymap::KeyId::Char('y') => self.term_copy(),
            keymap::KeyId::Char('n') => self.search_step(!shift),
            keymap::KeyId::Char('/') => {
                if let Some(t) = self.tab_mut() {
                    t.term.copy_exit();
                    t.refresh();
                }
                self.open_search()
            }
            keymap::KeyId::Named(Named::Escape) | keymap::KeyId::Char('q') => {
                if let Some(t) = self.tab_mut() {
                    t.term.copy_exit();
                    t.refresh();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Persist the terminal setting `key` and redraw every tab with it.
    fn term_settings_changed(&mut self, key: &'static str) -> Task<Message> {
        for t in &mut self.tabs {
            t.cache.clear();
        }
        let Some(json) = self.term_settings.serialize(key) else {
            return Task::none();
        };
        let store = self.store.clone();
        Task::perform(
            async move { store.query(move |db| db.settings().set(key, &json)).await },
            |r| Message::SettingSaved(r.map_err(|e| e.to_string())),
        )
    }

    /// Send the user's host-key decision to the actor.
    fn decide_host_key(&mut self, action: Action, modal: Modal) -> Task<Message> {
        let Modal::HostKey { changed_from, .. } = &modal else {
            return Task::none();
        };
        let accept = match (action, changed_from.is_some()) {
            (Action::Confirm, false) | (Action::ReplaceHostKey, true) => true,
            (Action::Cancel, _) => false,
            // Enter/y on a changed key (or Shift+R on a new one) must not decide: keep the dialog.
            _ => {
                self.modals.push(modal);
                self.sync_modal_mode();
                return Task::none();
            }
        };
        if let Some(s) = self.tab_mut() {
            if let Some(h) = &s.handle {
                h.try_send(tern_ssh::SessionCommand::HostKeyDecision(accept));
            }
            if accept {
                s.phase = Phase::Connecting;
            }
        }
        Task::none()
    }

    /// Save a typed `user@host:port` as a new host (S4-T07).
    fn save_quick_host(&mut self, q: palette::QuickTarget) -> Task<Message> {
        let mut host = tern_core::Host::new(q.host.clone(), q.host.clone());
        host.port = q.port;
        host.username = Some(q.user.clone());
        let label = host.label.clone();
        let store = self.store.clone();
        Task::perform(
            async move {
                store.insert_host(host).await.map_err(|e| e.to_string())?;
                Ok(label)
            },
            Message::HostSaved,
        )
    }

    fn sync_modal_mode(&mut self) {
        self.mode = if self.modals.is_empty() {
            self.resting_mode()
        } else {
            Mode::Modal
        };
        self.matcher.reset();
    }

    fn run_pending(&mut self, pending: Pending) -> Task<Message> {
        match pending {
            Pending::CloseTab(id) => {
                if let Some(i) = self.tab_index(id) {
                    self.remove_tab(i);
                }
                Task::none()
            }
            Pending::Paste(text) => {
                self.send_paste(&text);
                Task::none()
            }
            Pending::DeleteSelected => {
                let real = matches!(
                    self.section,
                    Section::Hosts | Section::Keys | Section::Tunnels
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
        self.swallow_enter = true;
        self.matcher.reset();
        if let PromptOutcome::RenameTab(id, name) = outcome {
            if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                t.custom_name = (!name.is_empty()).then_some(name);
            }
            self.mode = self.resting_mode();
            return Task::none();
        }
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
                    PromptOutcome::ExportKey(..) | PromptOutcome::RenameTab(..) => {
                        Ok(String::new())
                    }
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
                // this host and it is dropped on Esc, lock, or selecting another row.
                self.revealed = Some((id.to_string(), Sensitive::default()));
                let store = self.store.clone();
                Task::perform(
                    async move {
                        store
                            .get_host_password(id)
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
            SensitiveOp::OpenIo(kind) => self.open_io(kind),
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
        self.host_form.is_some()
            || self.tunnel_form.is_some()
            || self.key_form.is_some()
            || self.io_form.is_some()
            || self.sftp_form.is_some()
            || self.settings.is_some()
    }

    fn host_password_action(&mut self, action: Action) -> Task<Message> {
        let Some(form) = &mut self.host_form else {
            return Task::none();
        };
        match action {
            Action::FormReveal => form.reveal = !form.reveal,
            _ => {
                if form.password.is_empty() {
                    self.toast(ToastKind::Info, "No password to copy");
                    return Task::none();
                }
                let pw = form.password.as_str().to_owned();
                self.clip_token += 1;
                self.secret_on_clipboard = true;
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

    fn active_tunnel_count(&self) -> usize {
        self.tunnel_status
            .values()
            .filter(|s| s.state == TunState::Active)
            .count()
    }

    fn tunnel_refs(&self) -> Vec<palette::TunnelRef> {
        self.tunnels_data
            .tunnels
            .iter()
            .map(|t| palette::TunnelRef {
                id: t.id.to_string(),
                label: t.label.clone(),
                running: self
                    .tunnel_status
                    .get(&t.id)
                    .is_some_and(|s| s.is_running()),
            })
            .collect()
    }

    fn send_tunnel_start(&mut self, id: tern_core::TunnelId, params: tern_ssh::ConnectParams) {
        let (Some(mgr), Some(t)) = (
            &self.tunnel_mgr,
            self.tunnels_data.tunnels.iter().find(|t| t.id == id),
        ) else {
            return;
        };
        mgr.handle.send(tern_ssh::manager::TunnelCommand::Start {
            id,
            pool_key: t.host_id.to_string(),
            params: Box::new(params),
            spec: tern_ssh::tunnel::TunnelSpec {
                kind: t.kind,
                bind_addr: t.bind_addr.clone(),
                bind_port: t.bind_port,
                target_host: t.target_host.clone(),
                target_port: t.target_port,
            },
        });
    }

    fn set_tunnel_error(&mut self, id: tern_core::TunnelId, message: String, retrying: bool) {
        if !retrying {
            let label = self
                .tunnels_data
                .tunnels
                .iter()
                .find(|t| t.id == id)
                .map_or_else(|| "tunnel".to_owned(), |t| t.label.clone());
            self.toast(ToastKind::Error, format!("Tunnel '{label}': {message}"));
        }
        self.tunnel_status
            .insert(id, TunStatus::new(TunState::Error { message, retrying }));
        self.refresh_tunnel_rows();
    }

    fn refresh_tunnel_rows(&mut self) {
        self.tunnels_data.status = self.tunnel_status.clone();
        self.rebuild_tunnels_list();
    }

    fn on_tunnel_event(&mut self, event: tern_ssh::manager::TunnelEvent) -> Task<Message> {
        use tern_ssh::manager::TunnelEvent as E;
        match event {
            E::Starting(id) => {
                self.tunnel_status
                    .insert(id, TunStatus::new(TunState::Starting));
            }
            E::Active(id) => {
                self.tunnel_status
                    .insert(id, TunStatus::new(TunState::Active));
            }
            E::Error {
                id,
                message,
                retrying,
            } => {
                self.set_tunnel_error(id, message, retrying);
                return Task::none();
            }
            E::Stopped(id) => {
                self.tunnel_status.remove(&id);
            }
            E::Stats { id, conns, bytes } => {
                if let Some(s) = self.tunnel_status.get_mut(&id) {
                    s.conns = conns;
                    s.bytes = bytes;
                }
            }
        }
        self.refresh_tunnel_rows();
        Task::none()
    }

    fn stop_all_tunnels(&mut self) {
        if let Some(mgr) = &self.tunnel_mgr {
            mgr.handle.send(tern_ssh::manager::TunnelCommand::StopAll);
        }
        self.tunnel_status.clear();
        self.tunnels_data.status.clear();
    }

    /// Start every `auto_start` tunnel once the manager and the tunnel list are both ready.
    fn maybe_autostart(&mut self) -> Task<Message> {
        if !self.autostart_pending || self.tunnel_mgr.is_none() || !self.tunnels_loaded {
            return Task::none();
        }
        self.autostart_pending = false;
        let ids: Vec<String> = self
            .tunnels_data
            .tunnels
            .iter()
            .filter(|t| t.auto_start)
            .filter(|t| {
                !self
                    .tunnel_status
                    .get(&t.id)
                    .is_some_and(TunStatus::is_running)
            })
            .map(|t| t.id.to_string())
            .collect();
        self.refresh_tunnel_rows();
        self.set_tunnels_running(&ids, true)
    }

    /// `t` / `Enter`: toggle the selected tunnel, or all tunnels of the selected host.
    fn toggle_tunnels_action(&mut self) -> Task<Message> {
        let Some(item) = self.list().selected_item().cloned() else {
            return Task::none();
        };
        let ids: Vec<String> = match (self.section, item.id) {
            (Section::Tunnels, Some(id)) => vec![id],
            (Section::Hosts, Some(host_id)) if item.group.is_none() => self
                .tunnels_data
                .tunnels
                .iter()
                .filter(|t| t.host_id.to_string() == host_id)
                .map(|t| t.id.to_string())
                .collect(),
            _ => Vec::new(),
        };
        if ids.is_empty() {
            self.toast(ToastKind::Info, "No tunnels for this selection");
            return Task::none();
        }
        self.toggle_tunnel_ids(&ids)
    }

    /// If any of `ids` is running, stop them all; otherwise start them all.
    fn toggle_tunnel_ids(&mut self, ids: &[String]) -> Task<Message> {
        let any_running = ids.iter().any(|i| {
            i.parse::<tern_core::TunnelId>()
                .ok()
                .and_then(|id| self.tunnel_status.get(&id))
                .is_some_and(TunStatus::is_running)
        });
        self.set_tunnels_running(ids, !any_running)
    }

    fn set_tunnels_running(&mut self, ids: &[String], run: bool) -> Task<Message> {
        if self.tunnel_mgr.is_none() {
            self.toast(ToastKind::Info, "Tunnel manager is not ready yet");
            return Task::none();
        }
        let mut tasks = Vec::new();
        for id in ids
            .iter()
            .filter_map(|i| i.parse::<tern_core::TunnelId>().ok())
        {
            if !run {
                if let Some(mgr) = &self.tunnel_mgr {
                    mgr.handle.send(tern_ssh::manager::TunnelCommand::Stop(id));
                }
                continue;
            }
            let Some(host_id) = self
                .tunnels_data
                .tunnels
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.host_id.to_string())
            else {
                continue;
            };
            self.tunnel_status
                .insert(id, TunStatus::new(TunState::Starting));
            let store = self.store.clone();
            tasks.push(Task::perform(
                async move {
                    sessions::resolve(&store, &ConnTarget::Saved(host_id))
                        .await
                        .map(Box::new)
                },
                move |r| Message::TunnelResolved(id, r),
            ));
        }
        self.refresh_tunnel_rows();
        Task::batch(tasks)
    }

    fn rebuild_tunnels_list(&mut self) {
        let items = self.tunnels_data.items();
        self.lists[2].replace_items(items);
    }

    fn reload_tunnels(&self) -> Task<Message> {
        let store = self.store.clone();
        Task::perform(async move { store.list_tunnels().await }, |r| {
            Message::TunnelsLoaded(r.map_err(|e| e.to_string()))
        })
    }

    fn edit_tunnel_form(&mut self, f: impl FnOnce(&mut TunnelForm)) -> Task<Message> {
        if let Some(form) = &mut self.tunnel_form {
            f(form);
        }
        Task::none()
    }

    fn open_tunnel_form(&mut self, edit: bool) -> Task<Message> {
        if self.host_data.hosts.is_empty() {
            self.toast(ToastKind::Info, "Add a host before creating tunnels");
            return Task::none();
        }
        let existing = if edit {
            let found = self
                .list()
                .selected_item()
                .and_then(|i| i.id.as_deref())
                .and_then(|id| self.tunnels_data.tunnel(id))
                .cloned();
            if found.is_none() {
                self.toast(ToastKind::Info, "Select a tunnel to edit");
                return Task::none();
            }
            found
        } else {
            None
        };
        self.tunnel_form = Some(TunnelForm::new(&self.host_data.hosts, existing.as_ref()));
        self.mode = Mode::Insert;
        self.matcher.reset();
        iced::widget::operation::focus(tunnel_form::FIRST_ID)
    }

    fn save_tunnel_form(&mut self) -> Task<Message> {
        let Some(form) = &mut self.tunnel_form else {
            return Task::none();
        };
        let tunnel = match form.build() {
            Ok(t) => t,
            Err(e) => {
                form.error = Some(e);
                return Task::none();
            }
        };
        let is_edit = form.is_edit();
        let store = self.store.clone();
        Task::perform(
            async move {
                let label = tunnel.label.clone();
                let r = if is_edit {
                    store.update_tunnel(tunnel).await
                } else {
                    store.insert_tunnel(tunnel).await
                };
                r.map(|()| label).map_err(|e| e.to_string())
            },
            Message::TunnelSaved,
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
        let password = form.password_change();
        let store = self.store.clone();
        Task::perform(
            async move {
                let label = host.label.clone();
                let id = host.id;
                let r = if is_edit {
                    store.update_host(host).await
                } else {
                    store.insert_host(host).await
                };
                r.map_err(|e| e.to_string())?;
                if let Some(pw) = password {
                    store
                        .set_host_password(id, pw)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                Ok(label)
            },
            Message::HostSaved,
        )
    }

    fn delete_host_or_group(&mut self, id: &str, is_group: bool, label: String) -> Task<Message> {
        let store = self.store.clone();
        let is_key = self.section == Section::Keys;
        let is_tunnel = self.section == Section::Tunnels;
        let id = id.to_owned();
        Task::perform(
            async move {
                if is_tunnel {
                    let tid = id.parse().map_err(|e| format!("{e}"))?;
                    store.delete_tunnel(tid).await.map_err(|e| e.to_string())?;
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
                                term: term_settings::KEYS
                                    .iter()
                                    .filter_map(|k| {
                                        s.get(k).ok().flatten().map(|v| ((*k).to_owned(), v))
                                    })
                                    .collect(),
                                lock: autolock::KEYS
                                    .iter()
                                    .filter_map(|k| {
                                        s.get(k).ok().flatten().map(|v| ((*k).to_owned(), v))
                                    })
                                    .collect(),
                            })
                        })
                        .await
                },
                |r| Message::SettingsLoaded(r.map_err(|e| e.to_string())),
            )
        };
        self.autostart_pending = true;
        let mgr = if self.tunnel_mgr.is_none() {
            let (pool, store) = (self.pool.clone(), self.store.clone());
            Task::perform(
                async move {
                    let reconnect = store
                        .query(|db| db.settings().get("tunnel.reconnect"))
                        .await
                        .ok()
                        .flatten()
                        .is_none_or(|v: String| v.trim() != "false");
                    let known = std::sync::Arc::new(AppKnownHosts(store));
                    let (handle, rx) = tern_ssh::manager::spawn(pool, known, reconnect);
                    TunnelMgr::new(handle, rx)
                },
                Message::TunnelMgrReady,
            )
        } else {
            Task::none()
        };
        let backup = {
            let store = self.store.clone();
            Task::perform(
                async move { io_ops::auto_backup_due(&store).await },
                Message::AutoBackupDue,
            )
        };
        Task::batch([
            backup,
            mgr,
            hosts,
            keymap,
            self.reload_keys(),
            self.reload_tunnels(),
        ])
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self.screen {
            Screen::Loading => match &self.form.error {
                Some(e) => container(text(e.clone()).style(crate::theme::style::text_error))
                    .center(Fill)
                    .into(),
                None => container(text("Loading…")).center(Fill).into(),
            },
            Screen::Create => column![auth::create_view(&self.form), self.status_bar()].into(),
            Screen::Unlock => column![auth::unlock_view(&self.form), self.status_bar()].into(),
            Screen::Recovery => column![auth::recovery_view(), self.status_bar()].into(),
            Screen::Main => self.main_view(),
        }
    }

    fn main_view(&self) -> Element<'_, Message> {
        let home = self.shell_view();
        let content: Element<'_, Message> = match self.tab() {
            Some(s) => sessions::view(
                s,
                &sessions::ViewCtx {
                    settings: &self.term_settings,
                    spinner_frame: self.form.spinner_frame,
                    cursor_on: self.cursor_on,
                    focused: self.mode == Mode::Insert && self.modals.is_empty(),
                },
            ),
            None => home,
        };
        let base: Element<'_, Message> = if self.tabs.is_empty() {
            content
        } else {
            column![
                sessions::tab_bar(&self.tabs, self.active, &self.keymap),
                content
            ]
            .into()
        };
        let mut layers: Vec<Element<'_, Message>> = vec![self.with_overlays(base)];
        if let Some(toasts) = self.toast_layer() {
            layers.push(toasts);
        }
        column![iced::widget::stack(layers).height(Fill), self.status_bar()].into()
    }

    /// The form / modal / palette that sits above the base view, if any.
    fn with_overlays<'a>(&'a self, base: Element<'a, Message>) -> Element<'a, Message> {
        if let Some(form) = &self.host_form {
            return iced::widget::stack![base, host_form::view(form)].into();
        }
        if let Some(form) = &self.tunnel_form {
            return iced::widget::stack![base, tunnel_form::view(form)].into();
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
        if let Some(f) = &self.io_form {
            return iced::widget::stack![base, io_form::view(f)].into();
        }
        if let Some(f) = &self.sftp_form {
            return iced::widget::stack![base, sftp_form::view(f)].into();
        }
        if let Some(s) = &self.settings {
            let ctx = settings::ViewCtx {
                keymap: &self.keymap,
                overrides: &self.keymap_overrides,
                term: &self.term_settings,
                data_dir: crate::paths::resolve_data_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            };
            return iced::widget::stack![base, settings::view(s, &ctx)].into();
        }
        match (self.modals.top(), self.mode) {
            (Some(m), _) => iced::widget::stack![base, modal::view(m, &self.keymap)].into(),
            (None, Mode::Palette) => {
                iced::widget::stack![base, palette::view(&self.palette)].into()
            }
            (None, _) => base,
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
        if let Some(mgr) = &self.tunnel_mgr {
            subs.push(tunnel_run::subscription(mgr));
        }
        for s in &self.tabs {
            if let Some(src) = &s.source {
                subs.push(sessions::subscription(src));
            }
        }
        if let Some(s) = self.tab() {
            if matches!(s.phase, Phase::Connecting) {
                subs.push(
                    iced::time::every(std::time::Duration::from_millis(120))
                        .map(|_| Message::SpinnerTick),
                );
            }
            if s.snap.cursor_blinking && matches!(s.phase, Phase::Connected) {
                subs.push(iced::time::every(tern_term::widget::BLINK).map(|_| Message::BlinkTick));
            }
        }
        if self.screen == Screen::Main && self.autolock.enabled() {
            subs.push(
                iced::time::every(autolock::TICK)
                    .map(|_| Message::AutoLockTick(std::time::SystemTime::now())),
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
        crate::theme::theme(self.dark)
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

async fn load_keys(store: &Store) -> tern_store::Result<KeysData> {
    Ok(KeysData {
        keys: store.list_keys().await?,
        created: store.key_created_at().await?,
    })
}

/// Load everything the Hosts section shows.
async fn load_hosts(store: &Store) -> tern_store::Result<HostsData> {
    let hosts = store.list_hosts().await?;
    let mut with_password = std::collections::HashSet::new();
    for h in &hosts {
        if h.auth == tern_core::HostAuth::Password && store.host_has_password(h.id).await? {
            with_password.insert(h.id);
        }
    }
    Ok(HostsData {
        hosts,
        groups: store.list_groups().await?,
        keys: store.list_keys().await?,
        with_password,
        last_connected: store.host_last_connected().await?,
    })
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// Chords that keep working inside keyboard copy mode (tab/lock/palette control).
fn is_app_chord(chord: &keymap::Chord) -> bool {
    let m = chord.mods;
    (m.control() && m.shift())
        || (m.control()
            && matches!(
                chord.key,
                keymap::KeyId::Named(iced::keyboard::key::Named::Tab)
            ))
}

/// Actions that stay available while a terminal tab is focused (list navigation does not).
fn tab_action_allowed(action: Action) -> bool {
    !matches!(
        action,
        Action::MoveDown
            | Action::MoveUp
            | Action::First
            | Action::Last
            | Action::PageDown
            | Action::PageUp
            | Action::Edit
            | Action::Delete
            | Action::Filter
            | Action::Add
            | Action::GoHosts
            | Action::GoKeys
            | Action::GoTunnels
    )
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
    fn tunnel_events_drive_status_and_the_active_count() {
        use tern_ssh::manager::TunnelEvent as E;
        let mut s = main_state();
        let id = tern_core::TunnelId::new();
        let _ = s.update(Message::Tunnel(E::Starting(id)));
        assert_eq!(s.active_tunnel_count(), 0);
        let _ = s.update(Message::Tunnel(E::Active(id)));
        let _ = s.update(Message::Tunnel(E::Stats {
            id,
            conns: 2,
            bytes: 10,
        }));
        assert_eq!(s.active_tunnel_count(), 1);
        assert_eq!(s.tunnel_status[&id].bytes, 10);
        let _ = s.update(Message::Tunnel(E::Error {
            id,
            message: "lost".into(),
            retrying: true,
        }));
        assert_eq!(s.active_tunnel_count(), 0);
        assert!(s.tunnel_status[&id].is_running());
        let _ = s.update(Message::Tunnel(E::Stopped(id)));
        assert!(s.tunnel_status.is_empty());
    }

    #[test]
    fn toggling_tunnels_without_any_just_toasts() {
        let mut s = main_state();
        let _ = s.update(Message::Dispatch(Action::GoTunnels));
        let _ = s.update(Message::Dispatch(Action::ToggleTunnels));
        assert!(s.tunnel_status.is_empty());
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
        let _ = s.update(Message::Dispatch(Action::GoTunnels));
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
        let session = s.tab().expect("session started");
        assert_eq!(session.label, "prod-db");
        assert_eq!(session.phase, Phase::Connecting);
    }

    fn with_session() -> State {
        let mut s = main_state();
        let host = tern_core::Host::new("web", "web.example.com");
        let id = host.id.to_string();
        let _ = s.update(Message::HostsLoaded(Ok(HostsData {
            hosts: vec![host],
            ..HostsData::default()
        })));
        let _ = s.start_session(ConnTarget::Saved(id));
        s
    }

    fn host_key(changed_from: Option<&str>) -> Message {
        Message::Session(
            1,
            tern_ssh::SessionEvent::HostKeyPrompt {
                host: "web.example.com".into(),
                port: 22,
                key_type: "ssh-ed25519".into(),
                fingerprint: "SHA256:new".into(),
                changed_from: changed_from.map(str::to_owned),
            },
        )
    }

    #[test]
    fn unknown_host_key_is_trusted_with_confirm() {
        let mut s = with_session();
        let _ = s.update(host_key(None));
        assert_eq!(s.mode, Mode::Modal);
        assert!(matches!(s.modals.top(), Some(Modal::HostKey { .. })));
        let _ = s.update(Message::Dispatch(Action::Confirm));
        assert!(s.modals.is_empty());
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.tab().map(|x| &x.phase), Some(&Phase::Connecting));
    }

    #[test]
    fn changed_host_key_needs_the_explicit_replace_action() {
        let mut s = with_session();
        let _ = s.update(host_key(Some("SHA256:old")));
        // Enter / y must not trust a changed key.
        let _ = s.update(Message::Dispatch(Action::Confirm));
        assert!(matches!(s.modals.top(), Some(Modal::HostKey { .. })));
        assert_eq!(s.mode, Mode::Modal);
        let _ = s.update(Message::Dispatch(Action::ReplaceHostKey));
        assert!(s.modals.is_empty());
    }

    #[test]
    fn rejecting_a_host_key_via_esc_closes_the_dialog() {
        let mut s = with_session();
        let _ = s.update(host_key(None));
        let _ = s.update(Message::Dispatch(Action::Back));
        assert!(s.modals.is_empty());
        assert!(s.tab().is_some());
    }

    #[test]
    fn errors_end_the_session_and_reconnect_starts_a_new_one() {
        let mut s = with_session();
        let _ = s.update(Message::Session(
            1,
            tern_ssh::SessionEvent::Error(tern_ssh::Error::Refused),
        ));
        let session = s.tab().expect("kept for reconnect");
        assert!(matches!(&session.phase, Phase::Disconnected(Some(m)) if m.contains("refused")));
        // Events from a stale session id are ignored.
        let _ = s.update(Message::Session(99, tern_ssh::SessionEvent::Connected));
        let _ = s.update(Message::Dispatch(Action::Reconnect));
        let session = s.tab().expect("new session");
        assert_eq!(session.id, 2);
        assert_eq!(session.phase, Phase::Connecting);
    }

    #[test]
    fn list_actions_are_ignored_while_a_session_is_open() {
        let mut s = with_session();
        let _ = s.update(Message::Dispatch(Action::GoKeys));
        assert_eq!(s.section, Section::Hosts);
        let _ = s.update(Message::Dispatch(Action::Back));
        assert!(s.tabs.is_empty(), "Esc cancels a connection attempt");
    }

    fn connected_tabs(n: usize) -> State {
        let mut s = main_state();
        for i in 0..n {
            let _ = s.start_session(ConnTarget::Quick(palette::QuickTarget {
                user: "u".into(),
                host: format!("h{i}"),
                port: 22,
            }));
            let id = s.tabs[i].id;
            let _ = s.update(Message::Session(id, tern_ssh::SessionEvent::Connected));
        }
        s
    }

    #[test]
    fn connected_tab_takes_keys_and_esc_leaves_to_the_host_list() {
        let mut s = connected_tabs(1);
        assert_eq!(s.mode, Mode::Insert);
        assert!(s.terminal_focused());
        // Ctrl+Shift+Esc leaves the terminal, a second Esc goes to the host list.
        let _ = s.update(Message::Dispatch(Action::Back));
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(s.active, Some(0));
        let _ = s.update(Message::Dispatch(Action::Back));
        assert_eq!(s.active, None);
        assert_eq!(s.tabs.len(), 1, "the session keeps running");
    }

    #[test]
    fn tabs_cycle_jump_and_new_tab_goes_home() {
        let mut s = connected_tabs(3);
        assert_eq!(s.active, Some(2));
        let _ = s.update(Message::Dispatch(Action::NextTab));
        assert_eq!(s.active, Some(0));
        let _ = s.update(Message::Dispatch(Action::PrevTab));
        assert_eq!(s.active, Some(2));
        let _ = s.update(Message::Dispatch(Action::JumpTab2));
        assert_eq!(s.active, Some(1));
        let _ = s.update(Message::Dispatch(Action::JumpHome));
        assert_eq!(s.active, None);
        let _ = s.update(Message::Dispatch(Action::JumpTab2));
        assert_eq!(s.active, Some(1));
        // Out of range is ignored.
        let _ = s.update(Message::Dispatch(Action::JumpTab9));
        assert_eq!(s.active, Some(1));
        let _ = s.update(Message::Dispatch(Action::NewTab));
        assert_eq!(s.active, None);
        let _ = s.update(Message::Dispatch(Action::NextTab));
        assert_eq!(s.active, Some(0));
        assert_eq!(s.mode, Mode::Insert);
    }

    #[test]
    fn closing_a_live_tab_asks_first() {
        let mut s = connected_tabs(2);
        let _ = s.update(Message::Dispatch(Action::CloseTab));
        assert_eq!(s.tabs.len(), 2);
        assert!(matches!(s.modals.top(), Some(Modal::Confirm { .. })));
        let _ = s.update(Message::Dispatch(Action::Cancel));
        assert_eq!(s.tabs.len(), 2);
        assert_eq!(s.mode, Mode::Insert);
        let _ = s.update(Message::Dispatch(Action::CloseTab));
        let _ = s.update(Message::Dispatch(Action::Confirm));
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.active, Some(0));
    }

    #[test]
    fn output_reaches_the_emulator_of_its_own_tab() {
        let mut s = connected_tabs(2);
        let (a, b) = (s.tabs[0].id, s.tabs[1].id);
        let _ = s.update(Message::Session(
            a,
            tern_ssh::SessionEvent::Output(b"hello\r\n".as_slice().into()),
        ));
        assert_eq!(s.tabs[0].snap.row_text(0), "hello");
        assert_eq!(s.tabs[1].snap.row_text(0), "");
        let _ = b;
    }

    #[test]
    fn widget_resize_updates_emulator_and_new_session_size() {
        let mut s = connected_tabs(1);
        let id = s.tabs[0].id;
        let _ = s.update(Message::Term(
            id,
            TermEvent::Resized {
                cols: 100,
                rows: 30,
            },
        ));
        assert_eq!((s.tabs[0].snap.cols, s.tabs[0].snap.rows), (100, 30));
        assert_eq!(s.term_size, (100, 30));
    }

    #[test]
    fn rename_tab_sets_and_clears_the_custom_name() {
        let mut s = connected_tabs(1);
        let id = s.tabs[0].id;
        let _ = s.update(Message::Dispatch(Action::RenameTab));
        let _ = s.update(Message::PromptChanged("prod".into()));
        let _ = s.update(Message::PromptSubmit);
        assert_eq!(s.tabs[0].display_name(), "prod");
        assert_eq!(s.mode, Mode::Insert);
        let _ = s.update(Message::Dispatch(Action::RenameTab));
        let _ = s.update(Message::PromptChanged(String::new()));
        let _ = s.update(Message::PromptSubmit);
        assert_eq!(s.tabs[0].display_name(), "u@h0");
        let _ = id;
    }

    #[test]
    fn enter_that_submits_a_prompt_is_not_replayed_to_the_list() {
        let mut s = state();
        s.screen = Screen::Main;
        let _ = s.update(Message::Dispatch(Action::NewGroup));
        let _ = s.update(Message::PromptChanged("prod".into()));
        let _ = s.update(Message::PromptSubmit);
        assert!(s.swallow_enter);
        press(&mut s, "enter");
        assert!(!s.swallow_enter);
        assert!(s.tabs.is_empty());
    }

    #[test]
    fn multiline_paste_is_confirmed_single_line_is_not() {
        let mut s = connected_tabs(1);
        let _ = s.update(Message::Pasted(Some("one line\n".into())));
        assert!(s.modals.is_empty());
        let _ = s.update(Message::Pasted(Some("a\nb\nc".into())));
        assert!(matches!(
            s.modals.top(),
            Some(Modal::Confirm {
                then: Pending::Paste(_),
                ..
            })
        ));
        let _ = s.update(Message::Dispatch(Action::Cancel));
        assert!(s.modals.is_empty());
        assert_eq!(s.mode, Mode::Insert);
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
        assert_eq!(s.theme(), crate::theme::theme(true));
        let _ = s.update(Message::Dispatch(Action::ToggleTheme));
        assert_eq!(s.theme(), crate::theme::theme(false));
        let _ = s.update(Message::Dispatch(Action::ToggleTheme));
        assert_eq!(s.theme(), crate::theme::theme(true));
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

    fn tab(id: u64) -> SessionView {
        SessionView::new(id, ConnTarget::Saved("h".into()), "h".into(), (80, 24), 100)
    }

    fn press(s: &mut State, chord: &str) {
        let c = Chord::parse(chord).unwrap();
        let raw = keymap::RawKey {
            key: iced::keyboard::Key::Unidentified,
            modifiers: iced::keyboard::Modifiers::empty(),
            text: None,
        };
        let _ = s.update(Message::Key(c, raw));
    }

    fn settings_on_keybindings(action: Action) -> State {
        let mut s = main_state();
        let _ = s.dispatch(Action::Settings);
        assert_eq!(s.mode, Mode::Insert);
        press(&mut s, "ctrl+tab");
        press(&mut s, "ctrl+tab");
        let scr = s.settings.as_mut().unwrap();
        assert_eq!(scr.tab, settings::Tab::Keybindings);
        scr.selected = Action::ALL.iter().position(|a| *a == action).unwrap();
        s
    }

    #[test]
    fn settings_open_and_close_with_esc() {
        let mut s = main_state();
        let _ = s.dispatch(Action::Settings);
        assert!(s.settings.is_some());
        press(&mut s, "esc");
        assert!(s.settings.is_none());
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn rebinding_replaces_the_default_keys_and_reset_restores_them() {
        let mut s = settings_on_keybindings(Action::GoKeys);
        press(&mut s, "enter");
        assert_eq!(
            s.settings.as_ref().unwrap().capture,
            settings::Capture::Waiting
        );
        press(&mut s, "ctrl+shift+k");
        assert_eq!(s.keymap.keys_for(Action::GoKeys), ["ctrl+shift+k"]);
        assert!(s.keymap_overrides.contains_key("nav.keys"));
        press(&mut s, "r");
        assert_eq!(s.keymap.keys_for(Action::GoKeys), ["2"]);
        assert!(s.keymap_overrides.is_empty());
    }

    #[test]
    fn rebinding_to_a_taken_key_asks_before_applying() {
        let mut s = settings_on_keybindings(Action::GoKeys);
        press(&mut s, "enter");
        press(&mut s, "j");
        match &s.settings.as_ref().unwrap().capture {
            settings::Capture::Conflict { with, .. } => assert!(with.contains(&Action::MoveDown)),
            other => panic!("expected a conflict, got {other:?}"),
        }
        assert_eq!(s.keymap.keys_for(Action::GoKeys), ["2"]);
        // Esc cancels without changing anything.
        press(&mut s, "esc");
        assert_eq!(
            s.settings.as_ref().unwrap().capture,
            settings::Capture::Idle
        );
        assert_eq!(s.keymap.keys_for(Action::GoKeys), ["2"]);
        // Enter applies anyway.
        press(&mut s, "enter");
        press(&mut s, "j");
        press(&mut s, "enter");
        assert_eq!(s.keymap.keys_for(Action::GoKeys), ["j"]);
    }

    #[test]
    fn unbind_leaves_the_action_without_keys() {
        let mut s = settings_on_keybindings(Action::GoKeys);
        press(&mut s, "delete");
        assert!(s.keymap.keys_for(Action::GoKeys).is_empty());
        let _ = s.update(Message::Settings(SettingsMsg::KbResetAll));
        assert_eq!(s.keymap.keys_for(Action::GoKeys), ["2"]);
    }

    #[test]
    fn general_settings_validate_and_apply() {
        let mut s = main_state();
        let _ = s.dispatch(Action::Settings);
        let _ = s.update(Message::Settings(SettingsMsg::Idle("abc".into())));
        let _ = s.update(Message::Settings(SettingsMsg::Save));
        assert!(s.settings.as_ref().unwrap().error.is_some());
        let _ = s.update(Message::Settings(SettingsMsg::Idle("7".into())));
        let _ = s.update(Message::Settings(SettingsMsg::Save));
        assert_eq!(s.autolock.idle_minutes, 7);
        let _ = s.update(Message::Settings(SettingsMsg::Keep(true)));
        assert!(s.autolock.keep_sessions);
    }

    #[test]
    fn security_form_validates_before_touching_the_vault() {
        let mut s = main_state();
        let _ = s.dispatch(Action::Settings);
        let m = |m| Message::Settings(m);
        let _ = s.update(m(SettingsMsg::Tab(settings::Tab::Security)));
        let _ = s.update(m(SettingsMsg::Save));
        assert!(s.settings.as_ref().unwrap().error.is_some());
        let _ = s.update(m(SettingsMsg::Current(Sensitive::new("pw".into()))));
        let _ = s.update(m(SettingsMsg::New(Sensitive::new("a".into()))));
        let _ = s.update(m(SettingsMsg::Confirm(Sensitive::new("b".into()))));
        let _ = s.update(m(SettingsMsg::Save));
        let scr = s.settings.as_ref().unwrap();
        assert!(scr.error.as_deref().unwrap().contains("match"));
        assert!(!scr.busy);
    }

    #[tokio::test]
    async fn changing_password_and_cost_end_to_end() {
        let store = Store::open_in_memory().unwrap();
        let kdf = KdfParams::new(8, 1, 1).unwrap();
        let pw = |s: &str| secrecy::SecretString::from(s);
        store
            .create_vault_with_params(pw("old"), kdf)
            .await
            .unwrap();
        let costly = KdfParams::new(16, 2, 1).unwrap();
        store
            .change_security(pw("old"), pw("new"), Some(costly))
            .await
            .unwrap();
        assert_eq!(store.kdf_params().await.unwrap(), costly);
        store.lock().await.unwrap();
        assert!(store.unlock(pw("old")).await.is_err());
        store.unlock(pw("new")).await.unwrap();
        // A wrong current password changes nothing.
        assert!(
            store
                .change_security(pw("bad"), pw("x"), None)
                .await
                .is_err()
        );
        assert_eq!(store.kdf_params().await.unwrap(), costly);
    }

    #[test]
    fn locking_closes_tabs_unless_sessions_are_kept() {
        let mut s = main_state();
        s.tabs.push(tab(1));
        let _ = s.update(Message::Locked(Ok(())));
        assert!(s.tabs.is_empty());

        let mut s = main_state();
        s.autolock.keep_sessions = true;
        s.tabs.push(tab(1));
        let _ = s.update(Message::Locked(Ok(())));
        assert_eq!(s.screen, Screen::Unlock);
        assert_eq!(s.tabs.len(), 1);
    }

    #[test]
    fn idle_tick_locks_only_on_the_main_screen_and_when_enabled() {
        let now = std::time::SystemTime::now();
        let later = now + std::time::Duration::from_secs(3600);
        let mut s = main_state();
        s.autolock = AutoLock::new(now);
        s.autolock.idle_minutes = 0;
        let _ = s.update(Message::AutoLockTick(later));
        assert_eq!(s.screen, Screen::Main);

        s.autolock.idle_minutes = 1;
        s.secret_on_clipboard = true;
        let _ = s.update(Message::AutoLockTick(
            now + std::time::Duration::from_secs(30),
        ));
        // Lock is asynchronous; the clipboard flag is consumed when the lock is issued.
        assert!(s.secret_on_clipboard);
        let _ = s.update(Message::AutoLockTick(
            now + std::time::Duration::from_secs(70),
        ));
        assert!(!s.secret_on_clipboard);
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
