mod logging;
mod paths;

fn main() -> anyhow::Result<()> {
    let data_dir = paths::ensure_data_dir()?;
    let _log_guard = logging::init(&data_dir)?;
    tracing::info!(data_dir = %data_dir.display(), "tern starting");
    Ok(())
}
