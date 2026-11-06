//! tern-io: backup bundle, plaintext export, ssh_config import/export, key scanning.

pub mod bundle;

pub use bundle::{Bundle, Payload, decrypt, encrypt};

/// Errors produced by `tern-io`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a tern backup file")]
    NotABundle,
    #[error("unsupported backup version {0}")]
    UnsupportedVersion(u32),
    #[error("wrong password, or the backup is corrupted")]
    WrongPasswordOrCorrupt,
    #[error("invalid backup: {0}")]
    Invalid(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
