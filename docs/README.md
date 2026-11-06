# tern — a local-first, keyboard-first SSH client

> Working name: **tern** (rename freely). A personal Termius-style SSH client written in Rust with the `iced` GUI framework.

## Goals

- Store **hosts**, **SSH keys**, **passwords/identities** and **tunnels** locally.
- **No server, no account, no sync service.** All data lives in one SQLite file on disk.
- Secrets are **encrypted at rest** behind a master password.
- Full **import / export** (encrypted backup bundle + import from `~/.ssh/config` and key files).
- **SSH tunnels**: local (`-L`), remote (`-R`) and dynamic SOCKS5 (`-D`).
- A built-in **terminal** with tabs.
- **Keyboard first**: every action reachable without a mouse; command palette; vim-style navigation.

## Non-goals (for v1)

- Cloud sync, team sharing, mobile apps.
- Telnet / serial / mosh.
- Being a general-purpose local terminal emulator (local shell tabs are a backlog item).

## Document map

| File | Purpose |
|---|---|
| `ARCHITECTURE.md` | Crate layout, tech choices, data model, key design decisions |
| `KEYBINDINGS.md` | The keyboard model and default keymap |
| `sprints/sprint-00-foundation.md` | Workspace, tooling, iced skeleton |
| `sprints/sprint-01-storage-and-vault.md` | SQLite schema, migrations, encryption, master password |
| `sprints/sprint-02-keyboard-ui-shell.md` | App shell, focus model, command palette, keymap engine |
| `sprints/sprint-03-hosts-and-credentials.md` | Host / group / identity / key management UI |
| `sprints/sprint-04-ssh-engine.md` | `russh` client, auth, known-hosts, async bridge to iced |
| `sprints/sprint-05-terminal.md` | Terminal emulation + rendering widget, tabs |
| `sprints/sprint-06-tunnels.md` | Local / remote / dynamic port forwarding |
| `sprints/sprint-07-import-export.md` | Encrypted backup, ssh_config import, key import |
| `sprints/sprint-08-hardening-and-release.md` | Auto-lock, jump hosts, polish, packaging |
| `sprints/backlog.md` | SFTP, snippets, local shell, etc. |

## Sprint cadence

These are scoped for a **solo developer working evenings/weekends**: roughly 1–2 weeks per sprint. Each task has an ID (`S<sprint>-T<nn>`), a size estimate (S ≈ ≤2h, M ≈ half a day, L ≈ 1–2 days) and acceptance criteria. Don't start a sprint until the previous sprint's **Definition of Done** is met — later sprints depend heavily on the storage layer and the async bridge.

## Global Definition of Done (applies to every task)

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` pass.
- New logic in non-UI crates has unit tests; `cargo test --workspace` passes.
- No secret (password, private key, passphrase, decrypted data key) is ever written to logs.
- Any new keyboard action is registered in the keymap and visible in the command palette.
