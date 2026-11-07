use rusqlite::{Connection, OptionalExtension, Row, params};
use secrecy::SecretString;
use tern_core::{Host, HostAuth, HostId, KeyId};
use tern_vault::Vault;

use super::{NOW, open_secret, parse_col, parse_opt_col, require_changed, seal_secret};
use crate::{Error, Result};

pub struct HostRepo<'c> {
    conn: &'c Connection,
}

const COLS: &str = "id, label, hostname, port, username, auth_kind, ssh_key_id, group_id, \
                    jump_host_id, tags, notes, keepalive_secs";

fn auth_columns(auth: HostAuth) -> (&'static str, Option<String>) {
    match auth {
        HostAuth::Ask => ("ask", None),
        HostAuth::Password => ("password", None),
        HostAuth::Key(k) => ("key", Some(k.to_string())),
    }
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Host> {
    let tags: String = row.get(9)?;
    let kind: String = row.get(5)?;
    let key: Option<KeyId> = parse_opt_col(6, row.get(6)?)?;
    let auth = match (kind.as_str(), key) {
        ("key", Some(k)) => HostAuth::Key(k),
        ("password", _) => HostAuth::Password,
        _ => HostAuth::Ask,
    };
    Ok(Host {
        id: parse_col(0, &row.get::<_, String>(0)?)?,
        label: row.get(1)?,
        hostname: row.get(2)?,
        port: row.get(3)?,
        username: row.get(4)?,
        auth,
        group_id: parse_opt_col(7, row.get(7)?)?,
        jump_host_id: parse_opt_col(8, row.get(8)?)?,
        tags: serde_json::from_str(&tags).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e))
        })?,
        notes: row.get(10)?,
        keepalive_secs: row.get(11)?,
    })
}

impl<'c> HostRepo<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    pub fn list(&self) -> Result<Vec<Host>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM hosts ORDER BY label COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, id: HostId) -> Result<Option<Host>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM hosts WHERE id = ?1"),
                [id.to_string()],
                from_row,
            )
            .optional()?)
    }

    pub fn insert(&self, host: &Host) -> Result<()> {
        host.validate()?;
        let (auth_kind, auth_key) = auth_columns(host.auth);
        self.conn.execute(
            &format!(
                "INSERT INTO hosts ({COLS}, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, {NOW}, {NOW})"
            ),
            params![
                host.id.to_string(),
                host.label,
                host.hostname,
                host.port,
                host.username,
                auth_kind,
                auth_key,
                host.group_id.map(|g| g.to_string()),
                host.jump_host_id.map(|j| j.to_string()),
                serde_json::to_string(&host.tags).map_err(|e| Error::Corrupt(e.to_string()))?,
                host.notes,
                host.keepalive_secs,
            ],
        )?;
        Ok(())
    }

    pub fn update(&self, host: &Host) -> Result<()> {
        host.validate()?;
        let (auth_kind, auth_key) = auth_columns(host.auth);
        let n = self.conn.execute(
            &format!(
                "UPDATE hosts SET label = ?2, hostname = ?3, port = ?4, username = ?5, \
                 auth_kind = ?6, ssh_key_id = ?7, group_id = ?8, jump_host_id = ?9, tags = ?10, \
                 notes = ?11, keepalive_secs = ?12, updated_at = {NOW} WHERE id = ?1"
            ),
            params![
                host.id.to_string(),
                host.label,
                host.hostname,
                host.port,
                host.username,
                auth_kind,
                auth_key,
                host.group_id.map(|g| g.to_string()),
                host.jump_host_id.map(|j| j.to_string()),
                serde_json::to_string(&host.tags).map_err(|e| Error::Corrupt(e.to_string()))?,
                host.notes,
                host.keepalive_secs,
            ],
        )?;
        require_changed(n, "host")
    }

    /// Set or clear (`None`) the stored password. Always sealed for this host.
    pub fn set_password(
        &self,
        id: HostId,
        password: Option<&SecretString>,
        vault: &Vault,
    ) -> Result<()> {
        let enc = password
            .map(|p| seal_secret(vault, p, "hosts", "password_enc", &id))
            .transpose()?;
        let n = self.conn.execute(
            &format!(
                "UPDATE hosts SET password_enc = ?2, password_aad = NULL, updated_at = {NOW} \
                 WHERE id = ?1"
            ),
            params![id.to_string(), enc],
        )?;
        require_changed(n, "host")
    }

    pub fn has_password(&self, id: HostId) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT password_enc IS NOT NULL FROM hosts WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    /// Explicitly decrypt the stored password.
    pub fn get_password(&self, id: HostId, vault: &Vault) -> Result<Option<SecretString>> {
        let row: Option<(Option<Vec<u8>>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT password_enc, password_aad FROM hosts WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((Some(blob), legacy_aad)) = row else {
            return Ok(None);
        };
        match legacy_aad {
            // Copied from an identity by migration 002; bound to the identity's AAD.
            Some(aad) => {
                let plain = vault.open(&blob, aad.as_bytes())?;
                use secrecy::ExposeSecret;
                let text = std::str::from_utf8(plain.expose_secret())
                    .map_err(|_| Error::Corrupt("hosts.password_enc is not valid UTF-8".into()))?;
                Ok(Some(SecretString::from(text.to_owned())))
            }
            None => Ok(Some(open_secret(
                vault,
                &blob,
                "hosts",
                "password_enc",
                &id,
            )?)),
        }
    }

    /// Number of hosts that authenticate with `key`.
    pub fn count_using_key(&self, key: KeyId) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM hosts WHERE ssh_key_id = ?1",
            [key.to_string()],
            |r| r.get(0),
        )?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    pub fn delete(&self, id: HostId) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM hosts WHERE id = ?1", [id.to_string()])?
            > 0)
    }

    /// Record a successful connection (used for "recent hosts" ordering).
    pub fn touch_last_connected(&self, id: HostId) -> Result<()> {
        let n = self.conn.execute(
            &format!("UPDATE hosts SET last_connected_at = {NOW} WHERE id = ?1"),
            [id.to_string()],
        )?;
        require_changed(n, "host")
    }

    /// Move a host to a group (or to the top level with `None`).
    pub fn set_group(&self, id: HostId, group: Option<tern_core::GroupId>) -> Result<()> {
        let n = self.conn.execute(
            &format!("UPDATE hosts SET group_id = ?2, updated_at = {NOW} WHERE id = ?1"),
            params![id.to_string(), group.map(|g| g.to_string())],
        )?;
        require_changed(n, "host")
    }

    /// `last_connected_at` for every host that has connected at least once.
    pub fn last_connected_all(&self) -> Result<std::collections::HashMap<HostId, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, last_connected_at FROM hosts WHERE last_connected_at IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                parse_col::<HostId>(0, &r.get::<_, String>(0)?)?,
                r.get::<_, String>(1)?,
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn last_connected_at(&self, id: HostId) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT last_connected_at FROM hosts WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }
}
