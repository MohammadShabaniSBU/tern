//! SQLite persistence for tern: connection setup, migrations and (later) repositories.

mod db;
mod error;

pub use db::{open, open_in_memory};
pub use error::{Error, Result};
