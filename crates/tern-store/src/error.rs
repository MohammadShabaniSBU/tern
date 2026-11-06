/// Errors produced by `tern-store`. Messages never contain secret values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Vault(#[from] tern_vault::Error),
    #[error(transparent)]
    Validation(#[from] tern_core::ValidationError),
    /// A secret operation was attempted while the vault is locked.
    #[error("vault is locked")]
    Locked,
    #[error("no vault has been created yet")]
    NoVault,
    #[error("a vault already exists")]
    VaultExists,
    /// The store worker thread is gone (it panicked or the store was shut down).
    #[error("store worker has stopped")]
    Closed,
    #[error("{entity} not found")]
    NotFound { entity: &'static str },
    /// A stored value could not be interpreted (never includes the value itself).
    #[error("corrupt data: {0}")]
    Corrupt(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
