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
    /// A key cannot be deleted while hosts still authenticate with it.
    #[error("this key is used by {0} host(s); change their authentication first")]
    KeyInUse(usize),
    #[error("{entity} not found")]
    NotFound { entity: &'static str },
    /// A stored value could not be interpreted (never includes the value itself).
    #[error("corrupt data: {0}")]
    Corrupt(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// A sentence for the user (no jargon, no secrets) about what to do next.
    pub fn user_message(&self) -> String {
        use rusqlite::ErrorCode;
        match self {
            Error::Sqlite(rusqlite::Error::SqliteFailure(e, _)) => match e.code {
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => {
                    "The database is locked by another process. Is tern already running?".into()
                }
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => {
                    "The database file is damaged or is not a tern database. Restore it from a backup (.tern) or move it away to start fresh.".into()
                }
                ErrorCode::ReadOnly => "The database file is read-only.".into(),
                ErrorCode::DiskFull => "The disk is full.".into(),
                _ => self.to_string(),
            },
            Error::Corrupt(why) => format!("The database failed its integrity check: {why}"),
            Error::Closed => "tern's storage thread stopped unexpectedly; restart tern.".into(),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite(code: rusqlite::ffi::ErrorCode) -> Error {
        let ffi = rusqlite::ffi::Error {
            code,
            extended_code: 0,
        };
        Error::Sqlite(rusqlite::Error::SqliteFailure(ffi, None))
    }

    #[test]
    fn common_failures_get_actionable_messages() {
        use rusqlite::ffi::ErrorCode;
        assert!(
            sqlite(ErrorCode::DatabaseBusy)
                .user_message()
                .contains("already running")
        );
        assert!(
            sqlite(ErrorCode::DatabaseCorrupt)
                .user_message()
                .contains("backup")
        );
        assert!(
            sqlite(ErrorCode::NotADatabase)
                .user_message()
                .contains("backup")
        );
        assert!(Error::Locked.user_message().contains("locked"));
    }
}
