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
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
