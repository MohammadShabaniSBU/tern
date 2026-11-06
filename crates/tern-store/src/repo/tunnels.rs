use rusqlite::{Connection, OptionalExtension, Row, params};
use tern_core::{HostId, Tunnel, TunnelId, TunnelKind};

use super::{NOW, parse_col, require_changed};
use crate::Result;

pub struct TunnelRepo<'c> {
    conn: &'c Connection,
}

const COLS: &str = "id, label, host_id, kind, bind_addr, bind_port, target_host, target_port, \
                    auto_start";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Tunnel> {
    let kind: String = row.get(3)?;
    let kind = TunnelKind::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            "unknown tunnel kind".into(),
        )
    })?;
    Ok(Tunnel {
        id: parse_col(0, &row.get::<_, String>(0)?)?,
        label: row.get(1)?,
        host_id: parse_col(2, &row.get::<_, String>(2)?)?,
        kind,
        bind_addr: row.get(4)?,
        bind_port: row.get(5)?,
        target_host: row.get(6)?,
        target_port: row.get(7)?,
        auto_start: row.get(8)?,
    })
}

impl<'c> TunnelRepo<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    pub fn list(&self) -> Result<Vec<Tunnel>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM tunnels ORDER BY label COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn list_for_host(&self, host_id: HostId) -> Result<Vec<Tunnel>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM tunnels WHERE host_id = ?1 ORDER BY label COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([host_id.to_string()], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, id: TunnelId) -> Result<Option<Tunnel>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM tunnels WHERE id = ?1"),
                [id.to_string()],
                from_row,
            )
            .optional()?)
    }

    pub fn insert(&self, t: &Tunnel) -> Result<()> {
        t.validate()?;
        self.conn.execute(
            &format!(
                "INSERT INTO tunnels ({COLS}, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, {NOW}, {NOW})"
            ),
            params![
                t.id.to_string(),
                t.label,
                t.host_id.to_string(),
                t.kind.as_str(),
                t.bind_addr,
                t.bind_port,
                t.target_host,
                t.target_port,
                t.auto_start,
            ],
        )?;
        Ok(())
    }

    pub fn update(&self, t: &Tunnel) -> Result<()> {
        t.validate()?;
        let n = self.conn.execute(
            &format!(
                "UPDATE tunnels SET label = ?2, host_id = ?3, kind = ?4, bind_addr = ?5, \
                 bind_port = ?6, target_host = ?7, target_port = ?8, auto_start = ?9, \
                 updated_at = {NOW} WHERE id = ?1"
            ),
            params![
                t.id.to_string(),
                t.label,
                t.host_id.to_string(),
                t.kind.as_str(),
                t.bind_addr,
                t.bind_port,
                t.target_host,
                t.target_port,
                t.auto_start,
            ],
        )?;
        require_changed(n, "tunnel")
    }

    pub fn delete(&self, id: TunnelId) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM tunnels WHERE id = ?1", [id.to_string()])?
            > 0)
    }
}
