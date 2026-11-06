# tern

Personal, local-first, keyboard-first SSH client (Termius-style) in Rust with the `iced` GUI.
No server, no account, no sync; all data in one SQLite file, secrets encrypted at rest behind a master password.

Full plan lives in `docs/` — read `docs/README.md`, `docs/ARCHITECTURE.md`, `docs/KEYBINDINGS.md`, then the current sprint file in `docs/sprints/`.

## Current status

Working through **Sprint 0 only** (`docs/sprints/sprint-00-foundation.md`). Do not write code for later sprints.
CI (S0-T03) was explicitly skipped by the user.

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
