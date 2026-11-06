#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integration tests against a real OpenSSH server. Skipped unless `TERN_SSH_TEST_ADDR`
//! (`host:port`) is set; also `TERN_SSH_TEST_USER` and `TERN_SSH_TEST_PASSWORD`.
//!
//! Example (local sshd or `linuxserver/openssh-server` with password auth):
//! `TERN_SSH_TEST_ADDR=127.0.0.1:2222 TERN_SSH_TEST_USER=u TERN_SSH_TEST_PASSWORD=p cargo test -p tern-ssh --test live`

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::SecretString;
use tern_core::TunnelKind;
use tern_ssh::known_hosts::BoxFuture;
use tern_ssh::tunnel::{self, TunnelSpec};
use tern_ssh::{
    ConnectParams, ConnectionPool, Error, HostKeyInfo, KnownHostsStore, SessionCommand,
    SessionEvent, spawn, spawn_pooled,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::Receiver;

#[derive(Default)]
struct MemKnown(Mutex<HashMap<(String, u16, String), String>>);

impl KnownHostsStore for MemKnown {
    fn lookup<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        key_type: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(async move {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(host.into(), port, key_type.into()))
                .cloned())
        })
    }
    fn save<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        key: &'a HostKeyInfo,
        _replace: bool,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.0.lock().unwrap().insert(
                (host.into(), port, key.key_type.clone()),
                key.fingerprint.clone(),
            );
            Ok(())
        })
    }
}

struct Env {
    host: String,
    port: u16,
    user: String,
    password: String,
}

fn env() -> Option<Env> {
    let addr = std::env::var("TERN_SSH_TEST_ADDR").ok()?;
    let (host, port) = addr.rsplit_once(':')?;
    Some(Env {
        host: host.into(),
        port: port.parse().ok()?,
        user: std::env::var("TERN_SSH_TEST_USER").ok()?,
        password: std::env::var("TERN_SSH_TEST_PASSWORD").ok()?,
    })
}

async fn next(rx: &mut Receiver<SessionEvent>) -> SessionEvent {
    tokio::time::timeout(Duration::from_secs(20), rx.recv())
        .await
        .expect("timed out waiting for event")
        .expect("event stream ended")
}

fn params(e: &Env, password: &str) -> ConnectParams {
    let mut p = ConnectParams::new(&e.host, e.port, &e.user);
    p.password = Some(SecretString::from(password.to_owned()));
    p
}

#[tokio::test]
async fn password_login_runs_a_command_and_closes() {
    let Some(e) = env() else { return };
    let known = Arc::new(MemKnown::default());
    let (h, mut rx) = spawn(params(&e, &e.password), known.clone());
    let mut out = String::new();
    let mut prompted = false;
    loop {
        match next(&mut rx).await {
            SessionEvent::HostKeyPrompt { changed_from, .. } => {
                assert!(changed_from.is_none());
                prompted = true;
                h.send(SessionCommand::HostKeyDecision(true)).await;
            }
            SessionEvent::Connected => {
                h.send(SessionCommand::Write("echo tern-$((40+2))\nexit\n".into()))
                    .await;
            }
            SessionEvent::Output(b) => out.push_str(&String::from_utf8_lossy(&b)),
            SessionEvent::Exited(_) | SessionEvent::Closed => break,
            SessionEvent::Error(err) => panic!("{err}"),
            _ => {}
        }
    }
    assert!(prompted, "first connect must prompt for the host key");
    assert!(out.contains("tern-42"), "output was: {out:?}");

    // Second connect: key is known, no prompt.
    let (h, mut rx) = spawn(params(&e, &e.password), known);
    loop {
        match next(&mut rx).await {
            SessionEvent::HostKeyPrompt { .. } => panic!("known host must not prompt"),
            SessionEvent::Connected => {
                h.send(SessionCommand::Close).await;
            }
            SessionEvent::Closed | SessionEvent::Exited(_) => break,
            SessionEvent::Error(err) => panic!("{err}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn wrong_password_is_a_clear_error_not_a_hang() {
    let Some(e) = env() else { return };
    let known = Arc::new(MemKnown::default());
    let (h, mut rx) = spawn(params(&e, "definitely-wrong"), known);
    loop {
        match next(&mut rx).await {
            SessionEvent::HostKeyPrompt { .. } => {
                h.send(SessionCommand::HostKeyDecision(true)).await;
            }
            // Servers may fall back to keyboard-interactive; answer wrongly.
            SessionEvent::AuthPrompt { prompts, .. } => {
                let a = prompts
                    .iter()
                    .map(|_| SecretString::from("nope".to_owned()));
                h.send(SessionCommand::AuthResponse(a.collect())).await;
            }
            SessionEvent::Error(Error::AuthFailed) => break,
            other => assert!(
                matches!(other, SessionEvent::Connecting),
                "unexpected {other:?}"
            ),
        }
    }
}

#[tokio::test]
async fn changed_host_key_is_flagged_and_rejection_stops_the_connection() {
    let Some(e) = env() else { return };
    let known = Arc::new(MemKnown::default());
    // Pre-seed every common key type with a bogus fingerprint.
    for t in ["ssh-ed25519", "ecdsa-sha2-nistp256", "ssh-rsa"] {
        known
            .0
            .lock()
            .unwrap()
            .insert((e.host.clone(), e.port, t.into()), "SHA256:bogus".into());
    }
    let (h, mut rx) = spawn(params(&e, &e.password), known);
    loop {
        match next(&mut rx).await {
            SessionEvent::HostKeyPrompt { changed_from, .. } => {
                assert_eq!(changed_from.as_deref(), Some("SHA256:bogus"));
                h.send(SessionCommand::HostKeyDecision(false)).await;
            }
            SessionEvent::Error(Error::HostKeyRejected) => break,
            SessionEvent::Connecting => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn refused_connection_reports_refused() {
    // Port 1 on localhost is essentially never open.
    let known = Arc::new(MemKnown::default());
    let (_h, mut rx) = spawn(ConnectParams::new("127.0.0.1", 1, "x"), known);
    loop {
        match next(&mut rx).await {
            SessionEvent::Error(Error::Refused) => break,
            SessionEvent::Connecting => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn large_output_is_coalesced_into_few_events() {
    let Some(e) = env() else { return };
    let (h, mut rx) = spawn(params(&e, &e.password), Arc::new(MemKnown::default()));
    let (mut bytes, mut events) = (0usize, 0usize);
    loop {
        match next(&mut rx).await {
            SessionEvent::HostKeyPrompt { .. } => {
                h.send(SessionCommand::HostKeyDecision(true)).await;
            }
            SessionEvent::Connected => {
                h.send(SessionCommand::Write(
                    "head -c 30000000 /dev/zero | tr '\\0' 'y'; echo; echo END-MARK; exit\n".into(),
                ))
                .await;
            }
            SessionEvent::Output(b) => {
                bytes += b.len();
                events += 1;
            }
            SessionEvent::Exited(_) | SessionEvent::Closed => break,
            SessionEvent::Error(err) => panic!("{err}"),
            _ => {}
        }
    }
    assert!(bytes >= 30_000_000, "got {bytes} bytes");
    // 64 KiB flush threshold or 16 ms ticks: far fewer events than SSH packets (32 KiB max).
    assert!(
        events * 16 * 1024 < bytes,
        "{events} events for {bytes} bytes"
    );
}

/// Manual check: `TERN_SSH_KILL_TEST=1`, then stop the server while this waits.
#[tokio::test]
async fn server_death_ends_the_session_cleanly() {
    let Some(e) = env() else { return };
    if std::env::var("TERN_SSH_KILL_TEST").is_err() {
        return;
    }
    let (h, mut rx) = spawn(params(&e, &e.password), Arc::new(MemKnown::default()));
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(60), rx.recv())
            .await
            .expect("no event within 60 s of the server dying");
        match ev {
            Some(SessionEvent::HostKeyPrompt { .. }) => {
                h.send(SessionCommand::HostKeyDecision(true)).await;
            }
            Some(SessionEvent::Connected) => println!("READY"),
            Some(SessionEvent::Closed | SessionEvent::Error(_) | SessionEvent::Exited(_)) => {
                break;
            }
            Some(_) => {}
            None => panic!("stream ended without a final event"),
        }
    }
}

#[tokio::test]
async fn terminal_and_headless_user_share_one_connection() {
    let Some(e) = env() else { return };
    let known = Arc::new(MemKnown::default());
    let pool = ConnectionPool::new();
    let (h, mut rx) = spawn_pooled(
        params(&e, &e.password),
        known.clone(),
        Some((pool.clone(), "host-1".into())),
    );
    loop {
        match next(&mut rx).await {
            SessionEvent::HostKeyPrompt { .. } => {
                h.send(SessionCommand::HostKeyDecision(true)).await;
            }
            SessionEvent::Connected => break,
            SessionEvent::Error(err) => panic!("{err}"),
            _ => {}
        }
    }
    assert_eq!(pool.live(), 1);
    let shared = pool
        .get_or_connect("host-1", &params(&e, "wrong-on-purpose"), known)
        .await
        .expect("must reuse the terminal's connection, not authenticate again");
    assert_eq!(pool.live(), 1);

    // Closing the terminal keeps the connection alive for the other user.
    h.send(SessionCommand::Close).await;
    while !matches!(
        next(&mut rx).await,
        SessionEvent::Closed | SessionEvent::Error(_)
    ) {}
    assert!(pool.get("host-1").is_some());
    drop(shared);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(pool.get("host-1").is_none(), "unused connection must close");
}

async fn shared(e: &Env) -> tern_ssh::SharedConn {
    shared_with(e, Arc::new(MemKnown::default())).await
}

async fn shared_with(e: &Env, known: Arc<MemKnown>) -> tern_ssh::SharedConn {
    // Trust the host key the way a first terminal connect would.
    let (h, mut rx) = spawn(params(e, &e.password), known.clone());
    loop {
        match next(&mut rx).await {
            SessionEvent::HostKeyPrompt { .. } => {
                h.send(SessionCommand::HostKeyDecision(true)).await;
            }
            SessionEvent::Connected => break,
            SessionEvent::Error(err) => panic!("{err}"),
            _ => {}
        }
    }
    h.send(SessionCommand::Close).await;
    ConnectionPool::new()
        .get_or_connect("h", &params(e, &e.password), known)
        .await
        .expect("headless connect")
}

fn spec(kind: TunnelKind, bind_port: u16, target: Option<(&str, u16)>) -> TunnelSpec {
    TunnelSpec {
        kind,
        bind_addr: "127.0.0.1".into(),
        bind_port,
        target_host: target.map(|t| t.0.into()),
        target_port: target.map(|t| t.1),
    }
}

async fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().port()
}

async fn read_banner(mut s: TcpStream) -> String {
    let mut buf = [0u8; 8];
    tokio::time::timeout(Duration::from_secs(10), s.read_exact(&mut buf))
        .await
        .expect("banner timeout")
        .expect("banner read");
    String::from_utf8_lossy(&buf).into_owned()
}

#[tokio::test]
async fn local_forward_reaches_a_target_through_the_server() {
    let Some(e) = env() else { return };
    let conn = shared(&e).await;
    let port = free_port().await;
    // The server's own sshd, as seen from the server.
    let t = tunnel::start(
        conn,
        &spec(TunnelKind::Local, port, Some(("127.0.0.1", 2222))),
    )
    .await
    .unwrap();
    // Concurrent connections.
    let (a, b) = tokio::join!(
        async { read_banner(TcpStream::connect(("127.0.0.1", port)).await.unwrap()).await },
        async { read_banner(TcpStream::connect(("127.0.0.1", port)).await.unwrap()).await },
    );
    assert_eq!((a.as_str(), b.as_str()), ("SSH-2.0-", "SSH-2.0-"));
    assert!(t.stats.snapshot().1 >= 2);
    t.stop().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
}

#[tokio::test]
async fn bind_conflict_is_reported_at_start() {
    let Some(e) = env() else { return };
    let conn = shared(&e).await;
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let err = tunnel::start(conn, &spec(TunnelKind::Dynamic, port, None))
        .await
        .err()
        .expect("must fail");
    assert!(err.to_string().contains("already in use"), "{err}");
}

#[tokio::test]
async fn socks5_connect_with_remote_dns() {
    let Some(e) = env() else { return };
    let conn = shared(&e).await;
    let port = free_port().await;
    let t = tunnel::start(conn, &spec(TunnelKind::Dynamic, port, None))
        .await
        .unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(&[5, 1, 0]).await.unwrap();
    let mut r = [0u8; 2];
    s.read_exact(&mut r).await.unwrap();
    assert_eq!(r, [5, 0]);
    // "localhost" must be resolved by the server.
    let mut req = vec![5, 1, 0, 3, 9];
    req.extend_from_slice(b"localhost");
    req.extend_from_slice(&2222u16.to_be_bytes());
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0, "SOCKS reply code");
    assert_eq!(read_banner(s).await, "SSH-2.0-");
    t.stop().await;
}

#[tokio::test]
async fn remote_forward_reaches_a_local_target() {
    let Some(e) = env() else { return };
    let conn = shared(&e).await;
    // A local service that greets.
    let svc = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let svc_port = svc.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = svc.accept().await.unwrap();
            let _ = s.write_all(b"hello-r!").await;
        }
    });
    let remote_port = free_port().await;
    let r = match tunnel::start(
        conn.clone(),
        &spec(
            TunnelKind::Remote,
            remote_port,
            Some(("127.0.0.1", svc_port)),
        ),
    )
    .await
    {
        Ok(r) => r,
        Err(err) => {
            eprintln!("server does not allow remote forwarding, skipping: {err}");
            return;
        }
    };
    // Reach the server-side listener through a local forward on the same connection.
    let local = free_port().await;
    let l = tunnel::start(
        conn,
        &spec(TunnelKind::Local, local, Some(("127.0.0.1", remote_port))),
    )
    .await
    .unwrap();
    let got = read_banner(TcpStream::connect(("127.0.0.1", local)).await.unwrap()).await;
    assert_eq!(got, "hello-r!");
    l.stop().await;
    r.stop().await;
}

#[tokio::test]
async fn manager_starts_reports_and_stops_a_tunnel() {
    use tern_core::TunnelId;
    use tern_ssh::manager::{self, TunnelCommand, TunnelEvent};
    let Some(e) = env() else { return };
    let known = Arc::new(MemKnown::default());
    // Unknown host key + no UI: the tunnel must fail with a helpful error, not hang.
    let (mgr, mut rx) = manager::spawn(ConnectionPool::new(), known.clone(), false);
    let id = TunnelId::new();
    let port = free_port().await;
    let start = |mgr: &manager::TunnelManagerHandle| {
        mgr.send(TunnelCommand::Start {
            id,
            pool_key: "h".into(),
            params: Box::new(params(&e, &e.password)),
            spec: spec(TunnelKind::Local, port, Some(("127.0.0.1", 2222))),
        })
    };
    start(&mgr);
    let mut saw_error = false;
    while let Some(ev) = rx.recv().await {
        if let TunnelEvent::Error {
            message, retrying, ..
        } = ev
        {
            assert!(!retrying && message.contains("connect once"), "{message}");
            saw_error = true;
            break;
        }
    }
    assert!(saw_error);

    // After trusting the key (as a terminal connect would), it comes up and stops cleanly.
    drop(shared_with(&e, known.clone()).await);
    start(&mgr);
    loop {
        match tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(TunnelEvent::Active(_))) => break,
            Ok(Some(TunnelEvent::Error { message, .. })) => panic!("{message}"),
            Ok(Some(_)) => {}
            _ => panic!("no Active event"),
        }
    }
    assert_eq!(
        read_banner(TcpStream::connect(("127.0.0.1", port)).await.unwrap()).await,
        "SSH-2.0-"
    );
    mgr.send(TunnelCommand::StopAll);
    loop {
        match tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(TunnelEvent::Stopped(_))) => break,
            Ok(Some(_)) => {}
            _ => panic!("no Stopped event"),
        }
    }
}

/// Jump host: `TERN_SSH_TEST_*` is the bastion (needs `AllowTcpForwarding yes`);
/// `TERN_SSH_JUMP_TARGET=host:port` is the private target as seen *from the bastion*
/// (same user/password), e.g. a second container on a private docker network.
#[tokio::test]
async fn terminal_through_a_jump_host_verifies_every_hop() {
    let (Some(e), Ok(target)) = (env(), std::env::var("TERN_SSH_JUMP_TARGET")) else {
        return;
    };
    let (thost, tport) = target.rsplit_once(':').unwrap();
    let mut p = params(&e, &e.password);
    p.host = thost.into();
    p.port = tport.parse().unwrap();
    p.jump = Some(Box::new(params(&e, &e.password)));
    let known = Arc::new(MemKnown::default());
    let (h, mut rx) = spawn(p, known.clone());
    let mut out = String::new();
    let mut prompts = Vec::new();
    loop {
        match next(&mut rx).await {
            SessionEvent::HostKeyPrompt { host, port, .. } => {
                prompts.push((host, port));
                h.send(SessionCommand::HostKeyDecision(true)).await;
            }
            SessionEvent::Connected => {
                h.send(SessionCommand::Write("echo hop-$((40+2))\nexit\n".into()))
                    .await;
            }
            SessionEvent::Output(b) => out.push_str(&String::from_utf8_lossy(&b)),
            SessionEvent::Exited(_) | SessionEvent::Closed => break,
            SessionEvent::Error(err) => panic!("{err}"),
            _ => {}
        }
    }
    assert!(out.contains("hop-42"), "output was: {out:?}");
    assert_eq!(
        prompts.len(),
        2,
        "bastion and target are both verified: {prompts:?}"
    );
    assert_eq!(prompts[0].0, e.host);
    assert_eq!(prompts[1].0, thost);
}
