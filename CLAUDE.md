# tern

Personal, local-first, keyboard-first SSH client (Termius-style) in Rust with the `iced` GUI.
No server, no account, no sync; all data in one SQLite file, secrets encrypted at rest behind a master password.

Full plan lives in `docs/` — read `docs/README.md`, `docs/ARCHITECTURE.md`, `docs/KEYBINDINGS.md`, then the current sprint file in `docs/sprints/`.

## Current status

Identities were removed (after Sprint 9): a host carries `username` + `auth` (`HostAuth::{Ask, Password, Key(KeyId)}`). The password is encrypted in `hosts.password_enc` (AAD `hosts:password_enc:<host id>`; `Store::set_host_password/get_host_password`). Migration `002_host_auth.sql` folds old identities into hosts; the emptied `identities` table and `hosts.identity_id` stay unused (SQLite cannot drop an FK column without rebuilding `hosts`, which would cascade-delete tunnels), and a migrated password keeps its old AAD in `hosts.password_aad` until it is next saved. Sections are now Hosts/Keys/Tunnels (`1`..`3`); `Store::delete_key` refuses while hosts use the key. Old backups still import (`fold_legacy_identities` in `tern-io/bundle.rs`). Not hand-tested in the GUI.

Sprint 0–8 are done (S0-T03 CI was skipped by the user; no macOS/Windows packaging by request). Sprint 8 was implemented without GUI hand-testing: the settings screen, auto-lock and rebind flows are covered by unit tests only. Do not write code for later work until the user asks (`docs/sprints/backlog.md`).

Sprint 8: jump hosts (`ConnectParams.jump` chain, `connect_chain` in `tern-ssh/session.rs`, `resolve_host` in `sessions.rs` with cycle detection; live test `terminal_through_a_jump_host_verifies_every_hop` needs `TERN_SSH_JUMP_TARGET=host:port` as seen from the bastion, which needs `AllowTcpForwarding yes`), `autolock.rs` (settings `lock.idle_minutes|keep_sessions`; wall-clock gap between ticks = sleep detected), `settings.rs` + `app/settings_ops.rs` (settings overlay; keymap overrides in `State.keymap_overrides`; `Store::change_security` re-wraps the data key for password/KDF-cost changes), `crash.rs` (panic hook → `<data>/crash-*.log`), `docs/SECURITY.md`, `docs/PERFORMANCE.md`, `packaging/` (tarball verified; `.deb` metadata verified with cargo-deb; AppImage script untested). Binary is now named `tern` (package still `tern-app`). The optional `mlock` was skipped because `unsafe` is forbidden.

Sprint 7 (import/export): `tern-io` has `bundle.rs` (encrypted `tern-backup` v1, spec in `docs/EXPORT_FORMAT.md`), `merge.rs` (`snapshot`/`plan`/`apply` — atomic, strategies skip/overwrite/keep-both), `plain.rs`, `ssh_config.rs` (parser with Include, mapper, writer), `keyscan.rs`. `tern-app`: `io_form.rs` (one dialog for all flows, selection by typed text like `1,3,5-7`) and `app/io_ops.rs` (the `State` logic). Actions `io.*` are palette-only; backup/plaintext export require master-password re-entry. Scheduled backups: settings `backup.dir|keep|enabled`; after unlock the app asks for the backup password (never stored) and writes `tern-backup-<stamp>.tern`. No `rfd`: paths are typed. Not hand-tested in the GUI yet.

Sprint 6 (tunnels): `tern-ssh` has `pool.rs` (`ConnectionPool`, weak refs keyed by host id; terminals use `spawn_pooled`, tunnels `get_or_connect` headless — unknown host key or interactive auth makes the tunnel fail with "connect once from a terminal first"), `tunnel.rs` (-L/-R/-D over a `SharedConn`), `socks5.rs`, `manager.rs` (`TunnelCommand`/`TunnelEvent`, supervised with backoff; setting `tunnel.reconnect`=`false` disables retries). `tern-app`: `tunnels_view.rs`, `tunnel_form.rs`, `tunnel_run.rs` (status + subscription); lock sends `StopAll`; `auto_start` tunnels start after unlock. Live forwarding tests need a server with `AllowTcpForwarding yes` (the stock linuxserver image has it off: `docker exec <c> sed -i 's/^AllowTcpForwarding no/AllowTcpForwarding yes/' /config/sshd/sshd_config`, plus `MaxStartups 200:30:300` and `PerSourcePenalties no` to avoid flaky drops, then restart).

Sprint 4: `tern-ssh` has `session.rs` (`spawn(ConnectParams, Arc<dyn KnownHostsStore>)` → `SessionHandle` + event `Receiver`; auth key → password → keyboard-interactive; output coalesced ~16 ms/64 KiB) and `known_hosts.rs`. `tern-app/src/sessions.rs` holds `SessionView` (one per tab, owns the emulator), credential `resolve`, `AppKnownHosts`, the iced subscription, tab bar and tab view. Live tests: `crates/tern-ssh/tests/live.rs`, skipped unless `TERN_SSH_TEST_ADDR=host:port TERN_SSH_TEST_USER=… TERN_SSH_TEST_PASSWORD=…` are set (e.g. `docker run -d -p 127.0.0.1:2222:2222 -e PASSWORD_ACCESS=true -e USER_NAME=tern -e USER_PASSWORD=ternpass lscr.io/linuxserver/openssh-server`). russh 0.65's `check_server_key` takes `russh::keys::PublicKeyOrCertificate`.

Sprint 5 (terminal): `tern-term` = `emu.rs` (`Emulator` over `alacritty_terminal` 0.26: feed/resize/scroll/selection/search/copy mode, plain `Snapshot` types, no alacritty types leak), `input.rs` (xterm key + bracketed-paste encoding), `theme.rs` (3 schemes), `widget.rs` (iced canvas `TerminalView`; cell metrics hard-coded for JetBrains Mono; reports `TermEvent`s, grid size detected on `RedrawRequested`). `tern-app`: `State.tabs`/`active` (`None` = host list), `Message::Key(Chord, RawKey)` — in a connected terminal (`terminal_focused`) unmatched keys are encoded to the remote; `term_settings.rs` (`term.*` settings). Mouse reporting to remote apps is not implemented (wheel in alt-screen sends arrow keys).

Sprint 2 app modules (`crates/tern-app/src`): `action.rs` (one-table action registry; add an action there), `keymap.rs` (chords, sequences, mode-aware matcher, overrides from `settings`), `list.rs` (keyboard list model + view), `palette.rs`, `modal.rs` (stack; also hosts the cheat sheet), `cheatsheet.rs`, `toast.rs`. `State::dispatch` is the single entry for keymap and palette. List data is still dummy (`list::dummy_items`) until Sprint 3.
- `tern-vault`: Argon2id KEK (128 MiB/8 passes, ~420 ms), XChaCha20-Poly1305 `seal/open`, `Vault` envelope (create/unlock/change_password/lock).
- `tern-store`: migrations (`migrations/001_init.sql`), repos (secrets only via explicit `Vault` methods; AAD `"<table>:<column>:<row id>"`), `Store` = cloneable async handle over one worker thread that owns the Connection and the unlocked Vault.
- `tern-app`: `app.rs` (Loading/Create/Unlock/Recovery/Main screens), `auth.rs` (forms, `Sensitive` password wrapper), `keymap.rs` (global key subscription; future keymap engine plugs in here), `logging.rs`, `paths.rs`.
- Pinned `rusqlite` 0.40 because `rusqlite_migration` 2.6 requires it (0.41 exists).
- Password text lives only in `Sensitive` (zeroized, redacted Debug); never put raw password `String`s in Messages/State.
- Don't screenshot the whole desktop to check the UI (it captures other windows); crop to the tern window or ask the user.

## Crate layout (`crates/`)

| Crate | Role |
|---|---|
| `tern-core` | Domain types (Host, HostAuth, Group, SshKeyMeta, Tunnel) + validation. No I/O. |
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
