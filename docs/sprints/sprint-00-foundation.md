# Sprint 0 — Foundation

**Goal:** A compiling Cargo workspace with all crates stubbed, CI-quality tooling, logging, and an iced window that opens and responds to a keypress.

**Exit demo:** `cargo run -p tern-app` opens a window titled "tern"; pressing `Ctrl+K` toggles a placeholder "palette" label; logs appear in the console.

---

### S0-T01 — Create the workspace (S)
- Root `Cargo.toml` with `[workspace]`, `resolver = "2"`, shared `[workspace.package]` (edition, license, version) and `[workspace.dependencies]` for shared crates (`serde`, `thiserror`, `tracing`, `uuid`, `tokio`).
- Create crates per `ARCHITECTURE.md`: `tern-core`, `tern-vault`, `tern-store`, `tern-ssh`, `tern-term`, `tern-io` (libs) and `tern-app` (bin).

**Acceptance:** `cargo build --workspace` succeeds; dependency graph matches the architecture (no lib crate depends on `tern-app`).

### S0-T02 — Tooling & lint config (S)
- `rustfmt.toml`, `clippy.toml` (or `[workspace.lints]` in Cargo.toml) — deny `unwrap_used` outside tests in lib crates.
- `rust-toolchain.toml` pinning a stable version.
- `justfile` or `Makefile` with `fmt`, `lint`, `test`, `run` targets.

**Acceptance:** `just lint` and `just test` run clean.

### S0-T03 — CI (S, optional for personal use)
- GitHub Actions (or local pre-commit hook) running fmt, clippy, test on Linux/macOS/Windows.

**Acceptance:** A pushed commit runs the pipeline green.

### S0-T04 — Domain types in `tern-core` (M)
- Define `HostId`, `IdentityId`, `KeyId`, `TunnelId`, `GroupId` newtypes over `Uuid`.
- Structs: `Host`, `Group`, `Identity`, `SshKeyMeta` (no private material), `Tunnel { kind: TunnelKind }`, `TunnelKind::{Local, Remote, Dynamic}`.
- `serde` derives; validation functions (port range, non-empty hostname, label length).

**Acceptance:** Unit tests for validation; types compile without any I/O dependency.

### S0-T05 — iced app skeleton (M)
- `tern-app/src/main.rs` with `iced::application(...)`, `State`, `Message`, `update`, `view`, `subscription`, theme.
- Enable iced features: `tokio`, `canvas`, `advanced`.
- Placeholder layout: sidebar, main area, status bar.

**Acceptance:** Window opens; resizing works; status bar shows "NORMAL".

### S0-T06 — Global keyboard subscription spike (S)
- Subscribe to keyboard events (`iced::keyboard::on_key_press` or `iced::event::listen_with`) and map `Ctrl+K` to `Message::TogglePalette`.
- Document in code where the future keymap engine will plug in.

**Acceptance:** `Ctrl+K` toggles placeholder text.

### S0-T07 — Logging & error plumbing (S)
- `tracing-subscriber` with `RUST_LOG` env filter; log file in the platform data dir (via `directories`).
- Each lib crate gets an `Error` enum via `thiserror`; app uses `anyhow` at the edges.
- Add a `redact!`/`Secret` convention note in `CONTRIBUTING.md`: never log secrets.

**Acceptance:** Logs go to stdout in debug and to a file in release.

### S0-T08 — App paths & config bootstrap (S)
- Resolve data dir (`~/.local/share/tern`, `%APPDATA%\tern`, `~/Library/Application Support/tern`).
- Allow override via `TERN_DATA_DIR` env var (useful for testing and portable installs).

**Acceptance:** Running with `TERN_DATA_DIR=/tmp/x` creates `/tmp/x` on startup.

---

**Definition of Done:** All tasks merged, exit demo works, README has build instructions.
