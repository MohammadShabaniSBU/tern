# tern

A personal, local-first, keyboard-first SSH client written in Rust with [iced](https://iced.rs).
No server, no account, no sync: everything lives in one SQLite file, and secrets (passwords, private
keys, passphrases) are encrypted at rest behind a master password.

Features: host/group/tag manager, identities and SSH keys (generate, import, export), tabbed
terminals (alacritty_terminal emulation, search, copy mode), jump hosts (ProxyJump chains),
local/remote/dynamic (SOCKS5) port forwarding, import/export (encrypted backup, `~/.ssh/config`),
idle auto-lock, and a command palette for everything.

## Install

**Linux tarball** (build it with `just package`, or take `tern-<version>-linux-<arch>.tar.gz`):

```
tar xzf tern-*-linux-*.tar.gz
cd tern-*-linux-*
./install.sh                  # to ~/.local   (PREFIX=/usr/local sudo ./install.sh for system-wide)
tern
```

**Debian/Ubuntu**: `just deb` (needs `cargo install cargo-deb`), then `sudo apt install ./target/debian/tern_*.deb`.
**AppImage**: `just appimage` (needs `appimagetool`).
**From source**: `cargo install --path crates/tern-app` or `just run`.

Runtime needs the usual windowing/GPU libraries (Wayland or X11, Vulkan or OpenGL). macOS and
Windows packages are not provided.

## First run

1. Choose a **master password** and confirm it. tern shows a recovery warning once; there is no
   password reset, so keep the password safe (a password manager is a good home for it).
2. Add a host with `a` (or `Ctrl+K` → "Add"), optionally with an identity (username + password
   and/or key) and a jump host.
3. `Enter` connects. `Ctrl+K` opens the command palette, `F1` / `?` the cheat sheet,
   `Ctrl+,` the settings.

## Keyboard

Everything is reachable from the keyboard; the full table is in
[`docs/KEYBINDINGS.md`](docs/KEYBINDINGS.md) and in the in-app cheat sheet (`F1`). Rebind keys in
**Settings → Keybindings** (select an action, `Enter`, press the new key; `r` resets, `Del`
unbinds; clashes are flagged before they are applied).

## Safety and backups

- The vault locks after 15 idle minutes (Settings → General; also after the computer sleeps).
  By default locking closes terminals and tunnels; there is an option to keep them running.
- Read [`docs/SECURITY.md`](docs/SECURITY.md) for what is and is not protected.
- **Back up regularly**: palette → "Export encrypted backup…" writes a `.tern` file protected by
  its own password (format: [`docs/EXPORT_FORMAT.md`](docs/EXPORT_FORMAT.md)). "Scheduled backup
  settings…" can write one automatically after each unlock into a folder you sync elsewhere.
  Losing both the master password and every backup means losing the stored secrets.
- Plaintext JSON export exists but includes secrets only if you type `INCLUDE SECRETS`.

## Data and configuration

| Item | Where |
|---|---|
| Database | `<data dir>/tern.db` (mode 0600) |
| Logs / crash reports | `<data dir>/logs/`, `<data dir>/crash-*.log` |
| Data dir | `~/.local/share/tern` (override with `TERN_DATA_DIR`) |
| `RUST_LOG` | Log filter, default `info`. Debug builds log to stdout, release builds to the logs dir. |

## Development

Requirements: Rust (the pinned toolchain in `rust-toolchain.toml` is installed by rustup) and
[`just`](https://github.com/casey/just).

```
just run      # cargo run -p tern-app
just fmt      # cargo fmt --all
just lint     # fmt --check + clippy -D warnings
just test     # cargo test --workspace
```

Planning docs live in [`docs/`](docs/README.md); see [CONTRIBUTING.md](CONTRIBUTING.md) — in
particular: never log secrets. Live SSH tests are described in `crates/tern-ssh/tests/live.rs`
(including the jump-host test with two containers).
