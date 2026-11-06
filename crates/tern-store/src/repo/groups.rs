use rusqlite::{Connection, OptionalExtension, Row, params};
use tern_core::{Group, GroupId};

use super::{parse_col, parse_opt_col, require_changed};
use crate::Result;

pub struct GroupRepo<'c> {
    conn: &'c Connection,
}

const COLS: &str = "id, name, parent_id, sort_order";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Group> {
    Ok(Group {
        id: parse_col(0, &row.get::<_, String>(0)?)?,
        name: row.get(1)?,
        parent_id: parse_opt_col(2, row.get(2)?)?,
        sort_order: row.get(3)?,
    })
}

impl<'c> GroupRepo<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    pub fn list(&self) -> Result<Vec<Group>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM groups ORDER BY sort_order, name"
        ))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, id: GroupId) -> Result<Option<Group>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM groups WHERE id = ?1"),
                [id.to_string()],
                from_row,
            )
            .optional()?)
    }

    pub fn insert(&self, group: &Group) -> Result<()> {
        group.validate()?;
        self.conn.execute(
            "INSERT INTO groups (id, name, parent_id, sort_order) VALUES (?1, ?2, ?3, ?4)",
            params![
                group.id.to_string(),
                group.name,
                group.parent_id.map(|p| p.to_string()),
                group.sort_order
            ],
        )?;
        Ok(())
    }

    pub fn update(&self, group: &Group) -> Result<()> {
        group.validate()?;
        let n = self.conn.execute(
            "UPDATE groups SET name = ?2, parent_id = ?3, sort_order = ?4 WHERE id = ?1",
            params![
                group.id.to_string(),
                group.name,
                group.parent_id.map(|p| p.to_string()),
                group.sort_order
            ],
        )?;
        require_changed(n, "group")
    }

    /// Delete a group. Hosts and subgroups inside it move to the top level (`ON DELETE SET NULL`).
    pub fn delete(&self, id: GroupId) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM groups WHERE id = ?1", [id.to_string()])?
            > 0)
    }
}
