# tern

A personal, local-first, keyboard-first SSH client written in Rust with [iced](https://iced.rs).
Planning docs live in [`docs/`](docs/README.md).

## Build & run

Requirements: Rust (the pinned toolchain in `rust-toolchain.toml` is installed automatically by rustup) and [`just`](https://github.com/casey/just) (`cargo install just`). On Linux you also need the usual windowing/GPU libraries (Wayland or X11, Vulkan/OpenGL drivers).

```
just run      # cargo run -p tern-app
just fmt      # cargo fmt --all
just lint     # fmt --check + clippy -D warnings
just test     # cargo test --workspace
```

Press `Ctrl+K` to toggle the (placeholder) command palette.

## Configuration

| Variable | Effect |
|---|---|
| `TERN_DATA_DIR` | Override the data directory (default: `~/.local/share/tern`, `~/Library/Application Support/tern`, `%APPDATA%\tern\data`). Created on startup. |
| `RUST_LOG` | Log filter, default `info`. Debug builds log to stdout, release builds to `<data dir>/logs/`. |

See [CONTRIBUTING.md](CONTRIBUTING.md) — in particular: never log secrets.
