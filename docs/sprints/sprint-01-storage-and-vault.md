# Sprint 1 — Storage & Vault

**Goal:** Persist everything in SQLite with secrets encrypted behind a master password. No UI beyond create/unlock screens.

**Exit demo:** First launch asks to create a master password; second launch asks to unlock; wrong password is rejected; a test host with a password round-trips through the DB with the password stored encrypted.

**Depends on:** Sprint 0.

---

### S1-T01 — Crypto primitives in `tern-vault` (M)
- `derive_kek(password, salt, params) -> Key` using Argon2id. Default params tuned to ~300–500 ms on your machine; store params so they can be raised later.
- `seal(key, plaintext, aad) -> Vec<u8>` and `open(key, blob, aad) -> Secret<Vec<u8>>` with XChaCha20-Poly1305; output format `version(1) || nonce(24) || ciphertext`.
- Wrap all keys/plaintexts in `secrecy::SecretBox` / `zeroize`.

**Acceptance:** Tests: round-trip; wrong key fails; tampered ciphertext fails; wrong AAD fails; version byte checked.

### S1-T02 — Envelope key management (M)
- `Vault::create(master_pw) -> (Vault, VaultMeta)`: random data key (DK), wrap with KEK.
- `Vault::unlock(master_pw, &VaultMeta) -> Result<Vault>`.
- `Vault::change_password(old, new)`: re-wrap DK only.
- `Vault` holds the DK in memory; implements `Drop` with zeroize; `Vault::lock()` drops it.

**Acceptance:** Tests for create/unlock/wrong password/change password; DK never appears in `Debug` output.

### S1-T03 — SQLite connection & migrations in `tern-store` (M)
- Open DB at `<data_dir>/tern.db` with `PRAGMA foreign_keys = ON`, `journal_mode = WAL`, `busy_timeout`.
- `rusqlite_migration` with migration `001_init.sql` creating the schema in `ARCHITECTURE.md`.
- Set file permissions to `0600` on Unix.

**Acceptance:** Fresh DB migrates; re-running is a no-op; a test applies migrations to an in-memory DB.

### S1-T04 — Repositories (L)
- `HostRepo`, `GroupRepo`, `IdentityRepo`, `KeyRepo`, `TunnelRepo`, `KnownHostRepo`, `SettingsRepo` with `list`, `get`, `insert`, `update`, `delete`.
- Secret fields go in/out through the `Vault` (`seal` on write, `open` on explicit read only — listing never decrypts).
- AAD for each secret = `"<table>:<column>:<row id>"`.

**Acceptance:** Integration tests against in-memory SQLite: CRUD for each entity; FK `ON DELETE` behaviors verified; raw DB inspection shows no plaintext password.

### S1-T05 — Store facade & threading (M)
- `Store` struct owning the `Connection` behind a dedicated blocking thread (or `tokio::task::spawn_blocking` + `Mutex<Connection>`), exposing async methods so iced `update` never blocks on disk I/O.

**Acceptance:** App can call `store.list_hosts().await` from a `Task::perform`.

### S1-T06 — Create / unlock screens (M)
- Screen 1 (no vault): master password + confirm, strength hint (`zxcvbn` optional), `Enter` submits.
- Screen 2 (vault exists): password field focused on launch, `Enter` unlocks, error message on failure, small delay after failed attempts.
- Unlock runs off the UI thread (Argon2 is slow by design) with a spinner.

**Acceptance:** Fully usable without a mouse; unlock doesn't freeze the window.

### S1-T07 — Recovery note & backup reminder (S)
- On vault creation, show a clear warning: there is no password recovery; recommend an export once hosts are added (Sprint 7).

**Acceptance:** Warning shown once and requires `Enter` to acknowledge.

---

**Definition of Done:** Exit demo works; `sqlite3 tern.db "select password_enc from identities"` shows only binary blobs.
