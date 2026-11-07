mod action;
mod app;
mod auth;
mod autolock;
mod cheatsheet;
mod cli;
mod crash;
mod host_form;
mod hosts_view;
mod icons;
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
mod select;
mod sessions;
mod settings;
mod sftp_form;
mod term_settings;
mod theme;
mod toast;
mod tunnel_form;
mod tunnel_run;
mod tunnels_view;

use app::State;

/// wgpu probes every Vulkan/GL driver it finds (NVIDIA + Mesa/LLVM here), which costs ~65 MB of
/// resident memory. Unless the user chose a backend, restart ourselves pinned to Vulkan.
/// (`set_var` is `unsafe` in edition 2024 and `unsafe` is forbidden, so re-exec instead.)
#[cfg(unix)]
fn pin_gpu_backend() {
    use std::os::unix::process::CommandExt;
    if std::env::var_os("WGPU_BACKEND").is_some() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        // Only returns on failure; then we simply carry on with the default.
        let _ = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env("WGPU_BACKEND", "vulkan")
            .exec();
    }
}

/// `tern -c <host>`: no GUI, no logging to the terminal; the exit code is the remote's.
fn run_cli(name: Option<&str>) -> anyhow::Result<()> {
    let data_dir = paths::ensure_data_dir()?;
    let store = match tern_store::Store::open(&data_dir.join("tern.db")) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("tern: {}", e.user_message());
            std::process::exit(1);
        }
    };
    let Some(name) = name else {
        // Completion helper: stay quiet on failure (e.g. the GUI holds the database).
        let _ = cli::list_hosts(store);
        return Ok(());
    };
    match cli::connect(store, name) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("tern: {e}");
            std::process::exit(1);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mode = match cli::parse_args(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(e) => {
            eprintln!("tern: {e}");
            std::process::exit(2);
        }
    };
    match mode {
        cli::Mode::Print(text) => {
            println!("{text}");
            return Ok(());
        }
        cli::Mode::Connect(name) => return run_cli(Some(&name)),
        cli::Mode::ListHosts => return run_cli(None),
        cli::Mode::Gui => {}
    }
    #[cfg(unix)]
    pin_gpu_backend();
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
    .font(theme::INTER_REGULAR)
    .font(theme::INTER_MEDIUM)
    .font(theme::INTER_SEMIBOLD)
    .default_font(theme::UI)
    .subscription(State::subscription)
    .theme(State::theme)
    .window_size((1100.0, 700.0))
    .run()?;
    Ok(())
}
