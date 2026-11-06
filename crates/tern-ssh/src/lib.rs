//! tern-ssh: SSH client wrapper, session actor and key helpers.

pub mod keys;
pub mod known_hosts;
pub mod session;

pub use known_hosts::{HostKeyInfo, HostKeyVerdict, KnownHostsStore};
pub use session::{
    AuthPromptItem, ConnectParams, KeyAuth, SessionCommand, SessionEvent, SessionHandle, spawn,
};

/// Errors produced by `tern-ssh`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("could not resolve the host name")]
    Dns,
    #[error("connection refused")]
    Refused,
    #[error("connection timed out")]
    Timeout,
    #[error("network error: {0}")]
    Network(String),
    #[error("the host key was rejected")]
    HostKeyRejected,
    #[error("authentication failed")]
    AuthFailed,
    #[error("the private key could not be used: {0}")]
    Key(String),
    #[error("the connection was lost")]
    Disconnected,
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<russh::Error> for Error {
    fn from(e: russh::Error) -> Self {
        match e {
            russh::Error::IO(io) => Error::from(io),
            russh::Error::ConnectionTimeout => Error::Timeout,
            russh::Error::Disconnect
            | russh::Error::HUP
            | russh::Error::KeepaliveTimeout
            | russh::Error::InactivityTimeout
            | russh::Error::SendError
            | russh::Error::RecvError => Error::Disconnected,
            other => Error::Other(other.to_string()),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        match e.kind() {
            ConnectionRefused => Error::Refused,
            TimedOut => Error::Timeout,
            ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof => Error::Disconnected,
            _ => {
                let msg = e.to_string();
                let lower = msg.to_lowercase();
                if lower.contains("lookup")
                    || lower.contains("resolve")
                    || lower.contains("name or service")
                {
                    Error::Dns
                } else {
                    Error::Network(msg)
                }
            }
        }
    }
}
