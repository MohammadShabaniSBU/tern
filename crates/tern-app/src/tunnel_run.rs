//! Runtime side of tunnels (S6-T06): the manager handle, its event subscription and the
//! per-tunnel status shown in the list.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use iced::futures::{Stream, stream};
use tern_ssh::manager::{TunnelEvent, TunnelManagerHandle};
use tokio::sync::mpsc::Receiver;

use crate::app::Message;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunState {
    Starting,
    Active,
    /// Failed; `retrying` when the manager is backing off before another attempt.
    Error {
        message: String,
        retrying: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunStatus {
    pub state: TunState,
    pub conns: u64,
    pub bytes: u64,
}

impl TunStatus {
    pub fn new(state: TunState) -> Self {
        Self {
            state,
            conns: 0,
            bytes: 0,
        }
    }

    /// Running or trying to run: toggling stops it.
    pub fn is_running(&self) -> bool {
        match &self.state {
            TunState::Starting | TunState::Active => true,
            TunState::Error { retrying, .. } => *retrying,
        }
    }

    pub fn label(&self) -> String {
        match &self.state {
            TunState::Starting => "starting".into(),
            TunState::Active => format!("active  {}  {} conn", human_bytes(self.bytes), self.conns),
            TunState::Error { message, retrying } => {
                if *retrying {
                    format!("error (retrying): {message}")
                } else {
                    format!("error: {message}")
                }
            }
        }
    }
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// The running manager: command handle plus the one-shot event receiver.
#[derive(Clone)]
pub struct TunnelMgr {
    pub handle: TunnelManagerHandle,
    rx: Arc<Mutex<Option<Receiver<TunnelEvent>>>>,
}

impl TunnelMgr {
    pub fn new(handle: TunnelManagerHandle, rx: Receiver<TunnelEvent>) -> Self {
        Self {
            handle,
            rx: Arc::new(Mutex::new(Some(rx))),
        }
    }
}

impl std::fmt::Debug for TunnelMgr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelMgr")
    }
}

impl Hash for TunnelMgr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "tunnel-manager".hash(state);
    }
}

pub fn subscription(mgr: &TunnelMgr) -> iced::Subscription<Message> {
    iced::Subscription::run_with(mgr.clone(), events)
}

fn events(mgr: &TunnelMgr) -> impl Stream<Item = Message> + use<> {
    let rx = mgr.rx.lock().ok().and_then(|mut g| g.take());
    stream::unfold(rx, |rx| async move {
        let mut rx = rx?;
        let event = rx.recv().await?;
        Some((Message::Tunnel(event), Some(rx)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_human_readable() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn only_non_retrying_errors_are_not_running() {
        let err = |retrying| {
            TunStatus::new(TunState::Error {
                message: "x".into(),
                retrying,
            })
        };
        assert!(err(true).is_running());
        assert!(!err(false).is_running());
        assert!(TunStatus::new(TunState::Starting).is_running());
    }
}
