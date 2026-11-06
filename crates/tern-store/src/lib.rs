//! SQLite persistence for tern: connection setup, migrations and repositories.

mod db;
mod error;
mod repo;

pub use db::{open, open_in_memory};
pub use error::{Error, Result};
pub use repo::{
    GroupRepo, HostRepo, IdentityRepo, KeyRepo, KnownHostRepo, SettingsRepo, TunnelRepo,
    VaultMetaRepo,
};
