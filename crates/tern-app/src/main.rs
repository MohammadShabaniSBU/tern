mod action;
mod app;
mod auth;
mod autolock;
mod cheatsheet;
mod crash;
mod host_form;
mod hosts_view;
mod identity_form;
mod io_form;
mod key_form;
mod keymap;
mod keys_view;
mod list;
mod logging;
mod modal;
mod palette;
mod paths;
mod prompt;
mod reauth;
mod sessions;
mod settings;
mod term_settings;
mod toast;
mod tunnel_form;
mod tunnel_run;
mod tunnels_view;

use app::State;

fn main() -> anyhow::Result<()> {
    let data_dir = paths::ensure_data_dir()?;
    let _log_guard = logging::init(&data_dir)?;
    crash::install(data_dir.clone());
    tracing::info!(data_dir = %data_dir.display(), "tern starting");

    let store = match tern_store::Store::open(&data_dir.join("tern.db")) {
        Ok(store) => store,
        Err(e) => {
            tracing::error!(error = %e, "could not open the database");
            anyhow::bail!("{}", e.user_message());
        }
    };

    iced::application(
        move || State::new(store.clone()),
        State::update,
        State::view,
    )
    .title("tern")
    .font(app::FONT_BYTES)
    .default_font(app::MONO)
    .subscription(State::subscription)
    .theme(State::theme)
    .window_size((1100.0, 700.0))
    .run()?;
    Ok(())
}
