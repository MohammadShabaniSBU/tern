//! SSH session state and view (S4-T05/T06): the bridge between the `tern-ssh` actor and iced.
//!
//! Each tab owns one session plus its terminal emulator (S5). The input line below the terminal is
//! only used for authentication prompts.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use iced::futures::{Stream, stream};
use iced::widget::canvas::Cache;
use iced::widget::{column, container, row, space, text, text_input};
use iced::{Element, Fill};
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
use crate::theme;
use crate::theme::style::{self, Dot};
use crate::theme::tokens::*;

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
    let mut seen = Vec::new();
    resolve_host(store, id, &mut seen).await
}

/// Resolve a saved host and, recursively, its jump hosts. `seen` holds the ids already on the
/// chain: meeting one again is a cycle.
async fn resolve_host(
    store: &Store,
    id: HostId,
    seen: &mut Vec<HostId>,
) -> Result<ConnectParams, String> {
    if seen.contains(&id) {
        return Err("Jump host cycle: a host is its own jump host".to_owned());
    }
    if seen.len() > tern_ssh::MAX_JUMPS {
        return Err("Too many jump hosts in the chain".to_owned());
    }
    seen.push(id);
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
    if let Some(jid) = host.jump_host_id {
        let jump = Box::pin(resolve_host(store, jid, seen))
            .await
            .map_err(|e| {
                if e.starts_with("Jump host") || e.starts_with("Too many") {
                    e
                } else {
                    format!("Jump host: {e}")
                }
            })?;
        params.jump = Some(Box::new(jump));
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

fn phase_dot(phase: &Phase) -> Dot {
    match phase {
        Phase::Connected => Dot::Live,
        Phase::Connecting | Phase::HostKey | Phase::Auth { .. } => Dot::Connecting,
        Phase::Disconnected(None) => Dot::Idle,
        Phase::Disconnected(Some(_)) => Dot::Failed,
    }
}

fn tab_button<'a>(
    content: impl Into<Element<'a, Message>>,
    active: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let tab = container(content)
        .height(TAB_H)
        .padding([0.0, SPACE_12])
        .align_y(iced::Center)
        .style(style::tab(active));
    // The accent top edge is the outer container's 2 px padding, so the tab keeps its content width.
    let edged = container(tab)
        .padding(iced::Padding::ZERO.top(2))
        .style(style::tab_edge(active));
    iced::widget::mouse_area(edged).on_press(on_press).into()
}

/// The tab strip: a "Hosts" home tab followed by one tab per session.
pub fn tab_bar<'a>(tabs: &'a [SessionView], active: Option<usize>) -> Element<'a, Message> {
    let mut strip = row![].spacing(SPACE_4).align_y(iced::Alignment::End);
    strip = strip.push(tab_button(
        row![
            text("Hosts").size(TEXT_BODY).font(theme::UI_MEDIUM),
            text(if active.is_none() { "Alt 0" } else { "" })
                .size(MONO_KBD)
                .font(theme::MONO)
                .style(style::text_muted),
        ]
        .spacing(SPACE_8),
        active.is_none(),
        Message::SelectHome,
    ));
    for (i, t) in tabs.iter().enumerate() {
        let is_active = active == Some(i);
        let state_note: Element<'a, Message> = match &t.phase {
            Phase::Connecting | Phase::HostKey | Phase::Auth { .. } => text("connecting…")
                .size(TEXT_CAPTION)
                .style(style::text_warning)
                .into(),
            Phase::Disconnected(Some(_)) => text("lost")
                .size(TEXT_CAPTION)
                .style(style::text_error)
                .into(),
            _ => space().into(),
        };
        let label = row![
            container(space())
                .width(STATUS_DOT)
                .height(STATUS_DOT)
                .style(style::dot(phase_dot(&t.phase), STATUS_DOT)),
            text(t.display_name())
                .size(TEXT_BODY)
                .font(if is_active {
                    theme::UI_SEMIBOLD
                } else {
                    theme::UI
                })
                .style(if is_active {
                    style::text_primary
                } else {
                    style::text_secondary
                }),
            state_note,
            text(if is_active {
                format!("Alt {}", i + 1)
            } else {
                String::new()
            })
            .size(MONO_KBD)
            .font(theme::MONO)
            .style(style::text_muted),
        ]
        .spacing(SPACE_8)
        .align_y(iced::Center);
        strip = strip.push(tab_button(label, is_active, Message::SelectTab(i)));
    }
    container(
        row![
            strip,
            space().width(Fill),
            row![
                kbd_hint("Ctrl K"),
                text("commands").size(TEXT_SMALL).style(style::text_muted)
            ]
            .spacing(SPACE_6)
            .align_y(iced::Center)
        ]
        .align_y(iced::Alignment::End)
        .padding([0.0, SPACE_8]),
    )
    .height(TAB_BAR_H)
    .align_y(iced::alignment::Vertical::Bottom)
    .width(Fill)
    .style(style::tab_bar)
    .into()
}

fn kbd_hint<'a>(label: &'static str) -> Element<'a, Message> {
    container(text(label).size(MONO_KBD).font(theme::MONO))
        .padding([0.0, 5.0])
        .style(style::kbd)
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

    let mut body = column![].spacing(SPACE_6);
    if let Some(status) = status {
        body = body.push(text(status).size(TEXT_LABEL));
    }
    body = body.push(container(term).width(Fill).height(Fill));
    if let Phase::Auth {
        instructions,
        prompts,
        ..
    } = &s.phase
    {
        if !instructions.is_empty() {
            body = body.push(text(instructions.as_str()).size(TEXT_BODY));
        }
        if let Some(p) = prompts.get(s.answers.len()) {
            body = body.push(text(p.prompt.as_str()).size(TEXT_LABEL));
        }
    }
    if matches!(s.phase, Phase::Auth { .. }) {
        let secure = s.current_prompt().is_some_and(|p| !p.echo);
        body = body.push(
            text_input("", s.input.as_str())
                .style(crate::theme::style::input(false))
                .id(SESSION_INPUT_ID)
                .secure(secure)
                .on_input(|v| Message::SessionInput(Sensitive::new(v)))
                .on_submit(Message::SessionSubmit)
                .padding(SPACE_6),
        );
    }
    if let Some(q) = &s.search {
        let count = match s.term.search_status() {
            Some((cur, total)) => format!("{cur}/{total}"),
            None => "0/0".to_owned(),
        };
        body = body.push(
            row![
                text("Search").size(TEXT_BODY),
                text_input("text in scrollback…", q)
                    .style(crate::theme::style::input(false))
                    .id(SEARCH_INPUT_ID)
                    .on_input(Message::SearchQuery)
                    .on_submit(Message::SearchNext)
                    .padding(SPACE_4),
                text(count).size(TEXT_BODY),
                text("Enter next  Ctrl+Shift+Alt+G prev  Esc close").size(TEXT_SMALL),
            ]
            .spacing(SPACE_8)
            .align_y(iced::Center),
        );
    }
    body = body.push(text(hint).size(TEXT_SMALL));
    container(body)
        .padding([4, 8])
        .width(Fill)
        .height(Fill)
        .style(crate::theme::style::terminal_surface)
        .into()
}
#[cfg(test)]
mod tests {
    use super::*;
    use tern_core::Host;

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

    async fn store_with_hosts() -> (Store, Host, Host, Host) {
        let store = Store::open_in_memory().unwrap();
        let kdf = tern_vault::KdfParams::new(8, 1, 1).unwrap();
        store
            .create_vault_with_params(secrecy::SecretString::from("pw"), kdf)
            .await
            .unwrap();
        let mk = |l: &str| {
            let mut h = Host::new(l, format!("{l}.example"));
            h.username = Some("u".into());
            h
        };
        let (a, b, c) = (mk("a"), mk("b"), mk("c"));
        for h in [&a, &b, &c] {
            store.insert_host(h.clone()).await.unwrap();
        }
        (store, a, b, c)
    }

    #[tokio::test]
    async fn resolve_follows_the_jump_chain() {
        let (store, mut a, mut b, c) = store_with_hosts().await;
        a.jump_host_id = Some(b.id);
        b.jump_host_id = Some(c.id);
        store.update_host(a.clone()).await.unwrap();
        store.update_host(b).await.unwrap();
        let p = resolve(&store, &ConnTarget::Saved(a.id.to_string()))
            .await
            .unwrap();
        assert_eq!(p.host, "a.example");
        let j1 = p.jump.as_deref().unwrap();
        assert_eq!(j1.host, "b.example");
        assert_eq!(j1.jump.as_deref().unwrap().host, "c.example");
    }

    #[tokio::test]
    async fn resolve_rejects_jump_cycles() {
        let (store, mut a, mut b, _c) = store_with_hosts().await;
        a.jump_host_id = Some(b.id);
        b.jump_host_id = Some(a.id);
        store.update_host(a.clone()).await.unwrap();
        store.update_host(b).await.unwrap();
        let err = resolve(&store, &ConnTarget::Saved(a.id.to_string()))
            .await
            .unwrap_err();
        assert!(err.contains("cycle"), "{err}");
    }
}
