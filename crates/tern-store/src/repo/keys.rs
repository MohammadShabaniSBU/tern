use rusqlite::{Connection, OptionalExtension, Row, params};
use secrecy::SecretString;
use tern_core::{KeyId, SshKeyMeta};
use tern_vault::Vault;

use super::{NOW, open_secret, parse_col, require_changed, seal_secret};
use crate::Result;

pub struct KeyRepo<'c> {
    conn: &'c Connection,
}

const COLS: &str = "id, label, key_type, public_key, fingerprint";

fn from_row(row: &Row<'_>) -> rusqlite::Result<SshKeyMeta> {
    Ok(SshKeyMeta {
        id: parse_col(0, &row.get::<_, String>(0)?)?,
        label: row.get(1)?,
        key_type: row.get(2)?,
        public_key: row.get(3)?,
        fingerprint: row.get(4)?,
    })
}

impl<'c> KeyRepo<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    /// Public metadata only; never decrypts.
    pub fn list(&self) -> Result<Vec<SshKeyMeta>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM ssh_keys ORDER BY label COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, id: KeyId) -> Result<Option<SshKeyMeta>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM ssh_keys WHERE id = ?1"),
                [id.to_string()],
                from_row,
            )
            .optional()?)
    }

    /// Insert a key; the private key (OpenSSH PEM) and optional passphrase are sealed.
    pub fn insert(
        &self,
        meta: &SshKeyMeta,
        private_key: &SecretString,
        passphrase: Option<&SecretString>,
        vault: &Vault,
    ) -> Result<()> {
        meta.validate()?;
        let private_enc = seal_secret(vault, private_key, "ssh_keys", "private_key_enc", &meta.id)?;
        let passphrase_enc = passphrase
            .map(|p| seal_secret(vault, p, "ssh_keys", "passphrase_enc", &meta.id))
            .transpose()?;
        self.conn.execute(
            &format!(
                "INSERT INTO ssh_keys (id, label, key_type, public_key, fingerprint, \
                 private_key_enc, passphrase_enc, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, {NOW}, {NOW})"
            ),
            params![
                meta.id.to_string(),
                meta.label,
                meta.key_type,
                meta.public_key,
                meta.fingerprint,
                private_enc,
                passphrase_enc,
            ],
        )?;
        Ok(())
    }

    /// Update public metadata (the private key and passphrase are immutable once stored).
    pub fn update(&self, meta: &SshKeyMeta) -> Result<()> {
        meta.validate()?;
        let n = self.conn.execute(
            &format!(
                "UPDATE ssh_keys SET label = ?2, key_type = ?3, public_key = ?4, \
                 fingerprint = ?5, updated_at = {NOW} WHERE id = ?1"
            ),
            params![
                meta.id.to_string(),
                meta.label,
                meta.key_type,
                meta.public_key,
                meta.fingerprint
            ],
        )?;
        require_changed(n, "ssh key")
    }

    pub fn delete(&self, id: KeyId) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM ssh_keys WHERE id = ?1", [id.to_string()])?
            > 0)
    }

    /// `created_at` (UTC text) for every key.
    pub fn created_all(&self) -> Result<std::collections::HashMap<KeyId, String>> {
        let mut stmt = self.conn.prepare("SELECT id, created_at FROM ssh_keys")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                parse_col::<KeyId>(0, &r.get::<_, String>(0)?)?,
                r.get::<_, String>(1)?,
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_private_key(&self, id: KeyId, vault: &Vault) -> Result<Option<SecretString>> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT private_key_enc FROM ssh_keys WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        blob.map(|b| open_secret(vault, &b, "ssh_keys", "private_key_enc", &id))
            .transpose()
    }

    pub fn get_passphrase(&self, id: KeyId, vault: &Vault) -> Result<Option<SecretString>> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT passphrase_enc FROM ssh_keys WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        blob.map(|b| open_secret(vault, &b, "ssh_keys", "passphrase_enc", &id))
            .transpose()
    }
}
