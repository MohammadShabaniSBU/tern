mod app;
mod keymap;
mod logging;
mod paths;

use app::State;

fn main() -> anyhow::Result<()> {
    let data_dir = paths::ensure_data_dir()?;
    let _log_guard = logging::init(&data_dir)?;
    tracing::info!(data_dir = %data_dir.display(), "tern starting");

    iced::application(State::default, State::update, State::view)
        .title("tern")
        .subscription(State::subscription)
        .theme(State::theme)
        .window_size((1100.0, 700.0))
        .run()?;
    Ok(())
}
