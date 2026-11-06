//! SQLite persistence for tern: connection setup, migrations and repositories.

mod db;
mod error;
mod repo;
mod store;

pub use db::{open, open_in_memory};
pub use error::{Error, Result};
pub use repo::{
    GroupRepo, HostRepo, IdentityRepo, KeyRepo, KnownHostRepo, SettingsRepo, TunnelRepo,
    VaultMetaRepo,
};
pub use store::{Db, Store};
