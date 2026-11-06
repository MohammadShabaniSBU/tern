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
use tern_ssh::known_hosts::BoxFuture;
use tern_ssh::{
    ConnectParams, ConnectionPool, Error, HostKeyInfo, KnownHostsStore, SessionCommand,
    SessionEvent, spawn, spawn_pooled,
};
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
