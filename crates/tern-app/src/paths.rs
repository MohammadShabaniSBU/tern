//! Where tern keeps its files.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;

/// Environment variable that overrides the data directory (tests, portable installs).
pub const DATA_DIR_ENV: &str = "TERN_DATA_DIR";

/// Resolve the data directory without touching the filesystem.
///
/// `TERN_DATA_DIR` wins if set and non-empty; otherwise the platform default
/// (`~/.local/share/tern`, `~/Library/Application Support/tern`, `%APPDATA%\tern\data`).
pub fn resolve_data_dir() -> Result<PathBuf> {
    resolve_data_dir_with(std::env::var_os(DATA_DIR_ENV))
}

fn resolve_data_dir_with(override_dir: Option<OsString>) -> Result<PathBuf> {
    match override_dir.filter(|v| !v.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => ProjectDirs::from("", "", "tern")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .ok_or_else(|| anyhow!("could not determine a home directory for the data dir")),
    }
}

/// Resolve the data directory and create it (and parents) if missing.
pub fn ensure_data_dir() -> Result<PathBuf> {
    let dir = resolve_data_dir()?;
    create_dir(&dir)?;
    Ok(dir)
}

fn create_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating data directory {}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins() {
        let dir = resolve_data_dir_with(Some("/tmp/x".into())).unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/x"));
    }

    #[test]
    fn empty_override_falls_back_to_default() {
        let default = resolve_data_dir_with(None).unwrap();
        assert_eq!(resolve_data_dir_with(Some("".into())).unwrap(), default);
        assert!(default.ends_with("tern") || default.ends_with("data"));
    }

    #[test]
    fn creates_nested_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b");
        create_dir(&nested).unwrap();
        assert!(nested.is_dir());
        // idempotent
        create_dir(&nested).unwrap();
    }
}
