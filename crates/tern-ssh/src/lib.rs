//! tern-ssh: SSH client wrapper and key helpers.

pub mod keys;

/// Errors produced by `tern-ssh`. Variants are added as functionality lands.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
