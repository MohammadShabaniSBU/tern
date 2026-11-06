# Sprint 7 — Import & Export

**Goal:** Move all your data between machines without any server, and bootstrap from existing OpenSSH config.

**Exit demo:** Export everything to `tern-backup.tern` with an export password; wipe the data dir; create a new vault; import the file; all hosts, keys, identities, tunnels and known hosts are back. Separately, import `~/.ssh/config` and see hosts created.

**Depends on:** Sprints 1, 3, 6.

---

### S7-T01 — Export bundle format (M)
- Versioned, documented format in `tern-io`:
  ```json
  {
    "format": "tern-backup",
    "version": 1,
    "created_at": "...",
    "kdf": { "alg": "argon2id", "m_cost": 65536, "t_cost": 3, "p_cost": 1, "salt": "<b64>" },
    "cipher": "xchacha20poly1305",
    "nonce": "<b64>",
    "payload": "<b64 ciphertext of JSON>"
  }
  ```
- Decrypted payload: `{ groups, hosts, identities, ssh_keys (with private keys), tunnels, known_hosts, settings }` with UUIDs preserved.
- Export password is **separate** from the master password (prompted, with confirm).

**Acceptance:** Format spec written to `docs/EXPORT_FORMAT.md`; round-trip test.

### S7-T02 — Export UI (S)
- Action "Export backup…": choose scope (all / selected groups), password, file path (`rfd` file dialog, plus a typed-path fallback for keyboard users).
- Requires master-password re-entry.

### S7-T03 — Import with merge strategy (L)
- Parse + decrypt bundle; show a preview: N new, M conflicting (same UUID, different content), K identical.
- Conflict strategies: skip / overwrite / keep both (new UUIDs, labels suffixed).
- Import is a single DB transaction — all or nothing.
- Re-encrypt secrets with the local vault's data key.

**Acceptance:** Importing the same file twice with "skip" changes nothing.

### S7-T04 — Plaintext JSON export (S, opt-in)
- For scripting/migration; secrets excluded by default, included only with an explicit scary confirmation.

### S7-T05 — `~/.ssh/config` import (L)
- Parser supporting `Host` (incl. multiple patterns; skip wildcards), `HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump`, `LocalForward`, `RemoteForward`, `DynamicForward`, `Include`.
- Map to hosts, keys (import referenced `IdentityFile`s), jump hosts and tunnels. Show preview with checkboxes before import.

**Acceptance:** Tests with fixture config files, including `Include` and comments.

### S7-T06 — Bulk key import from `~/.ssh` (S)
- Scan for private keys (`id_*` without `.pub`), show list, import selected (prompt for passphrases).

### S7-T07 — Export to `ssh_config` (S, optional)
- Generate an OpenSSH config snippet for selected hosts (no secrets) — handy when using plain `ssh` elsewhere.

### S7-T08 — Scheduled local backups (S, optional)
- Setting: on unlock, write an encrypted backup to a chosen folder (e.g. a synced folder of your choice), keep last N.

---

**Definition of Done:** Exit demo works; a corrupted or wrong-password bundle fails with a clear error and changes nothing.
