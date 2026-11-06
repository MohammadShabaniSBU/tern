//! tern-term: terminal emulation adapter (and, later, the iced widget).

pub mod emu;
pub mod input;
pub mod theme;
pub mod widget;

pub use emu::{
    Cell, CellFlags, Color, Cursor, CursorShape, Emulator, Modes, RowSpan, SelectKind, Snapshot,
};

/// Errors produced by `tern-term`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
