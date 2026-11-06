# tern

Personal, local-first, keyboard-first SSH client (Termius-style) in Rust with the `iced` GUI.
No server, no account, no sync; all data in one SQLite file, secrets encrypted at rest behind a master password.

Full plan lives in `docs/` — read `docs/README.md`, `docs/ARCHITECTURE.md`, `docs/KEYBINDINGS.md`, then the current sprint file in `docs/sprints/`.

## Current status

Sprint 0 done (CI, S0-T03, skipped by the user). Sprint 1 done. Working through **Sprint 2 only** (`docs/sprints/sprint-02-keyboard-ui-shell.md`); done: S2-T01. Do not write code for later sprints.
- `tern-vault`: Argon2id KEK (128 MiB/8 passes, ~420 ms), XChaCha20-Poly1305 `seal/open`, `Vault` envelope (create/unlock/change_password/lock).
- `tern-store`: migrations (`migrations/001_init.sql`), repos (secrets only via explicit `Vault` methods; AAD `"<table>:<column>:<row id>"`), `Store` = cloneable async handle over one worker thread that owns the Connection and the unlocked Vault.
- `tern-app`: `app.rs` (Loading/Create/Unlock/Recovery/Main screens), `auth.rs` (forms, `Sensitive` password wrapper), `keymap.rs` (global key subscription; future keymap engine plugs in here), `logging.rs`, `paths.rs`.
- Pinned `rusqlite` 0.40 because `rusqlite_migration` 2.6 requires it (0.41 exists).
- Password text lives only in `Sensitive` (zeroized, redacted Debug); never put raw password `String`s in Messages/State.
- Don't screenshot the whole desktop to check the UI (it captures other windows); crop to the tern window or ask the user.

## Crate layout (`crates/`)

| Crate | Role |
|---|---|
| `tern-core` | Domain types (Host, Group, Identity, SshKeyMeta, Tunnel) + validation. No I/O. |
| `tern-vault` | KDF (Argon2id), AEAD (XChaCha20-Poly1305), data-key wrapping, secret helpers |
| `tern-store` | SQLite schema, migrations, repositories |
| `tern-ssh` | russh client wrapper, auth, known-hosts, tunnels (session actors over mpsc) |
| `tern-term` | alacritty_terminal adapter + iced terminal widget |
| `tern-io` | Backup bundle, ssh_config parser, key import |
| `tern-app` | iced binary: screens, keymap, palette |

Rules: only `tern-app` (and `tern-term`'s widget) depends on iced. No lib crate depends on `tern-app`.
Libs use `thiserror`; the binary uses `anyhow`.

## Commands

```
cargo build --workspace
cargo run -p tern-app
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```
(`just fmt|lint|test|run` wrap these. `just` is installed via `cargo install just`; toolchain pinned in `rust-toolchain.toml`; `unwrap_used` is denied outside tests.)

## Working rules

- One task at a time (S0-T01, S0-T02, ...). After each: `cargo fmt`, clippy with `-D warnings`, `cargo test`, fix issues, then commit as `S0-T01: create workspace`.
- Definition of Done (docs/README.md): fmt/clippy clean; new non-UI logic has unit tests; **never log secrets** (passwords, private keys, passphrases, decrypted data keys — use `secrecy`/`zeroize` wrappers); new keyboard actions are registered in the keymap and visible in the command palette.
- If the plan seems wrong or unclear, stop and ask rather than guess.
- Check crates.io / docs for current APIs before using a dependency (iced's API changes between releases; targeting iced 0.14).
- Commits end with the `Co-Authored-By` trailer required by the session.
