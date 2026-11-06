//! Panic hook (S8-T05): a panic is logged and written to a crash report instead of vanishing.
//! Reports contain the panic message, location and backtrace only; secrets are never formatted
//! into panic messages by this code base.

use std::backtrace::Backtrace;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn format_report(message: &str, location: &str, backtrace: &str) -> String {
    format!(
        "tern {} crashed\nmessage: {message}\nat: {location}\n\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION")
    )
}

fn report_path(dir: &Path) -> PathBuf {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    dir.join(format!("crash-{secs}.log"))
}

/// Install the hook. Reports go to `<dir>/crash-<unix time>.log` (owner-only on unix) and the
/// log; the previous hook still runs, so the message also reaches stderr.
pub fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = match info.payload().downcast_ref::<&str>() {
            Some(s) => (*s).to_owned(),
            None => info
                .payload()
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "(non-string panic payload)".into()),
        };
        let location = info
            .location()
            .map_or_else(|| "unknown".to_owned(), ToString::to_string);
        let report = format_report(&message, &location, &Backtrace::force_capture().to_string());
        tracing::error!(%message, %location, "panic");
        let path = report_path(&dir);
        match write_private(&path, &report) {
            Ok(()) => eprintln!("tern crashed; report written to {}", path.display()),
            Err(e) => eprintln!("tern crashed; could not write a report: {e}"),
        }
        previous(info);
    }));
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_names_message_and_location() {
        let r = format_report("boom", "src/x.rs:1:2", "frame");
        assert!(r.contains("message: boom") && r.contains("src/x.rs:1:2") && r.contains("frame"));
    }

    #[test]
    fn reports_are_written_once_and_privately() {
        let dir = std::env::temp_dir().join(format!("tern-crash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("c.log");
        write_private(&p, "x").unwrap();
        assert!(write_private(&p, "y").is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "x");
        std::fs::remove_dir_all(&dir).ok();
    }
}
