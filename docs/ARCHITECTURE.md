# Architecture

## Tech stack

| Concern | Choice | Notes |
|---|---|---|
| GUI | `iced` (latest stable, with `tokio` + `canvas` + `advanced` features) | Elm architecture: `State`, `Message`, `update`, `view`, `subscription`. Check the release notes when you start: `Task`/`Subscription` APIs shifted between 0.12 → 0.13 → later. |
| Async runtime | `tokio` | Shared with iced via its `tokio` feature. |
| SSH | `russh` (incl. its key-handling module, formerly `russh-keys`) | Pure Rust, async, supports password/pubkey/keyboard-interactive auth and `direct-tcpip` / `tcpip-forward` channels needed for tunnels. |
| Terminal emulation | `alacritty_terminal` | Battle-tested VT parser + grid. We render the grid ourselves in an iced `canvas`. (Alternative: the `vt100` crate — simpler, fewer features. `iced_term` is worth reading for reference.) |
| Storage | `rusqlite` (feature `bundled`) + `rusqlite_migration` | Single file DB, no external SQLite needed. `rusqlite_migration` 2.6 requires rusqlite 0.40, so pin 0.40 until it supports 0.41. |
| KDF | `argon2` (Argon2id) | Derives the key-encryption key from the master password. |
| Encryption | `chacha20poly1305` (XChaCha20-Poly1305) | AEAD for secret fields and export bundles. |
| Secret hygiene | `zeroize`, `secrecy` | Wrap passwords / keys so they're wiped on drop and never `Debug`-printed. |
| Serialization | `serde`, `serde_json` | Export format, settings. |
| IDs | `uuid` (v4 / v7) | Stable IDs survive export → import. |
| Errors | `thiserror` (libraries), `anyhow` (binary) | |
| Logging | `tracing`, `tracing-subscriber` | |
| Paths | `directories` | Platform data dir for the DB file. |
| Fuzzy search | `nucleo-matcher` or `fuzzy-matcher` | Command palette + host search. |
| Clipboard | `arboard` (or iced's clipboard API) | With timed clear for copied passwords. |

## Workspace layout

```
tern/
├── Cargo.toml              # [workspace]
├── crates/
│   ├── tern-core/          # Domain types (Host, Identity, SshKey, Tunnel, Group), no I/O
│   ├── tern-vault/         # KDF, AEAD, data-key wrapping, Secret<T> helpers
│   ├── tern-store/         # SQLite schema, migrations, repositories
│   ├── tern-ssh/           # russh client wrapper, auth, known-hosts, tunnels
│   ├── tern-term/          # alacritty_terminal adapter + iced terminal widget
│   ├── tern-io/            # Import/export: backup bundle, ssh_config parser, key import
│   └── tern-app/           # iced application (binary): screens, keymap, palette
└── docs/                   # these planning files
```

Rule: **only `tern-app` depends on iced** (except `tern-term`, which exposes the widget). Everything else is testable without a GUI.

## Data model (SQLite)

```sql
-- One row. Holds what is needed to unlock the vault.
vault_meta (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  kdf_params TEXT NOT NULL,        -- JSON: { m_cost, t_cost, p_cost }
  kdf_salt BLOB NOT NULL,
  wrapped_data_key BLOB NOT NULL,  -- data key sealed with the KEK (nonce is embedded in the blob)
  created_at TEXT NOT NULL
)

groups (id TEXT PK, name TEXT, parent_id TEXT NULL REFERENCES groups(id) ON DELETE SET NULL, sort_order INT)

ssh_keys (
  id TEXT PK, label TEXT, key_type TEXT, public_key TEXT, fingerprint TEXT,
  private_key_enc BLOB,             -- encrypted OpenSSH PEM
  passphrase_enc BLOB NULL,
  created_at TEXT, updated_at TEXT
)

identities (
  id TEXT PK, label TEXT, username TEXT,
  password_enc BLOB NULL,
  ssh_key_id TEXT NULL REFERENCES ssh_keys(id) ON DELETE SET NULL,
  created_at TEXT, updated_at TEXT
)

hosts (
  id TEXT PK, label TEXT, hostname TEXT, port INT DEFAULT 22,
  username TEXT NULL,              -- overrides identity username
  identity_id TEXT NULL REFERENCES identities(id) ON DELETE SET NULL,
  group_id TEXT NULL REFERENCES groups(id) ON DELETE SET NULL,
  jump_host_id TEXT NULL REFERENCES hosts(id) ON DELETE SET NULL,
  tags TEXT,                       -- JSON array
  notes TEXT,
  keepalive_secs INT NULL,
  last_connected_at TEXT NULL,
  created_at TEXT, updated_at TEXT
)

known_hosts (
  id TEXT PK, host TEXT, port INT, key_type TEXT, fingerprint TEXT,
  public_key TEXT, first_seen_at TEXT, UNIQUE(host, port, key_type)
)

tunnels (
  id TEXT PK, label TEXT, host_id TEXT REFERENCES hosts(id) ON DELETE CASCADE,
  kind TEXT CHECK (kind IN ('local','remote','dynamic')),
  bind_addr TEXT DEFAULT '127.0.0.1', bind_port INT,
  target_host TEXT NULL, target_port INT NULL,   -- NULL for dynamic
  auto_start INT DEFAULT 0,
  created_at TEXT, updated_at TEXT
)

settings (key TEXT PK, value TEXT)   -- JSON values; keymap overrides live here
```

## Key design decisions

### ADR-1: Field-level encryption with an envelope key (not SQLCipher)

- On vault creation: generate a random 256-bit **data key (DK)**. Derive a **key-encryption key (KEK)** from the master password with Argon2id + random salt. Store `AEAD(KEK, DK)` in `vault_meta`.
- Every secret column (`*_enc`) is `nonce || ciphertext` encrypted with DK, with the row ID as associated data (prevents swapping ciphertexts between rows).
- Changing the master password only re-wraps DK; no data re-encryption.
- **Trade-off:** hostnames, labels and usernames are stored in plaintext so search and listing work without decrypting everything. For a personal machine this is acceptable. If you later want *everything* encrypted, swap `rusqlite`'s `bundled` feature for `bundled-sqlcipher` and keep the same repository API — note this in a follow-up ADR.

### ADR-2: SSH sessions live outside iced state

- iced state must be cheap to clone/inspect and `update` must not block. Each SSH session runs in its own tokio task (a **session actor**).
- The UI talks to it through an `mpsc::Sender<SessionCommand>` (write bytes, resize, close, open tunnel).
- The actor emits `SessionEvent`s (output bytes, connected, auth prompt, host-key prompt, closed, error) back to iced via a `Subscription` that wraps an `mpsc::Receiver` stream.
- Same pattern for the tunnel manager.

### ADR-3: Keyboard-first, with an explicit focus model

- The app has **modes**: `Normal` (navigation, single-key commands like `j`/`k`), `Insert` (a text field or the terminal owns the keyboard), `Palette` (command palette open).
- When a terminal tab is focused, **all keys go to the remote** except bindings using the app modifier (`Ctrl+Shift` by default) or the leader key. See `KEYBINDINGS.md`.
- Every user-facing action is a named `Action` enum variant. Keybindings and the command palette both map to `Action`s, so nothing is reachable only by mouse.

### ADR-4: Stable UUIDs everywhere

All entities use UUIDs generated client-side so an export from one machine can be imported on another and merged without ID collisions.
