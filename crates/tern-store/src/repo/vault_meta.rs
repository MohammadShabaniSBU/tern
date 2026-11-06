use rusqlite::{Connection, OptionalExtension, params};
use tern_vault::VaultMeta;

use super::{NOW, require_changed};
use crate::{Error, Result};

/// The single `vault_meta` row needed to unlock the vault.
pub struct VaultMetaRepo<'c> {
    conn: &'c Connection,
}

impl<'c> VaultMetaRepo<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    pub fn exists(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM vault_meta WHERE id = 1", [], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn get(&self) -> Result<Option<VaultMeta>> {
        let row = self
            .conn
            .query_row(
                "SELECT kdf_params, kdf_salt, wrapped_data_key FROM vault_meta WHERE id = 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(params, kdf_salt, wrapped_data_key)| {
            Ok(VaultMeta {
                kdf_params: serde_json::from_str(&params)
                    .map_err(|_| Error::Corrupt("vault_meta.kdf_params".into()))?,
                kdf_salt,
                wrapped_data_key,
            })
        })
        .transpose()
    }

    /// Store the meta of a freshly created vault. Fails if one already exists.
    pub fn insert(&self, meta: &VaultMeta) -> Result<()> {
        self.conn.execute(
            &format!(
                "INSERT INTO vault_meta (id, kdf_params, kdf_salt, wrapped_data_key, created_at) \
                 VALUES (1, ?1, ?2, ?3, {NOW})"
            ),
            params![kdf_json(meta)?, meta.kdf_salt, meta.wrapped_data_key],
        )?;
        Ok(())
    }

    /// Replace the meta after a password change.
    pub fn update(&self, meta: &VaultMeta) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE vault_meta SET kdf_params = ?1, kdf_salt = ?2, wrapped_data_key = ?3 \
             WHERE id = 1",
            params![kdf_json(meta)?, meta.kdf_salt, meta.wrapped_data_key],
        )?;
        require_changed(n, "vault")
    }
}

fn kdf_json(meta: &VaultMeta) -> Result<String> {
    serde_json::to_string(&meta.kdf_params).map_err(|e| Error::Corrupt(e.to_string()))
}
