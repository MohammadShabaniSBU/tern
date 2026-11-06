//! tern-store: stub (filled in by a later sprint).

/// Errors produced by `tern-store`. Variants are added as functionality lands.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
