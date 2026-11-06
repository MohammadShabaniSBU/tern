//! Port forwarding over a shared connection (S6-T03/T04/T05): local (-L), remote (-R) and
//! dynamic (SOCKS5, -D). Dropping a [`RunningTunnel`] aborts its listener and all in-flight
//! forwarded connections.

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tern_core::TunnelKind;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

use crate::Error;
use crate::session::SharedConn;
use crate::socks5;

/// What to forward. Mirrors `tern_core::Tunnel` without the persistence fields.
#[derive(Debug, Clone)]
pub struct TunnelSpec {
    pub kind: TunnelKind,
    pub bind_addr: String,
    pub bind_port: u16,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
}

/// Live counters shared with the UI.
#[derive(Debug, Default)]
pub struct Stats {
    pub active: AtomicU64,
    pub total: AtomicU64,
    pub bytes: AtomicU64,
}

impl Stats {
    pub fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.active.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }
}

/// Where remote-forwarded connections go, keyed by the server-side port.
#[derive(Clone, Default)]
pub struct RemoteForwards(Arc<Mutex<HashMap<u32, RemoteTarget>>>);

#[derive(Clone)]
pub struct RemoteTarget {
    host: String,
    port: u16,
    stats: Arc<Stats>,
}

pub(crate) fn remote_target(forwards: &RemoteForwards, port: u32) -> Option<RemoteTarget> {
    forwards
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&port)
        .cloned()
}

/// Counts bytes in both directions on the local side of a forwarded connection.
struct Counted<T> {
    inner: T,
    stats: Arc<Stats>,
}

impl<T: AsyncRead + Unpin> AsyncRead for Counted<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        let n = buf.filled().len() - before;
        self.stats.bytes.fetch_add(n as u64, Ordering::Relaxed);
        r
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Counted<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let r = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &r {
            self.stats.bytes.fetch_add(*n as u64, Ordering::Relaxed);
        }
        r
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

struct ActiveGuard(Arc<Stats>);

impl ActiveGuard {
    fn new(stats: &Arc<Stats>) -> Self {
        stats.active.fetch_add(1, Ordering::Relaxed);
        stats.total.fetch_add(1, Ordering::Relaxed);
        Self(stats.clone())
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct RunningTunnel {
    pub stats: Arc<Stats>,
    task: Option<JoinHandle<()>>,
    conn: SharedConn,
    /// `(server bind address, port)` to cancel on stop (remote tunnels).
    remote: Option<(String, u32)>,
}

impl RunningTunnel {
    /// The underlying SSH connection dropped; the tunnel is dead and should be restarted.
    pub fn is_broken(&self) -> bool {
        self.conn.is_closed() || self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub async fn stop(mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
        if let Some((addr, port)) = self.remote.take() {
            if let Ok(h) = self.conn.handle() {
                let _ = h.cancel_tcpip_forward(addr, port).await;
            }
            self.conn
                .forwards
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&port);
        }
    }
}

impl Drop for RunningTunnel {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

fn bind_error(e: &io::Error, addr: &str, port: u16) -> Error {
    match e.kind() {
        io::ErrorKind::AddrInUse => Error::Other(format!("{addr}:{port} is already in use")),
        io::ErrorKind::PermissionDenied => {
            Error::Other(format!("not allowed to listen on {addr}:{port}"))
        }
        _ => Error::Other(format!("cannot listen on {addr}:{port}: {e}")),
    }
}

/// Start forwarding over `conn`. Bind/forward errors are returned here, not later.
pub async fn start(conn: SharedConn, spec: &TunnelSpec) -> Result<RunningTunnel, Error> {
    let stats = Arc::new(Stats::default());
    let missing = || Error::Other("tunnel target is missing".into());
    match spec.kind {
        TunnelKind::Local | TunnelKind::Dynamic => {
            let listener = TcpListener::bind((spec.bind_addr.as_str(), spec.bind_port))
                .await
                .map_err(|e| bind_error(&e, &spec.bind_addr, spec.bind_port))?;
            let target = match spec.kind {
                TunnelKind::Local => Some((
                    spec.target_host.clone().ok_or_else(missing)?,
                    spec.target_port.ok_or_else(missing)?,
                )),
                _ => None,
            };
            let task = tokio::spawn(accept_loop(listener, conn.clone(), target, stats.clone()));
            Ok(RunningTunnel {
                stats,
                task: Some(task),
                conn,
                remote: None,
            })
        }
        TunnelKind::Remote => {
            let target = RemoteTarget {
                host: spec.target_host.clone().ok_or_else(missing)?,
                port: spec.target_port.ok_or_else(missing)?,
                stats: stats.clone(),
            };
            let granted = conn
                .handle()?
                .tcpip_forward(spec.bind_addr.clone(), u32::from(spec.bind_port))
                .await
                .map_err(|e| match e {
                    russh::Error::RequestDenied => Error::Other(
                        "the server refused the remote forward (check AllowTcpForwarding / GatewayPorts)"
                            .into(),
                    ),
                    other => Error::from(other),
                })?;
            // Port 0 asks the server to pick one.
            let port = if spec.bind_port == 0 {
                granted
            } else {
                u32::from(spec.bind_port)
            };
            conn.forwards
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(port, target);
            Ok(RunningTunnel {
                stats,
                task: None,
                conn,
                remote: Some((spec.bind_addr.clone(), port)),
            })
        }
    }
}

async fn accept_loop(
    listener: TcpListener,
    conn: SharedConn,
    target: Option<(String, u16)>,
    stats: Arc<Stats>,
) {
    // Dropping the set (when this task is aborted) aborts every in-flight connection.
    let mut set = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((sock, peer)) = accepted else {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                };
                let (conn, stats, target) = (conn.clone(), stats.clone(), target.clone());
                set.spawn(async move {
                    let _guard = ActiveGuard::new(&stats);
                    let _ = match target {
                        Some((host, port)) => local_conn(&conn, sock, peer, &host, port, &stats).await,
                        None => socks_conn(&conn, sock, peer, &stats).await,
                    };
                });
            }
            Some(_) = set.join_next(), if !set.is_empty() => {}
        }
    }
}

async fn open_and_copy(
    conn: &SharedConn,
    sock: TcpStream,
    peer: std::net::SocketAddr,
    host: &str,
    port: u16,
    stats: &Arc<Stats>,
) -> Result<(), Error> {
    let channel = conn
        .handle()?
        .channel_open_direct_tcpip(
            host,
            u32::from(port),
            peer.ip().to_string(),
            u32::from(peer.port()),
        )
        .await?;
    let mut remote = channel.into_stream();
    let mut local = Counted {
        inner: sock,
        stats: stats.clone(),
    };
    let _ = copy_bidirectional(&mut local, &mut remote).await;
    Ok(())
}

async fn local_conn(
    conn: &SharedConn,
    sock: TcpStream,
    peer: std::net::SocketAddr,
    host: &str,
    port: u16,
    stats: &Arc<Stats>,
) -> Result<(), Error> {
    open_and_copy(conn, sock, peer, host, port, stats).await
}

async fn socks_conn(
    conn: &SharedConn,
    mut sock: TcpStream,
    peer: std::net::SocketAddr,
    stats: &Arc<Stats>,
) -> Result<(), Error> {
    let (host, port) = match socks5::handshake(&mut sock).await {
        Ok(t) => t,
        Err(socks5::Socks5Error::Reply(code)) => {
            let _ = socks5::reply(&mut sock, code).await;
            return Ok(());
        }
        Err(socks5::Socks5Error::Protocol) => return Ok(()),
    };
    // Open the channel first so the client only sees success when the target is reachable.
    let channel = match conn
        .handle()?
        .channel_open_direct_tcpip(
            host.as_str(),
            u32::from(port),
            peer.ip().to_string(),
            u32::from(peer.port()),
        )
        .await
    {
        Ok(c) => c,
        Err(russh::Error::ChannelOpenFailure(_)) => {
            let _ = socks5::reply(&mut sock, socks5::REP_CONNECTION_REFUSED).await;
            return Ok(());
        }
        Err(_) => {
            let _ = socks5::reply(&mut sock, socks5::REP_GENERAL_FAILURE).await;
            return Ok(());
        }
    };
    socks5::reply(&mut sock, socks5::REP_SUCCEEDED).await?;
    let mut remote = channel.into_stream();
    let mut local = Counted {
        inner: sock,
        stats: stats.clone(),
    };
    let _ = copy_bidirectional(&mut local, &mut remote).await;
    Ok(())
}

/// A connection the server opened for a remote forward: connect to the local target and copy.
pub(crate) async fn serve_remote(
    channel: russh::Channel<russh::client::Msg>,
    target: RemoteTarget,
) {
    let _guard = ActiveGuard::new(&target.stats);
    let Ok(sock) = TcpStream::connect((target.host.as_str(), target.port)).await else {
        return;
    };
    let mut remote = channel.into_stream();
    let mut local = Counted {
        inner: sock,
        stats: target.stats.clone(),
    };
    let _ = copy_bidirectional(&mut local, &mut remote).await;
}
