//! Async facade over the database.
//!
//! A dedicated OS thread owns the SQLite [`Connection`] and (once unlocked) the [`Vault`].
//! [`Store`] is a cheap, cloneable handle: each async method ships a closure to that thread and
//! awaits the reply on a oneshot channel, so UI code (iced `update`) never blocks on disk I/O or
//! on Argon2. The data key never leaves the worker thread.
//!
//! The worker exits, dropping (and zeroizing) the vault, when the last handle is dropped.

use std::path::Path;
use std::sync::mpsc;

use rusqlite::Connection;
use secrecy::SecretString;
use tern_core::{Host, HostId, Identity, IdentityId};
use tern_vault::{KdfParams, Vault};
use tokio::sync::oneshot;

use crate::repo::{
    GroupRepo, HostRepo, IdentityRepo, KeyRepo, KnownHostRepo, SettingsRepo, TunnelRepo,
    VaultMetaRepo,
};
use crate::{Error, Result};

/// Everything the worker thread owns. Closures passed to [`Store::query`] get a shared `&Db`.
pub struct Db {
    conn: Connection,
    vault: Option<Vault>,
}

impl Db {
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// The unlocked vault, or [`Error::Locked`].
    pub fn vault(&self) -> Result<&Vault> {
        self.vault.as_ref().ok_or(Error::Locked)
    }

    pub fn hosts(&self) -> HostRepo<'_> {
        HostRepo::new(&self.conn)
    }
    pub fn groups(&self) -> GroupRepo<'_> {
        GroupRepo::new(&self.conn)
    }
    pub fn identities(&self) -> IdentityRepo<'_> {
        IdentityRepo::new(&self.conn)
    }
    pub fn keys(&self) -> KeyRepo<'_> {
        KeyRepo::new(&self.conn)
    }
    pub fn tunnels(&self) -> TunnelRepo<'_> {
        TunnelRepo::new(&self.conn)
    }
    pub fn known_hosts(&self) -> KnownHostRepo<'_> {
        KnownHostRepo::new(&self.conn)
    }
    pub fn settings(&self) -> SettingsRepo<'_> {
        SettingsRepo::new(&self.conn)
    }
    pub fn vault_meta(&self) -> VaultMetaRepo<'_> {
        VaultMetaRepo::new(&self.conn)
    }
}

type Job = Box<dyn FnOnce(&mut Db) + Send>;

/// Cloneable async handle to the database worker thread.
#[derive(Clone)]
pub struct Store {
    tx: mpsc::Sender<Job>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Open (creating and migrating if needed) the database file and start the worker.
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self::spawn(crate::open(path)?))
    }

    /// A fully migrated in-memory store (tests).
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self::spawn(crate::open_in_memory()?))
    }

    fn spawn(conn: Connection) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("tern-store".into())
            .spawn(move || {
                let mut db = Db { conn, vault: None };
                while let Ok(job) = rx.recv() {
                    job(&mut db);
                }
                tracing::debug!("store worker stopped");
            })
            // Failing to spawn a thread at startup is unrecoverable.
            .expect("failed to spawn store worker thread");
        Self { tx }
    }

    async fn run<R, F>(&self, f: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut Db) -> Result<R> + Send + 'static,
    {
        let (reply, response) = oneshot::channel();
        let job: Job = Box::new(move |db| {
            // The receiver may have been dropped (e.g. a cancelled task); that's fine.
            let _ = reply.send(f(db));
        });
        self.tx.send(job).map_err(|_| Error::Closed)?;
        response.await.map_err(|_| Error::Closed)?
    }

    /// Run an arbitrary read/write closure on the worker thread.
    ///
    /// This is the escape hatch for anything without a dedicated method. Keep closures short:
    /// the worker handles one job at a time.
    pub async fn query<R, F>(&self, f: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce(&Db) -> Result<R> + Send + 'static,
    {
        self.run(move |db| f(db)).await
    }

    // ---------- vault ----------

    pub async fn vault_exists(&self) -> Result<bool> {
        self.query(|db| db.vault_meta().exists()).await
    }

    pub async fn is_unlocked(&self) -> Result<bool> {
        self.query(|db| Ok(db.vault.is_some())).await
    }

    /// Create the vault with the default (slow) KDF parameters and leave it unlocked.
    pub async fn create_vault(&self, password: SecretString) -> Result<()> {
        self.create_vault_with_params(password, KdfParams::default())
            .await
    }

    /// Like [`Store::create_vault`] with explicit KDF parameters (tests use cheap ones).
    pub async fn create_vault_with_params(
        &self,
        password: SecretString,
        params: KdfParams,
    ) -> Result<()> {
        self.run(move |db| {
            if db.vault_meta().exists()? {
                return Err(Error::VaultExists);
            }
            let (vault, meta) = Vault::create_with_params(&password, params)?;
            db.vault_meta().insert(&meta)?;
            db.vault = Some(vault);
            Ok(())
        })
        .await
    }

    /// Unlock the vault. A wrong password is `Error::Vault(tern_vault::Error::WrongPassword)`.
    pub async fn unlock(&self, password: SecretString) -> Result<()> {
        self.run(move |db| {
            let meta = db.vault_meta().get()?.ok_or(Error::NoVault)?;
            db.vault = Some(Vault::unlock(&password, &meta)?);
            Ok(())
        })
        .await
    }

    /// Drop the data key. Secret operations fail with [`Error::Locked`] until unlocked again.
    pub async fn lock(&self) -> Result<()> {
        self.run(|db| {
            db.vault = None;
            Ok(())
        })
        .await
    }

    /// Re-wrap the data key under a new master password (verifies `old`).
    pub async fn change_master_password(&self, old: SecretString, new: SecretString) -> Result<()> {
        self.run(move |db| {
            let meta = db.vault_meta().get()?.ok_or(Error::NoVault)?;
            let new_meta = db.vault()?.change_password(&old, &new, &meta)?;
            db.vault_meta().update(&new_meta)
        })
        .await
    }

    // ---------- hosts ----------

    pub async fn list_hosts(&self) -> Result<Vec<Host>> {
        self.query(|db| db.hosts().list()).await
    }

    pub async fn get_host(&self, id: HostId) -> Result<Option<Host>> {
        self.query(move |db| db.hosts().get(id)).await
    }

    pub async fn insert_host(&self, host: Host) -> Result<()> {
        self.query(move |db| db.hosts().insert(&host)).await
    }

    pub async fn update_host(&self, host: Host) -> Result<()> {
        self.query(move |db| db.hosts().update(&host)).await
    }

    pub async fn delete_host(&self, id: HostId) -> Result<bool> {
        self.query(move |db| db.hosts().delete(id)).await
    }

    // ---------- identities ----------

    pub async fn list_identities(&self) -> Result<Vec<Identity>> {
        self.query(|db| db.identities().list()).await
    }

    /// Insert an identity, optionally with a password. Requires an unlocked vault (the UI only
    /// edits identities while unlocked), even when no password is given.
    pub async fn insert_identity(
        &self,
        identity: Identity,
        password: Option<SecretString>,
    ) -> Result<()> {
        self.query(move |db| {
            db.identities()
                .insert(&identity, password.as_ref(), db.vault()?)
        })
        .await
    }

    pub async fn get_identity_password(&self, id: IdentityId) -> Result<Option<SecretString>> {
        self.query(move |db| db.identities().get_password(id, db.vault()?))
            .await
    }

    pub async fn delete_identity(&self, id: IdentityId) -> Result<bool> {
        self.query(move |db| db.identities().delete(id)).await
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s)
    }

    fn fast() -> KdfParams {
        KdfParams::new(8, 1, 1).unwrap()
    }

    #[tokio::test]
    async fn list_hosts_from_async_context() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.list_hosts().await.unwrap().is_empty());
        let host = Host::new("web", "web.example.com");
        store.insert_host(host.clone()).await.unwrap();
        assert_eq!(store.list_hosts().await.unwrap(), vec![host.clone()]);
        assert_eq!(store.get_host(host.id).await.unwrap(), Some(host.clone()));
        assert!(store.delete_host(host.id).await.unwrap());
    }

    #[tokio::test]
    async fn handles_are_cloneable_and_concurrent() {
        let store = Store::open_in_memory().unwrap();
        let mut tasks = Vec::new();
        for i in 0..20 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .insert_host(Host::new(format!("h{i}"), "example.com"))
                    .await
            }));
        }
        for t in tasks {
            t.await.unwrap().unwrap();
        }
        assert_eq!(store.list_hosts().await.unwrap().len(), 20);
    }

    #[tokio::test]
    async fn vault_lifecycle() {
        let store = Store::open_in_memory().unwrap();
        assert!(!store.vault_exists().await.unwrap());
        assert!(matches!(store.unlock(pw("x")).await, Err(Error::NoVault)));

        store
            .create_vault_with_params(pw("master"), fast())
            .await
            .unwrap();
        assert!(store.vault_exists().await.unwrap());
        assert!(store.is_unlocked().await.unwrap());
        assert!(matches!(
            store.create_vault_with_params(pw("again"), fast()).await,
            Err(Error::VaultExists)
        ));

        store.lock().await.unwrap();
        assert!(!store.is_unlocked().await.unwrap());
        assert!(matches!(
            store.unlock(pw("wrong")).await,
            Err(Error::Vault(tern_vault::Error::WrongPassword))
        ));
        assert!(!store.is_unlocked().await.unwrap());
        store.unlock(pw("master")).await.unwrap();
        assert!(store.is_unlocked().await.unwrap());
    }

    #[tokio::test]
    async fn secrets_require_unlocked_vault_and_round_trip() {
        let store = Store::open_in_memory().unwrap();
        store
            .create_vault_with_params(pw("master"), fast())
            .await
            .unwrap();
        let ident = Identity {
            id: IdentityId::new(),
            label: "me".into(),
            username: "root".into(),
            ssh_key_id: None,
        };
        store
            .insert_identity(ident.clone(), Some(pw("hunter2")))
            .await
            .unwrap();

        // Listing works while locked and never decrypts.
        store.lock().await.unwrap();
        assert_eq!(store.list_identities().await.unwrap(), vec![ident.clone()]);
        assert!(matches!(
            store.get_identity_password(ident.id).await,
            Err(Error::Locked)
        ));

        store.unlock(pw("master")).await.unwrap();
        let got = store
            .get_identity_password(ident.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.expose_secret(), "hunter2");
    }

    #[tokio::test]
    async fn change_master_password_keeps_data_readable() {
        let store = Store::open_in_memory().unwrap();
        store
            .create_vault_with_params(pw("old"), fast())
            .await
            .unwrap();
        let ident = Identity {
            id: IdentityId::new(),
            label: "me".into(),
            username: "root".into(),
            ssh_key_id: None,
        };
        store
            .insert_identity(ident.clone(), Some(pw("secret")))
            .await
            .unwrap();

        assert!(matches!(
            store.change_master_password(pw("nope"), pw("new")).await,
            Err(Error::Vault(tern_vault::Error::WrongPassword))
        ));
        store
            .change_master_password(pw("old"), pw("new"))
            .await
            .unwrap();
        store.lock().await.unwrap();
        assert!(store.unlock(pw("old")).await.is_err());
        store.unlock(pw("new")).await.unwrap();
        assert_eq!(
            store
                .get_identity_password(ident.id)
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "secret"
        );
    }

    #[tokio::test]
    async fn file_store_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tern.db");
        let host = Host::new("web", "web.example.com");
        {
            let store = Store::open(&path).unwrap();
            store
                .create_vault_with_params(pw("master"), fast())
                .await
                .unwrap();
            store.insert_host(host.clone()).await.unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert!(store.vault_exists().await.unwrap());
        assert!(!store.is_unlocked().await.unwrap());
        assert_eq!(store.list_hosts().await.unwrap(), vec![host]);
        store.unlock(pw("master")).await.unwrap();
    }
}
