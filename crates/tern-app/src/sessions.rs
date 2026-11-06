//! SSH session state and view (S4-T05/T06): the bridge between the `tern-ssh` actor and iced.
//!
//! Each tab owns one session plus its terminal emulator (S5). The input line below the terminal is
//! only used for authentication prompts.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use iced::futures::{Stream, stream};
use iced::widget::canvas::Cache;
use iced::widget::{button, column, container, row, text, text_input};
use iced::{Color, Element, Fill, Theme};
use secrecy::SecretString;
use tern_core::{HostId, Identity};
use tern_ssh::known_hosts::BoxFuture;
use tern_ssh::{
    AuthPromptItem, ConnectParams, HostKeyInfo, KeyAuth, KnownHostsStore, SessionEvent,
    SessionHandle,
};
use tern_store::Store;
use tern_term::widget::{TermEvent, TerminalView};
use tern_term::{Emulator, Snapshot};
use tokio::sync::mpsc::Receiver;

use crate::app::Message;
use crate::auth::Sensitive;
use crate::palette::QuickTarget;
use crate::term_settings::TermSettings;

pub const SESSION_INPUT_ID: &str = "session-input";
pub const SEARCH_INPUT_ID: &str = "term-search";
const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];

pub type SessionId = u64;

/// What a session connects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnTarget {
    /// A saved host (its id).
    Saved(String),
    /// A typed `user@host:port` (S4-T07); credentials are asked for during auth.
    Quick(QuickTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Connecting,
    /// Waiting for the host-key modal.
    HostKey,
    Auth {
        name: String,
        instructions: String,
        prompts: Vec<AuthPromptItem>,
    },
    Connected,
    /// Ended; the string is a human-readable reason when it was not a clean close.
    Disconnected(Option<String>),
}

#[derive(Debug)]
pub struct SessionView {
    pub id: SessionId,
    pub target: ConnTarget,
    pub label: String,
    pub phase: Phase,
    pub term: Emulator,
    /// Last rendered state of `term`; refreshed by [`SessionView::refresh`].
    pub snap: Snapshot,
    pub cache: Cache,
    /// User-chosen tab name (overrides `label`).
    pub custom_name: Option<String>,
    /// Latest search text while the search bar is open.
    pub search: Option<String>,
    /// Debounce token for the remote PTY resize.
    pub resize_gen: u64,
    /// Visual-bell flash is showing.
    pub flash: bool,
    /// The mouse moved since the selection started (a bare click selects nothing).
    pub drag_moved: bool,
    /// The input line (command text, or an auth answer which may be a password).
    pub input: Sensitive,
    /// Answers collected so far for the current auth prompt.
    answers: Vec<SecretString>,
    pub handle: Option<SessionHandle>,
    pub source: Option<EventSource>,
}

impl SessionView {
    pub fn new(
        id: SessionId,
        target: ConnTarget,
        label: String,
        size: (usize, usize),
        scrollback: usize,
    ) -> Self {
        let term = Emulator::new(size.0, size.1, scrollback);
        let snap = term.snapshot();
        Self {
            term,
            snap,
            cache: Cache::new(),
            custom_name: None,
            search: None,
            resize_gen: 0,
            flash: false,
            drag_moved: false,
            id,
            target,
            label,
            phase: Phase::Connecting,
            input: Sensitive::default(),
            answers: Vec::new(),
            handle: None,
            source: None,
        }
    }

    /// Tab title: user name, else the remote's OSC title for shells, else the host label.
    pub fn display_name(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.label)
    }

    /// Re-snapshot the emulator and invalidate the drawn frame.
    pub fn refresh(&mut self) {
        self.snap = self.term.snapshot();
        self.cache.clear();
    }

    /// Show a status line inside the terminal (e.g. "[connection closed]").
    pub fn note(&mut self, msg: &str) {
        self.term
            .feed(format!("\r\n\x1b[2m{msg}\x1b[0m\r\n").as_bytes());
        self.refresh();
    }

    pub fn is_live(&self) -> bool {
        !matches!(self.phase, Phase::Disconnected(_))
    }

    /// The auth prompt currently being answered, if any.
    pub fn current_prompt(&self) -> Option<&AuthPromptItem> {
        match &self.phase {
            Phase::Auth { prompts, .. } => prompts.get(self.answers.len()),
            _ => None,
        }
    }

    pub fn begin_auth(&mut self, name: String, instructions: String, prompts: Vec<AuthPromptItem>) {
        self.answers.clear();
        self.input.clear();
        self.phase = Phase::Auth {
            name,
            instructions,
            prompts,
        };
    }

    /// Record the typed answer. Returns all answers once every prompt has one.
    pub fn answer(&mut self) -> Option<Vec<SecretString>> {
        let Phase::Auth { prompts, .. } = &self.phase else {
            return None;
        };
        let total = prompts.len();
        self.answers.push(self.input.take_secret());
        if self.answers.len() >= total {
            self.phase = Phase::Connecting;
            Some(std::mem::take(&mut self.answers))
        } else {
            None
        }
    }

    pub fn finish(&mut self, reason: Option<String>) {
        self.phase = Phase::Disconnected(reason);
        self.handle = None;
        self.source = None;
        self.answers.clear();
        self.input.clear();
    }
}

/// Where the subscription takes the event receiver from. Hashed by session id only.
#[derive(Clone)]
pub struct EventSource {
    pub id: SessionId,
    rx: Arc<Mutex<Option<Receiver<SessionEvent>>>>,
}

impl EventSource {
    pub fn new(id: SessionId, rx: Receiver<SessionEvent>) -> Self {
        Self {
            id,
            rx: Arc::new(Mutex::new(Some(rx))),
        }
    }
}

impl std::fmt::Debug for EventSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventSource").field("id", &self.id).finish()
    }
}

impl Hash for EventSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// Turns a session's event receiver into `Message::Session` values.
pub fn subscription(source: &EventSource) -> iced::Subscription<Message> {
    iced::Subscription::run_with(source.clone(), events)
}

fn events(source: &EventSource) -> impl Stream<Item = Message> + use<> {
    let id = source.id;
    let rx = source.rx.lock().ok().and_then(|mut g| g.take());
    stream::unfold(rx, move |rx| async move {
        let mut rx = rx?;
        let event = rx.recv().await?;
        Some((Message::Session(id, event), Some(rx)))
    })
}

/// Result of starting a session task. `Clone` so it can travel in a `Message`.
#[derive(Clone)]
pub struct Spawned {
    pub handle: SessionHandle,
    pub source: EventSource,
}

impl std::fmt::Debug for Spawned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Spawned")
    }
}

/// Everything the SSH layer needs for a host, decrypted just now. Secrets live only in the
/// returned params (zeroized on drop).
pub async fn resolve(store: &Store, target: &ConnTarget) -> Result<ConnectParams, String> {
    let host_id = match target {
        ConnTarget::Saved(id) => id,
        ConnTarget::Quick(q) => {
            return Ok(ConnectParams::new(q.host.clone(), q.port, q.user.clone()));
        }
    };
    let id: HostId = host_id.parse().map_err(|_| "Unknown host".to_owned())?;
    let host = store
        .get_host(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("That host no longer exists")?;
    let identity: Option<Identity> = match host.identity_id {
        Some(iid) => store
            .list_identities()
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|i| i.id == iid),
        None => None,
    };
    let username = host
        .username
        .clone()
        .filter(|u| !u.is_empty())
        .or_else(|| identity.as_ref().map(|i| i.username.clone()))
        .filter(|u| !u.is_empty())
        .ok_or("This host has no username; edit it or pick an identity")?;
    let mut params = ConnectParams::new(host.hostname.clone(), host.port, username);
    if let Some(secs) = host.keepalive_secs {
        params.keepalive = (secs > 0).then(|| std::time::Duration::from_secs(u64::from(secs)));
    }
    if let Some(identity) = identity {
        params.password = store
            .get_identity_password(identity.id)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(kid) = identity.ssh_key_id
            && let Some(pem) = store
                .get_key_private(kid)
                .await
                .map_err(|e| e.to_string())?
        {
            let passphrase = store
                .get_key_passphrase(kid)
                .await
                .map_err(|e| e.to_string())?;
            params.key = Some(KeyAuth { pem, passphrase });
        }
    }
    Ok(params)
}

/// Known-hosts persistence on top of the store.
pub struct AppKnownHosts(pub Store);

impl KnownHostsStore for AppKnownHosts {
    fn lookup<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        key_type: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(async move {
            let (h, t) = (host.to_owned(), key_type.to_owned());
            self.0
                .query(move |db| db.known_hosts().find(&h, port, &t))
                .await
                .map(|k| k.map(|k| k.fingerprint))
                .map_err(|e| e.to_string())
        })
    }

    fn save<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        key: &'a HostKeyInfo,
        replace: bool,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let (h, key) = (host.to_owned(), key.clone());
            self.0
                .query(move |db| {
                    let repo = db.known_hosts();
                    let existing = repo.find(&h, port, &key.key_type)?;
                    let mut entry = tern_core::KnownHost {
                        id: tern_core::KnownHostId::new(),
                        host: h,
                        port,
                        key_type: key.key_type,
                        fingerprint: key.fingerprint,
                        public_key: key.public_key,
                    };
                    match existing {
                        Some(old) if replace => {
                            entry.id = old.id;
                            repo.update(&entry)
                        }
                        Some(_) => Ok(()),
                        None => repo.insert(&entry),
                    }
                })
                .await
                .map_err(|e| e.to_string())
        })
    }
}

/// Human-readable explanation of why a connection failed.
pub fn friendly_error(e: &tern_ssh::Error, host: &str) -> String {
    use tern_ssh::Error as E;
    match e {
        E::Dns => format!("Could not resolve '{host}'. Check the hostname."),
        E::Refused => format!("Connection to '{host}' was refused. Is SSH running on that port?"),
        E::Timeout => format!("Connecting to '{host}' timed out."),
        E::HostKeyRejected => "Host key rejected; nothing was saved.".into(),
        E::AuthFailed => "Authentication failed: wrong password or key.".into(),
        E::Key(m) => format!("The private key could not be used: {m}"),
        E::Disconnected => "The connection was lost.".into(),
        E::Network(m) | E::Other(m) => format!("Connection error: {m}"),
    }
}

/// Presentation state shared by all tabs.
pub struct ViewCtx<'a> {
    pub settings: &'a TermSettings,
    pub spinner_frame: usize,
    pub cursor_on: bool,
    /// The terminal has keyboard focus (Insert mode, no modal).
    pub focused: bool,
}

fn phase_color(phase: &Phase) -> Color {
    match phase {
        Phase::Connected => Color::from_rgb8(0x3f, 0xb9, 0x50),
        Phase::Connecting | Phase::HostKey | Phase::Auth { .. } => {
            Color::from_rgb8(0xd2, 0x99, 0x22)
        }
        Phase::Disconnected(None) => Color::from_rgb8(0x88, 0x88, 0x88),
        Phase::Disconnected(Some(_)) => Color::from_rgb8(0xf8, 0x51, 0x49),
    }
}

/// The tab strip: a "Hosts" home tab followed by one button per session.
pub fn tab_bar<'a>(tabs: &'a [SessionView], active: Option<usize>) -> Element<'a, Message> {
    let mut strip = row![].spacing(4).padding([4, 8]);
    let home = button(text("Hosts").size(13))
        .padding([3, 10])
        .style(if active.is_none() {
            button::primary
        } else {
            button::secondary
        })
        .on_press(Message::SelectHome);
    strip = strip.push(home);
    for (i, t) in tabs.iter().enumerate() {
        let label = row![
            text("●").size(11).color(phase_color(&t.phase)),
            text(format!("{} {}", i + 1, t.display_name())).size(13),
        ]
        .spacing(6)
        .align_y(iced::Center);
        strip = strip.push(
            button(label)
                .padding([3, 10])
                .style(if active == Some(i) {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press(Message::SelectTab(i)),
        );
    }
    container(strip)
        .width(Fill)
        .style(container::bordered_box)
        .into()
}

pub fn view<'a>(s: &'a SessionView, ctx: &ViewCtx<'a>) -> Element<'a, Message> {
    let id = s.id;
    let settings = ctx.settings;
    let status = match &s.phase {
        Phase::Connecting | Phase::HostKey => Some(format!(
            "{} Connecting to {}…",
            SPINNER[ctx.spinner_frame % SPINNER.len()],
            s.label
        )),
        Phase::Auth { name, .. } => Some(format!("{} — {name}", s.label)),
        Phase::Connected => None,
        Phase::Disconnected(None) => Some(format!("{} — disconnected", s.label)),
        Phase::Disconnected(Some(why)) => Some(format!("{} — {why}", s.label)),
    };
    let hint = match &s.phase {
        _ if s.snap.copy_mode => {
            "COPY MODE  hjkl move  w/b word  0/$ line  g/G top/bottom  Ctrl+u/d page  v select  y copy  n/N search  Esc exit"
        }
        Phase::Disconnected(_) => {
            "r reconnect   Ctrl+Shift+W close   Ctrl+Shift+Esc / Esc back to hosts"
        }
        Phase::Connected => {
            "Ctrl+Shift+Esc leave terminal   Ctrl+Shift+F search   Ctrl+Shift+Space copy mode   F1 help"
        }
        _ => "Esc cancel",
    };
    let term = TerminalView {
        snapshot: &s.snap,
        cache: &s.cache,
        scheme: settings.scheme.scheme(),
        font: settings.font(),
        font_size: settings.font_size,
        cursor_on: ctx.cursor_on,
        focused: ctx.focused,
        flash: s.flash,
        cursor_shape: settings.cursor.shape(),
        on_event: Box::new(move |e: TermEvent| Message::Term(id, e)),
    }
    .into_element();

    let mut body = column![].spacing(6);
    if let Some(status) = status {
        body = body.push(text(status).size(14));
    }
    body = body.push(container(term).width(Fill).height(Fill));
    if let Phase::Auth {
        instructions,
        prompts,
        ..
    } = &s.phase
    {
        if !instructions.is_empty() {
            body = body.push(text(instructions.as_str()).size(13));
        }
        if let Some(p) = prompts.get(s.answers.len()) {
            body = body.push(text(p.prompt.as_str()).size(14));
        }
    }
    if matches!(s.phase, Phase::Auth { .. }) {
        let secure = s.current_prompt().is_some_and(|p| !p.echo);
        body = body.push(
            text_input("", s.input.as_str())
                .id(SESSION_INPUT_ID)
                .secure(secure)
                .on_input(|v| Message::SessionInput(Sensitive::new(v)))
                .on_submit(Message::SessionSubmit)
                .padding(6),
        );
    }
    if let Some(q) = &s.search {
        let count = match s.term.search_status() {
            Some((cur, total)) => format!("{cur}/{total}"),
            None => "0/0".to_owned(),
        };
        body = body.push(
            row![
                text("Search").size(13),
                text_input("text in scrollback…", q)
                    .id(SEARCH_INPUT_ID)
                    .on_input(Message::SearchQuery)
                    .on_submit(Message::SearchNext)
                    .padding(4),
                text(count).size(13),
                text("Enter next  Ctrl+Shift+Alt+G prev  Esc close").size(12),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
    }
    body = body.push(text(hint).size(12));
    container(body)
        .padding([4, 8])
        .width(Fill)
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(theme.extended_palette().background.base.color.into()),
            ..container::Style::default()
        })
        .into()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_answers_are_collected_per_prompt() {
        let mut s = SessionView::new(1, ConnTarget::Saved("h".into()), "h".into(), (80, 24), 100);
        s.begin_auth(
            "kbd".into(),
            String::new(),
            vec![
                AuthPromptItem {
                    prompt: "a".into(),
                    echo: true,
                },
                AuthPromptItem {
                    prompt: "b".into(),
                    echo: false,
                },
            ],
        );
        s.input = Sensitive::new("one".into());
        assert!(s.answer().is_none());
        assert_eq!(s.current_prompt().map(|p| p.prompt.as_str()), Some("b"));
        s.input = Sensitive::new("two".into());
        assert_eq!(s.answer().map(|a| a.len()), Some(2));
        assert_eq!(s.phase, Phase::Connecting);
    }
}
