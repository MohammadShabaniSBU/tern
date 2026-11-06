//! Repositories. Each borrows a `&Connection` and maps one table to `tern-core` types.
//!
//! Rules:
//! - Listing and plain `get` never decrypt anything.
//! - Secret columns are written/read only through explicit methods that take a [`Vault`].
//! - AAD for every secret is `"<table>:<column>:<row id>"`, binding ciphertext to its row.
//! - `created_at`/`updated_at` are set by SQLite (`strftime`), not by callers.

mod groups;
mod hosts;
mod identities;
mod keys;
mod known_hosts;
mod settings;
mod tunnels;
mod vault_meta;

use std::fmt::Display;
use std::str::FromStr;

use secrecy::{ExposeSecret, SecretString};
use tern_vault::Vault;

use crate::{Error, Result};

pub use groups::GroupRepo;
pub use hosts::HostRepo;
pub use identities::IdentityRepo;
pub use keys::KeyRepo;
pub use known_hosts::KnownHostRepo;
pub use settings::SettingsRepo;
pub use tunnels::TunnelRepo;
pub use vault_meta::VaultMetaRepo;

/// SQL expression for "now" as an RFC 3339 UTC timestamp.
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

pub(crate) fn aad(table: &str, column: &str, id: &impl Display) -> Vec<u8> {
    format!("{table}:{column}:{id}").into_bytes()
}

pub(crate) fn seal_secret(
    vault: &Vault,
    secret: &SecretString,
    table: &str,
    column: &str,
    id: &impl Display,
) -> Result<Vec<u8>> {
    Ok(vault.seal(secret.expose_secret().as_bytes(), &aad(table, column, id))?)
}

pub(crate) fn open_secret(
    vault: &Vault,
    blob: &[u8],
    table: &str,
    column: &str,
    id: &impl Display,
) -> Result<SecretString> {
    let plain = vault.open(blob, &aad(table, column, id))?;
    let text = std::str::from_utf8(plain.expose_secret())
        .map_err(|_| Error::Corrupt(format!("{table}.{column} is not valid UTF-8")))?;
    Ok(SecretString::from(text.to_owned()))
}

/// Parse a TEXT column (ids) into a typed value, as a rusqlite conversion error on failure.
pub(crate) fn parse_col<T>(idx: usize, text: &str) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    text.parse().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub(crate) fn parse_opt_col<T>(idx: usize, text: Option<String>) -> rusqlite::Result<Option<T>>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    text.map(|t| parse_col(idx, &t)).transpose()
}

/// Error for rows that matched nothing.
pub(crate) fn require_changed(changed: usize, entity: &'static str) -> Result<()> {
    if changed == 0 {
        Err(Error::NotFound { entity })
    } else {
        Ok(())
    }
}
