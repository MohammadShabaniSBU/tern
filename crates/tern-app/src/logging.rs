//! Logging setup: stdout in debug builds, a daily-rolling file in release builds.
//!
//! Filter with `RUST_LOG` (default: `info`). **Never log secrets** — see CONTRIBUTING.md.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::EnvFilter;

const LOG_FILE_PREFIX: &str = "tern.log";

/// Keeps the background log writer alive; drop it at shutdown to flush.
#[must_use = "dropping the guard stops file logging"]
pub struct LogGuard(#[allow(dead_code)] Option<WorkerGuard>);

pub fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Install the global tracing subscriber.
pub fn init(data_dir: &Path) -> Result<LogGuard> {
    if cfg!(debug_assertions) {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter())
            .init();
        Ok(LogGuard(None))
    } else {
        let (writer, guard) = file_writer(&log_dir(data_dir))?;
        tracing_subscriber::fmt()
            .with_env_filter(env_filter())
            .with_ansi(false)
            .with_writer(writer)
            .init();
        Ok(LogGuard(Some(guard)))
    }
}

fn file_writer(dir: &Path) -> Result<(NonBlocking, WorkerGuard)> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating log directory {}", dir.display()))?;
    Ok(tracing_appender::non_blocking(
        tracing_appender::rolling::daily(dir, LOG_FILE_PREFIX),
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn log_dir_is_under_data_dir() {
        assert_eq!(log_dir(Path::new("/d")), PathBuf::from("/d/logs"));
    }

    #[test]
    fn file_writer_creates_dir_and_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = log_dir(tmp.path());
        let (mut writer, guard) = file_writer(&dir).unwrap();
        writeln!(writer, "hello log").unwrap();
        drop(guard); // flushes

        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let path = entries.into_iter().next().unwrap().unwrap().path();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(LOG_FILE_PREFIX)
        );
        assert!(std::fs::read_to_string(path).unwrap().contains("hello log"));
    }
}
