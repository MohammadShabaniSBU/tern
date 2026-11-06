# Sprint 3 — Hosts & Credentials

**Goal:** Full management of hosts, groups, identities (username + password and/or key) and SSH keys.

**Exit demo:** Create a group "Work", add a host in it, generate an Ed25519 key, attach it to an identity, assign the identity to the host, search for the host with `/`, edit it, delete it — all from the keyboard. Restart and everything is still there.

**Depends on:** Sprints 1–2.

---

### S3-T01 — Hosts list & detail view (M)
- Tree list grouped by `Group`, showing label, `user@hostname:port`, tags, last connected.
- Detail pane shows full info (secrets masked).
- Sort: by label / last connected (toggle via action).

### S3-T02 — Host form (M)
- Fields: label, hostname, port, username override, identity picker (fuzzy dropdown), group picker, jump host picker (stored now, used in Sprint 8), tags, keepalive, notes.
- Validation inline; `Ctrl+S` save, `Esc` cancel with dirty check.

**Acceptance:** Invalid port / empty hostname blocked with a clear message.

### S3-T03 — Groups (S)
- Create / rename / delete (hosts become ungrouped) / move host to group (`m` action on a host).

### S3-T04 — SSH key management (L)
- List keys: label, type, fingerprint (SHA256), created date.
- **Generate**: Ed25519 (default), RSA 4096, ECDSA P-256; optional passphrase.
- **Import from file**: OpenSSH private key (with passphrase prompt if encrypted); also PEM/PKCS#8 if `russh`'s key module supports it.
- **Paste**: import from clipboard text.
- Actions: copy public key (`y p`), export private key to file (requires master password re-entry).

**Acceptance:** Generated public key matches what `ssh-keygen -y` outputs for the exported private key.

### S3-T05 — Identities (M)
- Identity = label + username + optional password + optional key.
- Password field: reveal toggle (`Ctrl+R`), generate (`Ctrl+G`), copy with 30 s clipboard auto-clear.

### S3-T06 — Host search & quick connect picker (M)
- `/` filters the host list (label, hostname, tags, group).
- `Ctrl+Shift+O` opens a palette-style host picker (connect wired up in Sprint 4; for now shows a toast).

### S3-T07 — Duplicate & bulk tag (S)
- `y` duplicates a host ("label (copy)"); multi-select with `Space` then `T` to add tags.

### S3-T08 — Re-auth for sensitive actions (S)
- Revealing a password or exporting a private key requires the master password if the vault was unlocked more than N minutes ago (configurable).

---

**Definition of Done:** Exit demo works; repos covered by tests; no secret ever rendered unless explicitly revealed.
