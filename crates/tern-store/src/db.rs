use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::Result;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(include_str!("../migrations/001_init.sql"))])
}

/// Open (creating if needed) the database at `path`, apply pragmas and migrate to the latest
/// schema. On Unix the file is created with mode `0600`.
pub fn open(path: &Path) -> Result<Connection> {
    restrict_permissions(path)?;
    let mut conn = Connection::open(path)?;
    configure(&conn, true)?;
    migrations().to_latest(&mut conn)?;
    tracing::debug!(path = %path.display(), "database ready");
    Ok(conn)
}

/// An in-memory, fully migrated database (tests, previews).
pub fn open_in_memory() -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn, false)?;
    migrations().to_latest(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &Connection, wal: bool) -> Result<()> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    if wal {
        // journal_mode returns a row, so it can't go through pragma_update.
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        tracing::debug!(%mode, "journal mode");
        conn.pragma_update(None, "synchronous", "NORMAL")?;
    }
    Ok(())
}

/// Create the file (if missing) with owner-only permissions *before* SQLite does, so it is never
/// world-readable, and tighten an existing file. SQLite gives `-wal`/`-shm` the same mode.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::fs::{OpenOptions, Permissions, set_permissions};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    set_permissions(path, Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_names(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn migrations_are_valid() {
        migrations().validate().unwrap();
    }

    #[test]
    fn in_memory_db_has_full_schema() {
        let conn = open_in_memory().unwrap();
        assert_eq!(
            table_names(&conn),
            [
                "groups",
                "hosts",
                "identities",
                "known_hosts",
                "settings",
                "ssh_keys",
                "tunnels",
                "vault_meta"
            ]
        );
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let conn = open_in_memory().unwrap();
        let on: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(on, 1);
        let err = conn.execute(
            "INSERT INTO tunnels (id, label, host_id, kind, bind_port, created_at, updated_at) \
             VALUES ('t', 't', 'missing', 'local', 80, 'x', 'x')",
            [],
        );
        assert!(err.is_err());
    }

    #[test]
    fn rerunning_migrations_is_a_noop() {
        let mut conn = open_in_memory().unwrap();
        let version = |c: &Connection| -> i64 {
            c.query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap()
        };
        let before = version(&conn);
        assert_eq!(before, 1);
        migrations().to_latest(&mut conn).unwrap();
        assert_eq!(version(&conn), before);
        assert_eq!(migrations().pending_migrations(&conn).unwrap(), 0);
    }

    #[test]
    fn file_db_persists_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tern.db");
        {
            let conn = open(&path).unwrap();
            conn.execute("INSERT INTO settings (key, value) VALUES ('k', '1')", [])
                .unwrap();
            let mode: String = conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
        }
        let conn = open(&path).unwrap();
        let value: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'k'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(value, "1");
    }

    #[cfg(unix)]
    #[test]
    fn db_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tern.db");
        let conn = open(&path).unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES ('k', '1')", [])
            .unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        let wal = dir.path().join("tern.db-wal");
        if wal.exists() {
            assert_eq!(mode(&wal), 0o600);
        }
    }

    #[cfg(unix)]
    #[test]
    fn existing_permissive_file_is_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tern.db");
        std::fs::write(&path, b"").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        open(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
