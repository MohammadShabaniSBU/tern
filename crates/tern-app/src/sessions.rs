//! SSH session state and view (S4-T05/T06): the bridge between the `tern-ssh` actor and iced.
//!
//! One session at a time until terminal tabs exist (Sprint 5). The output pane is a plain,
//! sanitised text log and the input line is temporary.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use iced::futures::{Stream, stream};
use iced::widget::{column, container, scrollable, text, text_input};
use iced::{Element, Fill, Theme};
use secrecy::SecretString;
use tern_core::{HostId, Identity};
use tern_ssh::known_hosts::BoxFuture;
use tern_ssh::{
    AuthPromptItem, ConnectParams, HostKeyInfo, KeyAuth, KnownHostsStore, SessionEvent,
    SessionHandle,
};
use tern_store::Store;
use tokio::sync::mpsc::Receiver;

use crate::app::Message;
use crate::auth::Sensitive;
use crate::palette::QuickTarget;

pub const SESSION_INPUT_ID: &str = "session-input";
/// Output kept in memory (characters). Older text is dropped.
const MAX_LOG: usize = 256 * 1024;
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
    pub log: RawLog,
    /// The input line (command text, or an auth answer which may be a password).
    pub input: Sensitive,
    /// Answers collected so far for the current auth prompt.
    answers: Vec<SecretString>,
    pub handle: Option<SessionHandle>,
    pub source: Option<EventSource>,
}

impl SessionView {
    pub fn new(id: SessionId, target: ConnTarget, label: String) -> Self {
        Self {
            id,
            target,
            label,
            phase: Phase::Connecting,
            log: RawLog::default(),
            input: Sensitive::default(),
            answers: Vec::new(),
            handle: None,
            source: None,
        }
    }

    pub fn is_live(&self) -> bool {
        !matches!(self.phase, Phase::Disconnected(_))
    }

    pub fn accepts_input(&self) -> bool {
        matches!(self.phase, Phase::Connected | Phase::Auth { .. })
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

/// Plain-text log of remote output: escape sequences and control characters are removed
/// (a real terminal arrives in Sprint 5), partial UTF-8 and partial escapes carry over.
#[derive(Debug, Default)]
pub struct RawLog {
    text: String,
    carry: Vec<u8>,
    esc: EscState,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum EscState {
    #[default]
    Text,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

impl RawLog {
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.carry.extend_from_slice(bytes);
        let carry = std::mem::take(&mut self.carry);
        let mut rest: &[u8] = &carry;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    self.push_str(s);
                    break;
                }
                Err(e) => {
                    let (good, bad) = rest.split_at(e.valid_up_to());
                    // valid_up_to guarantees `good` is valid UTF-8.
                    self.push_str(&String::from_utf8_lossy(good));
                    match e.error_len() {
                        Some(n) => {
                            self.push_str("\u{fffd}");
                            rest = &bad[n..];
                        }
                        None => {
                            self.carry = bad.to_vec();
                            break;
                        }
                    }
                }
            }
        }
        self.trim();
    }

    fn push_str(&mut self, s: &str) {
        for c in s.chars() {
            match self.esc {
                EscState::Text => match c {
                    '\u{1b}' => self.esc = EscState::Esc,
                    '\n' | '\t' => self.text.push(c),
                    '\u{8}' => {
                        // Backspace: erase the previous character on the current line.
                        if self.text.chars().next_back().is_some_and(|p| p != '\n') {
                            self.text.pop();
                        }
                    }
                    c if c.is_control() => {}
                    c => self.text.push(c),
                },
                EscState::Esc => {
                    self.esc = match c {
                        '[' => EscState::Csi,
                        ']' => EscState::Osc,
                        // Two-character escape (e.g. `ESC =`): done.
                        _ => EscState::Text,
                    };
                }
                EscState::Csi => {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        self.esc = EscState::Text;
                    }
                }
                EscState::Osc => match c {
                    '\u{7}' => self.esc = EscState::Text,
                    '\u{1b}' => self.esc = EscState::OscEsc,
                    _ => {}
                },
                EscState::OscEsc => self.esc = EscState::Text,
            }
        }
    }

    fn trim(&mut self) {
        if self.text.len() > MAX_LOG {
            let mut cut = self.text.len() - MAX_LOG / 2;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
        }
    }

    /// Local status line (e.g. "Connected"), separated from remote output.
    pub fn note(&mut self, line: &str) {
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
        self.text.push_str(line);
        self.text.push('\n');
    }
}

pub fn view<'a>(s: &'a SessionView, spinner_frame: usize) -> Element<'a, Message> {
    let status = match &s.phase {
        Phase::Connecting | Phase::HostKey => format!(
            "{} Connecting to {}…",
            SPINNER[spinner_frame % SPINNER.len()],
            s.label
        ),
        Phase::Auth { name, .. } => format!("{} — {name}", s.label),
        Phase::Connected => format!("{} — connected", s.label),
        Phase::Disconnected(None) => format!("{} — disconnected", s.label),
        Phase::Disconnected(Some(why)) => format!("{} — {why}", s.label),
    };
    let hint = match &s.phase {
        Phase::Disconnected(_) => "r reconnect   Esc / Ctrl+Shift+W close",
        Phase::Connected => {
            "Enter send   Esc normal mode (Esc again closes)   Ctrl+Shift+W disconnect"
        }
        _ => "Esc cancel",
    };
    let mut body = column![
        text(status).size(16),
        container(
            scrollable(text(s.log.as_str()).size(13).font(iced::Font::MONOSPACE))
                .anchor_bottom()
                .width(Fill)
                .height(Fill)
        )
        .padding(8)
        .width(Fill)
        .height(Fill)
        .style(container::bordered_box),
    ]
    .spacing(8);
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
    if s.accepts_input() {
        let secure = s.current_prompt().is_some_and(|p| !p.echo);
        let placeholder = if matches!(s.phase, Phase::Connected) {
            "type a command (temporary input line)"
        } else {
            ""
        };
        body = body.push(
            text_input(placeholder, s.input.as_str())
                .id(SESSION_INPUT_ID)
                .secure(secure)
                .on_input(|v| Message::SessionInput(Sensitive::new(v)))
                .on_submit(Message::SessionSubmit)
                .padding(6),
        );
    }
    body = body.push(text(hint).size(12));
    container(body.spacing(8))
        .padding(12)
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
    fn strips_escape_sequences_and_controls() {
        let mut l = RawLog::default();
        l.push_bytes(b"\x1b[1;32muser@host\x1b[0m:~$ ls\r\n\x1b]0;title\x07done\n");
        assert_eq!(l.as_str(), "user@host:~$ ls\ndone\n");
    }

    #[test]
    fn partial_utf8_and_escape_carry_over_chunks() {
        let mut l = RawLog::default();
        let bytes = "héllo".as_bytes();
        l.push_bytes(&bytes[..2]);
        l.push_bytes(&bytes[2..]);
        assert_eq!(l.as_str(), "héllo");
        l.push_bytes(b"\x1b[3");
        l.push_bytes(b"1mred");
        assert_eq!(l.as_str(), "héllored");
    }

    #[test]
    fn backspace_erases_within_the_line_only() {
        let mut l = RawLog::default();
        l.push_bytes(b"ab\x08c\n\x08x");
        assert_eq!(l.as_str(), "ac\nx");
    }

    #[test]
    fn log_is_capped() {
        let mut l = RawLog::default();
        for _ in 0..40 {
            l.push_bytes(&vec![b'x'; 16 * 1024]);
        }
        assert!(l.as_str().len() <= MAX_LOG);
    }

    #[test]
    fn auth_answers_are_collected_per_prompt() {
        let mut s = SessionView::new(1, ConnTarget::Saved("h".into()), "h".into());
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
