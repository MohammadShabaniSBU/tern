use rusqlite::{Connection, OptionalExtension, Row, params};
use tern_core::{Host, HostId};

use super::{NOW, parse_col, parse_opt_col, require_changed};
use crate::{Error, Result};

pub struct HostRepo<'c> {
    conn: &'c Connection,
}

const COLS: &str = "id, label, hostname, port, username, identity_id, group_id, jump_host_id, \
                    tags, notes, keepalive_secs";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Host> {
    let tags: String = row.get(8)?;
    Ok(Host {
        id: parse_col(0, &row.get::<_, String>(0)?)?,
        label: row.get(1)?,
        hostname: row.get(2)?,
        port: row.get(3)?,
        username: row.get(4)?,
        identity_id: parse_opt_col(5, row.get(5)?)?,
        group_id: parse_opt_col(6, row.get(6)?)?,
        jump_host_id: parse_opt_col(7, row.get(7)?)?,
        tags: serde_json::from_str(&tags).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?,
        notes: row.get(9)?,
        keepalive_secs: row.get(10)?,
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
        self.conn.execute(
            &format!(
                "INSERT INTO hosts ({COLS}, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, {NOW}, {NOW})"
            ),
            params![
                host.id.to_string(),
                host.label,
                host.hostname,
                host.port,
                host.username,
                host.identity_id.map(|i| i.to_string()),
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
        let n = self.conn.execute(
            &format!(
                "UPDATE hosts SET label = ?2, hostname = ?3, port = ?4, username = ?5, \
                 identity_id = ?6, group_id = ?7, jump_host_id = ?8, tags = ?9, notes = ?10, \
                 keepalive_secs = ?11, updated_at = {NOW} WHERE id = ?1"
            ),
            params![
                host.id.to_string(),
                host.label,
                host.hostname,
                host.port,
                host.username,
                host.identity_id.map(|i| i.to_string()),
                host.group_id.map(|g| g.to_string()),
                host.jump_host_id.map(|j| j.to_string()),
                serde_json::to_string(&host.tags).map_err(|e| Error::Corrupt(e.to_string()))?,
                host.notes,
                host.keepalive_secs,
            ],
        )?;
        require_changed(n, "host")
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
