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
use crate::pool::ConnectionPool;
use crate::tunnel::RemoteForwards;

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
    /// Bastion to tunnel through (ProxyJump). Its own `jump` makes a chain; the outermost hop is
    /// connected first. Every hop has its host key verified and is authenticated on its own.
    pub jump: Option<Box<ConnectParams>>,
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
            jump: None,
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
#[derive(Debug, Clone)]
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
pub(crate) enum HandlerError {
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

pub(crate) struct Handler {
    host: String,
    port: u16,
    known: Arc<dyn KnownHostsStore>,
    prompts: mpsc::UnboundedSender<HostKeyRequest>,
    forwards: RemoteForwards,
}

impl client::Handler for Handler {
    type Error = HandlerError;

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        match crate::tunnel::remote_target(&self.forwards, connected_port) {
            Some(target) => {
                reply.accept().await;
                tokio::spawn(crate::tunnel::serve_remote(channel, target));
            }
            None => {
                reply
                    .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                    .await;
            }
        }
        Ok(())
    }

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
    spawn_pooled(params, known, None)
}

/// Like [`spawn`], but shares an authenticated connection through `pool` (keyed by the given
/// string): an existing live connection is reused without authenticating again, and a freshly
/// authenticated one is registered for later users (e.g. tunnels).
pub fn spawn_pooled(
    params: ConnectParams,
    known: Arc<dyn KnownHostsStore>,
    pool: Option<(ConnectionPool, String)>,
) -> (SessionHandle, mpsc::Receiver<SessionEvent>) {
    let (cmd_tx, cmd_rx) = mpsc::channel(256);
    // Bounded: a slow UI backpressures the remote instead of buffering unboundedly.
    let (ev_tx, ev_rx) = mpsc::channel(64);
    tokio::spawn(async move {
        let mut actor = Actor {
            events: ev_tx,
            commands: cmd_rx,
            headless: false,
        };
        let end = actor.run(params, known, pool).await;
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
    /// No UI attached (tunnels): prompts fail with an explanatory error instead of waiting.
    headless: bool,
}

/// An authenticated SSH connection that can be shared by several users (terminal sessions and
/// tunnels). The connection is closed when the last [`SharedConn`] is dropped.
pub struct Conn {
    handle: Option<client::Handle<Handler>>,
    /// Jump-host connections carrying this one; kept alive for as long as it is used.
    _hops: Vec<client::Handle<Handler>>,
    pub(crate) forwards: RemoteForwards,
}

pub type SharedConn = Arc<Conn>;

impl Conn {
    fn new(
        handle: client::Handle<Handler>,
        hops: Vec<client::Handle<Handler>>,
        forwards: RemoteForwards,
    ) -> SharedConn {
        Arc::new(Self {
            handle: Some(handle),
            _hops: hops,
            forwards,
        })
    }

    pub(crate) fn handle(&self) -> Result<&client::Handle<Handler>, Error> {
        // `handle` is only taken in `drop`.
        self.handle.as_ref().ok_or(Error::Disconnected)
    }

    pub fn is_closed(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_closed())
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take()
            && let Ok(rt) = tokio::runtime::Handle::try_current()
        {
            rt.spawn(async move {
                let _ = handle
                    .disconnect(russh::Disconnect::ByApplication, "", "en")
                    .await;
            });
        }
    }
}

/// Maximum number of jump hosts in a chain.
pub const MAX_JUMPS: usize = 8;

/// The hops to connect, outermost jump host first and the target last.
fn hop_path(params: &ConnectParams) -> Result<Vec<&ConnectParams>, Error> {
    let mut path = vec![params];
    let mut cur = params;
    while let Some(j) = cur.jump.as_deref() {
        if path.len() > MAX_JUMPS {
            return Err(Error::Other("too many jump hosts".into()));
        }
        path.push(j);
        cur = j;
    }
    path.reverse();
    Ok(path)
}

const NEEDS_TERMINAL: &str = "needs interactive input; connect once from a terminal first";

/// Connect and authenticate without a UI. Unknown/changed host keys and interactive prompts
/// fail instead of waiting for an answer.
pub(crate) async fn connect_headless(
    params: &ConnectParams,
    known: Arc<dyn KnownHostsStore>,
) -> Result<SharedConn, Error> {
    let (_cmd_tx, cmd_rx) = mpsc::channel(1);
    let (ev_tx, _ev_rx) = mpsc::channel(1);
    let mut actor = Actor {
        events: ev_tx,
        commands: cmd_rx,
        headless: true,
    };
    let forwards = RemoteForwards::default();
    let (handle, hops) = actor.connect_chain(params, known, forwards.clone()).await?;
    Ok(Conn::new(handle, hops, forwards))
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
        pool: Option<(ConnectionPool, String)>,
    ) -> Result<(), Error> {
        self.emit(SessionEvent::Connecting).await;
        let reused = pool.as_ref().and_then(|(p, key)| p.get(key));
        let conn = match reused {
            Some(c) => c,
            None => {
                let forwards = RemoteForwards::default();
                let (handle, hops) = self.connect_chain(&params, known, forwards.clone()).await?;
                let conn = Conn::new(handle, hops, forwards);
                if let Some((p, key)) = &pool {
                    p.insert(key, &conn);
                }
                conn
            }
        };

        let channel = conn
            .handle()?
            .channel_open_session()
            .await
            .map_err(Error::from)?;
        channel
            .request_pty(true, &params.term, params.cols, params.rows, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;
        self.emit(SessionEvent::Connected).await;
        // Dropping the last `SharedConn` disconnects; tunnels may keep it alive.
        self.shell_loop(channel).await
    }

    /// Connect + authenticate the target, going through every jump host first. Returns the
    /// target handle and the (authenticated) hop handles, outermost first.
    async fn connect_chain(
        &mut self,
        params: &ConnectParams,
        known: Arc<dyn KnownHostsStore>,
        forwards: RemoteForwards,
    ) -> Result<(client::Handle<Handler>, Vec<client::Handle<Handler>>), Error> {
        let path = hop_path(params)?;
        let mut hops: Vec<client::Handle<Handler>> = Vec::new();
        for (i, hop) in path.iter().enumerate() {
            let last = i + 1 == path.len();
            let via = match hops.last() {
                None => None,
                Some(prev) => Some(
                    prev.channel_open_direct_tcpip(
                        hop.host.clone(),
                        u32::from(hop.port),
                        "127.0.0.1",
                        0,
                    )
                    .await
                    .map_err(|e| {
                        Error::Other(format!(
                            "jump host could not reach {}:{}: {e}",
                            hop.host, hop.port
                        ))
                    })?
                    .into_stream(),
                ),
            };
            let fwd = if last {
                forwards.clone()
            } else {
                RemoteForwards::default()
            };
            let mut handle = self.connect(hop, known.clone(), fwd, via).await?;
            self.authenticate(&mut handle, hop).await?;
            if last {
                return Ok((handle, hops));
            }
            hops.push(handle);
        }
        Err(Error::Other("empty connection path".into()))
    }

    /// Connect and verify the host key, relaying the host-key prompt to the UI. `via` is an
    /// already-open channel to the hop's address (jump host), otherwise a direct TCP connection.
    async fn connect(
        &mut self,
        params: &ConnectParams,
        known: Arc<dyn KnownHostsStore>,
        forwards: RemoteForwards,
        via: Option<russh::ChannelStream<client::Msg>>,
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
            forwards,
        };
        let mut fut: std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<client::Handle<Handler>, HandlerError>>
                    + Send,
            >,
        > = match via {
            Some(stream) => Box::pin(client::connect_stream(config, stream, handler)),
            None => Box::pin(client::connect(
                config,
                (params.host.clone(), params.port),
                handler,
            )),
        };
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
                    if self.headless {
                        let _ = req.reply.send(false);
                        return Err(Error::Other(format!("host key not trusted yet: {NEEDS_TERMINAL}")));
                    }
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
        if self.headless {
            return Err(Error::Other(format!("authentication {NEEDS_TERMINAL}")));
        }
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

#[cfg(test)]
mod jump_tests {
    use super::*;

    #[test]
    fn connect_params_debug_never_prints_secrets() {
        let mut p = ConnectParams::new("h", 22, "u");
        p.password = Some(SecretString::from("pw-secret-1".to_owned()));
        p.key = Some(KeyAuth {
            pem: SecretString::from("PEM-SECRET".to_owned()),
            passphrase: Some(SecretString::from("pp-secret-2".to_owned())),
        });
        p.jump = Some(Box::new(p.clone()));
        let dbg = format!("{p:?}");
        for needle in ["pw-secret-1", "PEM-SECRET", "pp-secret-2"] {
            assert!(!dbg.contains(needle), "{needle} leaked: {dbg}");
        }
    }

    #[test]
    fn hop_path_is_outermost_first() {
        let mut a = ConnectParams::new("a", 22, "u");
        let mut b = ConnectParams::new("b", 22, "u");
        let c = ConnectParams::new("c", 22, "u");
        b.jump = Some(Box::new(c));
        a.jump = Some(Box::new(b));
        let hosts: Vec<_> = hop_path(&a)
            .unwrap()
            .iter()
            .map(|p| p.host.clone())
            .collect();
        assert_eq!(hosts, ["c", "b", "a"]);
    }

    #[test]
    fn hop_path_rejects_runaway_chains() {
        let mut p = ConnectParams::new("h0", 22, "u");
        for i in 1..=MAX_JUMPS + 2 {
            let mut n = ConnectParams::new(format!("h{i}"), 22, "u");
            n.jump = Some(Box::new(p));
            p = n;
        }
        assert!(hop_path(&p).is_err());
    }
}
