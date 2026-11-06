# Sprint 8 — Hardening, Jump Hosts & Release

**Goal:** Make tern safe to leave running all day and easy to install: auto-lock, jump hosts, settings, robustness, packaging.

**Exit demo:** Connect to a private host through a bastion; walk away and the vault auto-locks (sessions stay or close per setting); install tern from a built package on a clean machine.

**Depends on:** Sprints 1–7.

---

### S8-T01 — Jump hosts / ProxyJump (L)
- When a host has `jump_host_id`, connect to the jump host first, open `direct-tcpip` to the target, and run the SSH handshake over that channel (`russh` supports connecting over any `AsyncRead + AsyncWrite` stream).
- Support chains (A → B → C) with cycle detection.
- Host keys verified for every hop.

**Acceptance:** Works for terminals and tunnels; tested against two Docker sshd containers on a private network.

### S8-T02 — Auto-lock (M)
- Lock after N minutes idle (keyboard/mouse inactivity) and optionally on OS sleep/screen lock where detectable.
- On lock: zeroize data key, clear decrypted caches, clear clipboard if it holds a secret.
- Setting: keep open sessions & tunnels running while locked vs. terminate them.

### S8-T03 — Secret hygiene audit (M)
- Grep/clippy review: every secret type is `Secret`/`Zeroizing`; no `Debug`/`Display` leaks; logs scrubbed.
- Optional: `memsec`/`mlock` for the data key on platforms that support it.
- Document threat model in `docs/SECURITY.md` (protects against: stolen disk / DB file; does not protect against: malware running as your user).

### S8-T04 — Settings screen (M)
- Sections: General (auto-lock, data dir, backups), Terminal, Keybindings (rebind with "press new key", conflict warnings, reset to default), Security (change master password, Argon2 cost), About.

### S8-T05 — Reliability pass (M)
- Reconnect flow for dropped sessions (`r` in a disconnected tab).
- Handle DB lock/corruption errors gracefully; run `PRAGMA integrity_check` on startup in debug builds.
- Panic hook that logs and shows an error instead of vanishing.

### S8-T06 — Performance pass (S)
- Profile startup (target < 1 s to unlock screen), host list with 1,000 hosts, terminal throughput.

### S8-T07 — Packaging (M)
- `cargo-dist` or `cargo-bundle` / `cargo-packager` for: Linux (AppImage / .deb), macOS (.app / .dmg), Windows (.msi / zip).
- App icon, version shown in About.

### S8-T08 — User docs (S)
- README: install, first run, keybindings, backup advice, export format reference.

---

**Definition of Done:** v1.0 tagged; you've used it as your only SSH client for a week without reaching for another tool.
