//! SQLite persistence for tern: connection setup, migrations and repositories.

mod db;
mod error;
mod repo;
mod store;

pub use db::{open, open_in_memory};
pub use error::{Error, Result};
pub use repo::{
    GroupRepo, HostRepo, KeyRepo, KnownHostRepo, SettingsRepo, TunnelRepo, VaultMetaRepo,
};
pub use store::{Db, Store};
