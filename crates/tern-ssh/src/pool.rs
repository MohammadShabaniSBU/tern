//! Shared authenticated connections (S6-T02): one SSH connection per host, used by terminal
//! sessions and tunnels. Entries are weak, so a connection closes when its last user drops it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use crate::Error;
use crate::known_hosts::KnownHostsStore;
use crate::session::{ConnectParams, SharedConn, connect_headless};

#[derive(Clone, Default)]
pub struct ConnectionPool {
    conns: Arc<Mutex<HashMap<String, Weak<crate::session::Conn>>>>,
    /// Serialises headless connects so concurrent tunnels to one host authenticate once.
    connecting: Arc<tokio::sync::Mutex<()>>,
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Weak<crate::session::Conn>>> {
        self.conns.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A live shared connection for `key`, if any.
    pub fn get(&self, key: &str) -> Option<SharedConn> {
        let mut map = self.lock();
        match map.get(key).and_then(Weak::upgrade) {
            Some(c) if !c.is_closed() => Some(c),
            Some(_) | None => {
                map.remove(key);
                None
            }
        }
    }

    pub(crate) fn insert(&self, key: &str, conn: &SharedConn) {
        self.lock().insert(key.to_owned(), Arc::downgrade(conn));
    }

    /// Reuse the connection for `key` or open one without a UI (tunnels). Fails when the host
    /// key is not yet trusted or authentication needs interactive input.
    pub async fn get_or_connect(
        &self,
        key: &str,
        params: &ConnectParams,
        known: Arc<dyn KnownHostsStore>,
    ) -> Result<SharedConn, Error> {
        if let Some(c) = self.get(key) {
            return Ok(c);
        }
        let _guard = self.connecting.lock().await;
        if let Some(c) = self.get(key) {
            return Ok(c);
        }
        let conn = connect_headless(params, known).await?;
        self.insert(key, &conn);
        Ok(conn)
    }

    /// Number of live pooled connections (for tests and diagnostics).
    pub fn live(&self) -> usize {
        self.lock()
            .values()
            .filter(|w| w.upgrade().is_some_and(|c| !c.is_closed()))
            .count()
    }
}

impl std::fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionPool")
            .field("live", &self.live())
            .finish()
    }
}
