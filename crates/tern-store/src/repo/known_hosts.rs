use rusqlite::{Connection, OptionalExtension, Row, params};
use tern_core::{KnownHost, KnownHostId};

use super::{NOW, parse_col, require_changed};
use crate::Result;

pub struct KnownHostRepo<'c> {
    conn: &'c Connection,
}

const COLS: &str = "id, host, port, key_type, fingerprint, public_key";

fn from_row(row: &Row<'_>) -> rusqlite::Result<KnownHost> {
    Ok(KnownHost {
        id: parse_col(0, &row.get::<_, String>(0)?)?,
        host: row.get(1)?,
        port: row.get(2)?,
        key_type: row.get(3)?,
        fingerprint: row.get(4)?,
        public_key: row.get(5)?,
    })
}

impl<'c> KnownHostRepo<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    pub fn list(&self) -> Result<Vec<KnownHost>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM known_hosts ORDER BY host, port, key_type"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, id: KnownHostId) -> Result<Option<KnownHost>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM known_hosts WHERE id = ?1"),
                [id.to_string()],
                from_row,
            )
            .optional()?)
    }

    /// Look up the trusted key of a given type for `host:port` (used for host-key verification).
    pub fn find(&self, host: &str, port: u16, key_type: &str) -> Result<Option<KnownHost>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {COLS} FROM known_hosts WHERE host = ?1 AND port = ?2 AND key_type = ?3"
                ),
                params![host, port, key_type],
                from_row,
            )
            .optional()?)
    }

    /// Fails on a duplicate `(host, port, key_type)`.
    pub fn insert(&self, k: &KnownHost) -> Result<()> {
        k.validate()?;
        self.conn.execute(
            &format!(
                "INSERT INTO known_hosts ({COLS}, first_seen_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, {NOW})"
            ),
            params![
                k.id.to_string(),
                k.host,
                k.port,
                k.key_type,
                k.fingerprint,
                k.public_key
            ],
        )?;
        Ok(())
    }

    /// Replace the trusted key (e.g. the user accepted a changed host key).
    pub fn update(&self, k: &KnownHost) -> Result<()> {
        k.validate()?;
        let n = self.conn.execute(
            "UPDATE known_hosts SET host = ?2, port = ?3, key_type = ?4, fingerprint = ?5, \
             public_key = ?6 WHERE id = ?1",
            params![
                k.id.to_string(),
                k.host,
                k.port,
                k.key_type,
                k.fingerprint,
                k.public_key
            ],
        )?;
        require_changed(n, "known host")
    }

    pub fn delete(&self, id: KnownHostId) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM known_hosts WHERE id = ?1", [id.to_string()])?
            > 0)
    }
}
