//! SSH client + session actor (S4-T01…T04).
//!
//! One tokio task per session owns the russh handle and shell channel. The UI talks to it with
//! [`SessionCommand`]s and receives [`SessionEvent`]s. Secrets are never put in events.

use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey};
use russh::{ChannelMsg, MethodKind};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::{mpsc, oneshot};

use crate::Error;
use crate::known_hosts::{HostKeyInfo, HostKeyVerdict, KnownHostsStore, verdict};

/// A private key to authenticate with (OpenSSH PEM, possibly encrypted).
#[derive(Clone)]
pub struct KeyAuth {
    pub pem: SecretString,
    pub passphrase: Option<SecretString>,
}

#[derive(Clone)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub key: Option<KeyAuth>,
    pub password: Option<SecretString>,
    pub term: String,
    pub cols: u32,
    pub rows: u32,
    pub keepalive: Option<Duration>,
    pub connect_timeout: Duration,
}

impl ConnectParams {
    pub fn new(host: impl Into<String>, port: u16, username: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            username: username.into(),
            key: None,
            password: None,
            term: "xterm-256color".into(),
            cols: 80,
            rows: 24,
            keepalive: Some(Duration::from_secs(30)),
            connect_timeout: Duration::from_secs(15),
        }
    }
}

impl std::fmt::Debug for ConnectParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectParams")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthPromptItem {
    pub prompt: String,
    pub echo: bool,
}

pub enum SessionCommand {
    Write(Bytes),
    Resize {
        cols: u32,
        rows: u32,
    },
    /// Answers to the last `AuthPrompt`, one per prompt item.
    AuthResponse(Vec<SecretString>),
    /// Answer to `HostKeyPrompt`: trust (and save) the key, or abort.
    HostKeyDecision(bool),
    Close,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    Connecting,
    HostKeyPrompt {
        host: String,
        port: u16,
        key_type: String,
        fingerprint: String,
        /// Set when a different key is already stored: possible man-in-the-middle.
        changed_from: Option<String>,
    },
    AuthPrompt {
        name: String,
        instructions: String,
        prompts: Vec<AuthPromptItem>,
    },
    Connected,
    Output(Bytes),
    Exited(Option<u32>),
    Closed,
    Error(Error),
}

/// Cheap handle for sending commands to a running session.
#[derive(Clone)]
pub struct SessionHandle {
    tx: mpsc::Sender<SessionCommand>,
}

impl SessionHandle {
    /// Returns false if the session has already ended.
    pub async fn send(&self, cmd: SessionCommand) -> bool {
        self.tx.send(cmd).await.is_ok()
    }

    pub fn try_send(&self, cmd: SessionCommand) -> bool {
        self.tx.try_send(cmd).is_ok()
    }
}

struct HostKeyRequest {
    info: HostKeyInfo,
    changed_from: Option<String>,
    reply: oneshot::Sender<bool>,
}

#[derive(Debug)]
enum HandlerError {
    Russh(russh::Error),
    Rejected,
    Store(String),
}

impl From<russh::Error> for HandlerError {
    fn from(e: russh::Error) -> Self {
        Self::Russh(e)
    }
}

impl From<HandlerError> for Error {
    fn from(e: HandlerError) -> Self {
        match e {
            HandlerError::Russh(e) => e.into(),
            HandlerError::Rejected => Error::HostKeyRejected,
            HandlerError::Store(s) => Error::Other(s),
        }
    }
}

struct Handler {
    host: String,
    port: u16,
    known: Arc<dyn KnownHostsStore>,
    prompts: mpsc::UnboundedSender<HostKeyRequest>,
}

impl client::Handler for Handler {
    type Error = HandlerError;

    async fn check_server_key(
        &mut self,
        server_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // Host certificates are not supported yet; fall back to refusing them.
        let russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } = server_key else {
            return Err(HandlerError::Store(
                "host certificates are not supported".into(),
            ));
        };
        let info = describe_key(key);
        let stored = self
            .known
            .lookup(&self.host, self.port, &info.key_type)
            .await
            .map_err(HandlerError::Store)?;
        let changed_from = match verdict(stored.as_deref(), &info.fingerprint) {
            HostKeyVerdict::Known => return Ok(true),
            HostKeyVerdict::Unknown => None,
            HostKeyVerdict::Mismatch { old_fingerprint } => Some(old_fingerprint),
        };
        let (reply, answer) = oneshot::channel();
        let replace = changed_from.is_some();
        self.prompts
            .send(HostKeyRequest {
                info: info.clone(),
                changed_from,
                reply,
            })
            .map_err(|_| HandlerError::Rejected)?;
        if !answer.await.unwrap_or(false) {
            return Err(HandlerError::Rejected);
        }
        self.known
            .save(&self.host, self.port, &info, replace)
            .await
            .map_err(HandlerError::Store)?;
        Ok(true)
    }
}

fn describe_key(key: &PublicKey) -> HostKeyInfo {
    HostKeyInfo {
        key_type: key.algorithm().as_str().to_owned(),
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        public_key: key.to_openssh().unwrap_or_default(),
    }
}

/// Start a session actor. Must be called inside a tokio runtime.
///
/// Returns the command handle and the event stream. The task ends (after a final `Closed` or
/// `Error` event) when the connection ends, the handle sends `Close`, or the receiver is dropped.
pub fn spawn(
    params: ConnectParams,
    known: Arc<dyn KnownHostsStore>,
) -> (SessionHandle, mpsc::Receiver<SessionEvent>) {
    let (cmd_tx, cmd_rx) = mpsc::channel(256);
    // Bounded: a slow UI backpressures the remote instead of buffering unboundedly.
    let (ev_tx, ev_rx) = mpsc::channel(64);
    tokio::spawn(async move {
        let mut actor = Actor {
            events: ev_tx,
            commands: cmd_rx,
        };
        let end = actor.run(params, known).await;
        match end {
            Ok(()) => actor.emit(SessionEvent::Closed).await,
            Err(e) => actor.emit(SessionEvent::Error(e)).await,
        };
    });
    (SessionHandle { tx: cmd_tx }, ev_rx)
}

struct Actor {
    events: mpsc::Sender<SessionEvent>,
    commands: mpsc::Receiver<SessionCommand>,
}

/// Flush interval / size for coalescing remote output into few UI events.
const FLUSH_EVERY: Duration = Duration::from_millis(16);
const FLUSH_BYTES: usize = 64 * 1024;
const MAX_KBD_ROUNDS: usize = 6;

impl Actor {
    async fn emit(&self, ev: SessionEvent) -> bool {
        self.events.send(ev).await.is_ok()
    }

    async fn run(
        &mut self,
        params: ConnectParams,
        known: Arc<dyn KnownHostsStore>,
    ) -> Result<(), Error> {
        self.emit(SessionEvent::Connecting).await;
        let mut handle = self.connect(&params, known).await?;
        self.authenticate(&mut handle, &params).await?;

        let channel = handle.channel_open_session().await.map_err(Error::from)?;
        channel
            .request_pty(true, &params.term, params.cols, params.rows, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;
        self.emit(SessionEvent::Connected).await;
        let result = self.shell_loop(channel).await;
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
        result
    }

    /// Connect and verify the host key, relaying the host-key prompt to the UI.
    async fn connect(
        &mut self,
        params: &ConnectParams,
        known: Arc<dyn KnownHostsStore>,
    ) -> Result<client::Handle<Handler>, Error> {
        let config = Arc::new(client::Config {
            keepalive_interval: params.keepalive,
            keepalive_max: 3,
            ..Default::default()
        });
        let (prompt_tx, mut prompt_rx) = mpsc::unbounded_channel::<HostKeyRequest>();
        let handler = Handler {
            host: params.host.clone(),
            port: params.port,
            known,
            prompts: prompt_tx,
        };
        let fut = client::connect(config, (params.host.as_str(), params.port), handler);
        tokio::pin!(fut);
        let deadline = tokio::time::sleep(params.connect_timeout);
        tokio::pin!(deadline);
        let mut pending: Option<oneshot::Sender<bool>> = None;
        // The connect timeout covers name resolution + TCP + handshake, but not the time the user
        // spends deciding on the host key.
        let mut deciding = false;
        loop {
            tokio::select! {
                res = &mut fut => return res.map_err(Error::from),
                () = &mut deadline, if !deciding => return Err(Error::Timeout),
                Some(req) = prompt_rx.recv() => {
                    deciding = true;
                    pending = Some(req.reply);
                    let ok = self.emit(SessionEvent::HostKeyPrompt {
                        host: params.host.clone(),
                        port: params.port,
                        key_type: req.info.key_type,
                        fingerprint: req.info.fingerprint,
                        changed_from: req.changed_from,
                    }).await;
                    if !ok { return Err(Error::Disconnected); }
                }
                cmd = self.commands.recv() => match cmd {
                    Some(SessionCommand::HostKeyDecision(accept)) => {
                        if let Some(p) = pending.take() { let _ = p.send(accept); }
                        deciding = false;
                        deadline.as_mut().reset(tokio::time::Instant::now() + params.connect_timeout);
                    }
                    Some(SessionCommand::Close) | None => return Err(Error::Disconnected),
                    Some(_) => {}
                }
            }
        }
    }

    /// Wait for the UI to answer an auth prompt. `None` when the session was closed.
    async fn ask(
        &mut self,
        name: &str,
        instructions: &str,
        prompts: Vec<AuthPromptItem>,
    ) -> Result<Vec<SecretString>, Error> {
        self.emit(SessionEvent::AuthPrompt {
            name: name.into(),
            instructions: instructions.into(),
            prompts,
        })
        .await;
        loop {
            match self.commands.recv().await {
                Some(SessionCommand::AuthResponse(r)) => return Ok(r),
                Some(SessionCommand::Close) | None => return Err(Error::Disconnected),
                Some(_) => {}
            }
        }
    }

    /// Order: public key → password → keyboard-interactive.
    async fn authenticate(
        &mut self,
        handle: &mut client::Handle<Handler>,
        params: &ConnectParams,
    ) -> Result<(), Error> {
        let user = params.username.as_str();

        if let Some(auth) = &params.key {
            let key = self.load_key(auth).await?;
            let hash = handle
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten();
            let res = handle
                .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await?;
            if res.success() {
                return Ok(());
            }
        }

        let mut password = params.password.clone();
        let mut asked = false;
        loop {
            let Some(pw) = password.take() else {
                if asked {
                    break;
                }
                asked = true;
                let answers = self
                    .ask(
                        "Password",
                        "",
                        vec![AuthPromptItem {
                            prompt: format!("{user}@{}'s password:", params.host),
                            echo: false,
                        }],
                    )
                    .await?;
                password = answers.into_iter().next();
                if password.is_none() {
                    break;
                }
                continue;
            };
            let res = handle
                .authenticate_password(user, pw.expose_secret())
                .await?;
            if res.success() {
                return Ok(());
            }
            let allowed = match res {
                russh::client::AuthResult::Failure {
                    remaining_methods, ..
                } => remaining_methods.contains(&MethodKind::Password),
                _ => false,
            };
            // A stored password that failed is not retried; ask once, then give up.
            if !allowed || asked {
                break;
            }
            asked = true;
            let answers = self
                .ask(
                    "Password",
                    "",
                    vec![AuthPromptItem {
                        prompt: format!("{user}@{}'s password:", params.host),
                        echo: false,
                    }],
                )
                .await?;
            password = answers.into_iter().next();
            if password.is_none() {
                break;
            }
        }

        if self.keyboard_interactive(handle, user).await? {
            return Ok(());
        }
        Err(Error::AuthFailed)
    }

    async fn keyboard_interactive(
        &mut self,
        handle: &mut client::Handle<Handler>,
        user: &str,
    ) -> Result<bool, Error> {
        let mut resp = handle
            .authenticate_keyboard_interactive_start(user, None)
            .await?;
        for _ in 0..MAX_KBD_ROUNDS {
            match resp {
                KeyboardInteractiveAuthResponse::Success => return Ok(true),
                KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
                KeyboardInteractiveAuthResponse::InfoRequest {
                    name,
                    instructions,
                    prompts,
                } => {
                    let answers = if prompts.is_empty() {
                        Vec::new()
                    } else {
                        let items = prompts
                            .into_iter()
                            .map(|p| AuthPromptItem {
                                prompt: p.prompt,
                                echo: p.echo,
                            })
                            .collect();
                        self.ask(&name, &instructions, items)
                            .await?
                            .iter()
                            .map(|s| s.expose_secret().to_owned())
                            .collect()
                    };
                    resp = handle
                        .authenticate_keyboard_interactive_respond(answers)
                        .await?;
                }
            }
        }
        Ok(false)
    }

    /// Decode the key, asking for a passphrase when it is encrypted and none is stored.
    async fn load_key(&mut self, auth: &KeyAuth) -> Result<PrivateKey, Error> {
        let key = PrivateKey::from_openssh(auth.pem.expose_secret().trim())
            .map_err(|e| Error::Key(e.to_string()))?;
        if !key.is_encrypted() {
            return Ok(key);
        }
        let mut pass = auth.passphrase.clone();
        for _ in 0..3 {
            let p = match pass.take() {
                Some(p) => p,
                None => self
                    .ask(
                        "Passphrase",
                        "",
                        vec![AuthPromptItem {
                            prompt: "Key passphrase:".into(),
                            echo: false,
                        }],
                    )
                    .await?
                    .into_iter()
                    .next()
                    .ok_or(Error::AuthFailed)?,
            };
            if let Ok(k) = key.decrypt(p.expose_secret()) {
                return Ok(k);
            }
        }
        Err(Error::Key("wrong passphrase".into()))
    }

    async fn shell_loop(&mut self, mut channel: russh::Channel<client::Msg>) -> Result<(), Error> {
        let mut buf = BytesMut::new();
        let mut tick = tokio::time::interval(FLUSH_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut exit_code = None;
        loop {
            tokio::select! {
                msg = channel.wait() => match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        buf.extend_from_slice(&data);
                        if buf.len() >= FLUSH_BYTES && !self.flush(&mut buf).await {
                            return Ok(());
                        }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        self.flush(&mut buf).await;
                        self.emit(SessionEvent::Exited(exit_code)).await;
                        return Ok(());
                    }
                    Some(_) => {}
                },
                _ = tick.tick(), if !buf.is_empty() => {
                    if !self.flush(&mut buf).await { return Ok(()); }
                }
                cmd = self.commands.recv() => match cmd {
                    Some(SessionCommand::Write(b)) => channel.data_bytes(b).await?,
                    Some(SessionCommand::Resize { cols, rows }) => channel.window_change(cols, rows, 0, 0).await?,
                    Some(SessionCommand::Close) | None => {
                        let _ = channel.close().await;
                        return Ok(());
                    }
                    Some(_) => {}
                }
            }
        }
    }

    /// Send buffered output. False when the UI side is gone.
    async fn flush(&self, buf: &mut BytesMut) -> bool {
        if buf.is_empty() {
            return true;
        }
        self.emit(SessionEvent::Output(buf.split().freeze())).await
    }
}
