use rusqlite::{Connection, OptionalExtension, Row, params};
use secrecy::SecretString;
use tern_core::{Identity, IdentityId};
use tern_vault::Vault;

use super::{NOW, open_secret, parse_col, parse_opt_col, require_changed, seal_secret};
use crate::Result;

pub struct IdentityRepo<'c> {
    conn: &'c Connection,
}

const COLS: &str = "id, label, username, ssh_key_id";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Identity> {
    Ok(Identity {
        id: parse_col(0, &row.get::<_, String>(0)?)?,
        label: row.get(1)?,
        username: row.get(2)?,
        ssh_key_id: parse_opt_col(3, row.get(3)?)?,
    })
}

impl<'c> IdentityRepo<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    /// Listing never decrypts.
    pub fn list(&self) -> Result<Vec<Identity>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM identities ORDER BY label COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, id: IdentityId) -> Result<Option<Identity>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM identities WHERE id = ?1"),
                [id.to_string()],
                from_row,
            )
            .optional()?)
    }

    /// Insert an identity, optionally with a password (sealed with `vault`).
    pub fn insert(
        &self,
        identity: &Identity,
        password: Option<&SecretString>,
        vault: &Vault,
    ) -> Result<()> {
        identity.validate()?;
        let password_enc = password
            .map(|p| seal_secret(vault, p, "identities", "password_enc", &identity.id))
            .transpose()?;
        self.conn.execute(
            &format!(
                "INSERT INTO identities (id, label, username, ssh_key_id, password_enc, \
                 created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, {NOW}, {NOW})"
            ),
            params![
                identity.id.to_string(),
                identity.label,
                identity.username,
                identity.ssh_key_id.map(|k| k.to_string()),
                password_enc,
            ],
        )?;
        Ok(())
    }

    /// Update non-secret fields. The stored password is left untouched; use [`Self::set_password`].
    pub fn update(&self, identity: &Identity) -> Result<()> {
        identity.validate()?;
        let n = self.conn.execute(
            &format!(
                "UPDATE identities SET label = ?2, username = ?3, ssh_key_id = ?4, \
                 updated_at = {NOW} WHERE id = ?1"
            ),
            params![
                identity.id.to_string(),
                identity.label,
                identity.username,
                identity.ssh_key_id.map(|k| k.to_string()),
            ],
        )?;
        require_changed(n, "identity")
    }

    pub fn delete(&self, id: IdentityId) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM identities WHERE id = ?1", [id.to_string()])?
            > 0)
    }

    /// Set or clear (`None`) the stored password.
    pub fn set_password(
        &self,
        id: IdentityId,
        password: Option<&SecretString>,
        vault: &Vault,
    ) -> Result<()> {
        let enc = password
            .map(|p| seal_secret(vault, p, "identities", "password_enc", &id))
            .transpose()?;
        let n = self.conn.execute(
            &format!("UPDATE identities SET password_enc = ?2, updated_at = {NOW} WHERE id = ?1"),
            params![id.to_string(), enc],
        )?;
        require_changed(n, "identity")
    }

    pub fn has_password(&self, id: IdentityId) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT password_enc IS NOT NULL FROM identities WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    /// Explicitly decrypt the stored password.
    pub fn get_password(&self, id: IdentityId, vault: &Vault) -> Result<Option<SecretString>> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT password_enc FROM identities WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        blob.map(|b| open_secret(vault, &b, "identities", "password_enc", &id))
            .transpose()
    }
}
