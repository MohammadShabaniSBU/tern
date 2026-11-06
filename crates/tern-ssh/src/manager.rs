//! Tunnel manager actor (S6-T06): starts, stops and supervises tunnels independently of
//! terminal tabs, reconnecting with backoff when the shared connection drops.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tern_core::TunnelId;
use tokio::sync::{mpsc, oneshot};

use crate::Error;
use crate::known_hosts::KnownHostsStore;
use crate::pool::ConnectionPool;
use crate::session::ConnectParams;
use crate::tunnel::{self, TunnelSpec};

pub enum TunnelCommand {
    Start {
        id: TunnelId,
        /// Connection pool key (the host id), so tunnels and terminals share a connection.
        pool_key: String,
        params: Box<ConnectParams>,
        spec: TunnelSpec,
    },
    Stop(TunnelId),
    StopAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelEvent {
    Starting(TunnelId),
    Active(TunnelId),
    /// Failed or lost; `retrying` when the manager will try again by itself.
    Error {
        id: TunnelId,
        message: String,
        retrying: bool,
    },
    Stopped(TunnelId),
    Stats {
        id: TunnelId,
        conns: u64,
        bytes: u64,
    },
}

#[derive(Clone)]
pub struct TunnelManagerHandle {
    tx: mpsc::UnboundedSender<TunnelCommand>,
}

impl TunnelManagerHandle {
    pub fn send(&self, cmd: TunnelCommand) {
        let _ = self.tx.send(cmd);
    }
}

/// Backoff before retry number `attempt` (0-based): 1 s doubling up to 30 s.
pub fn backoff(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << attempt.min(5)).min(30))
}

/// Connection-level failures are worth retrying; configuration/auth problems are not.
fn retryable(e: &Error) -> bool {
    matches!(
        e,
        Error::Dns | Error::Refused | Error::Timeout | Error::Network(_) | Error::Disconnected
    )
}

/// Start the manager. Must be called inside a tokio runtime.
pub fn spawn(
    pool: ConnectionPool,
    known: Arc<dyn KnownHostsStore>,
    reconnect: bool,
) -> (TunnelManagerHandle, mpsc::Receiver<TunnelEvent>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (ev_tx, ev_rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut running: HashMap<TunnelId, oneshot::Sender<()>> = HashMap::new();
        while let Some(cmd) = rx.recv().await {
            running.retain(|_, stop| !stop.is_closed());
            match cmd {
                TunnelCommand::Start {
                    id,
                    pool_key,
                    params,
                    spec,
                } => {
                    if running.contains_key(&id) {
                        continue;
                    }
                    let (stop_tx, stop_rx) = oneshot::channel();
                    running.insert(id, stop_tx);
                    tokio::spawn(supervise(
                        id,
                        pool_key,
                        *params,
                        spec,
                        pool.clone(),
                        known.clone(),
                        reconnect,
                        ev_tx.clone(),
                        stop_rx,
                    ));
                }
                TunnelCommand::Stop(id) => {
                    if let Some(stop) = running.remove(&id) {
                        let _ = stop.send(());
                    }
                }
                TunnelCommand::StopAll => {
                    for (_, stop) in running.drain() {
                        let _ = stop.send(());
                    }
                }
            }
        }
    });
    (TunnelManagerHandle { tx }, ev_rx)
}

#[allow(clippy::too_many_arguments)]
async fn supervise(
    id: TunnelId,
    pool_key: String,
    params: ConnectParams,
    spec: TunnelSpec,
    pool: ConnectionPool,
    known: Arc<dyn KnownHostsStore>,
    reconnect: bool,
    events: mpsc::Sender<TunnelEvent>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut attempt = 0u32;
    'outer: loop {
        let _ = events.send(TunnelEvent::Starting(id)).await;
        let started = tokio::select! {
            _ = &mut stop => break 'outer,
            r = async {
                let conn = pool.get_or_connect(&pool_key, &params, known.clone()).await?;
                tunnel::start(conn, &spec).await
            } => r,
        };
        let err = match started {
            Ok(running) => {
                attempt = 0;
                let _ = events.send(TunnelEvent::Active(id)).await;
                let mut last = (0, 0);
                let mut tick = tokio::time::interval(Duration::from_millis(500));
                loop {
                    tokio::select! {
                        _ = &mut stop => {
                            running.stop().await;
                            break 'outer;
                        }
                        _ = tick.tick() => {
                            let (active, _total, bytes) = running.stats.snapshot();
                            if (active, bytes) != last {
                                last = (active, bytes);
                                let _ = events.send(TunnelEvent::Stats { id, conns: active, bytes }).await;
                            }
                            if running.is_broken() {
                                running.stop().await;
                                break;
                            }
                        }
                    }
                }
                Error::Disconnected
            }
            Err(e) => e,
        };
        let again = reconnect && retryable(&err);
        let _ = events
            .send(TunnelEvent::Error {
                id,
                message: err.to_string(),
                retrying: again,
            })
            .await;
        if !again {
            return;
        }
        let wait = backoff(attempt);
        attempt += 1;
        tokio::select! {
            _ = &mut stop => break 'outer,
            () = tokio::time::sleep(wait) => {}
        }
    }
    let _ = events.send(TunnelEvent::Stopped(id)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let secs: Vec<_> = (0..8).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[test]
    fn only_connection_errors_retry() {
        assert!(retryable(&Error::Disconnected));
        assert!(retryable(&Error::Timeout));
        assert!(!retryable(&Error::AuthFailed));
        assert!(!retryable(&Error::Other("x is already in use".into())));
    }
}
