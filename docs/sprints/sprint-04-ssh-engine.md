# Sprint 4 — SSH Engine

**Goal:** A robust async SSH client in `tern-ssh`, wired to iced through a session actor + subscription. Output is shown raw (proper terminal comes in Sprint 5).

**Exit demo:** Select a host, press `Enter`: host-key prompt on first connect, authenticate with key or password, run `ls` from a temporary text input, and see raw output. Disconnect cleanly.

**Depends on:** Sprints 1, 3.

---

### S4-T01 — `russh` client wrapper (L)
- `SshClient::connect(ConnectParams) -> Result<Session>` where `ConnectParams` holds host, port, username, auth methods (decrypted at connect time, held as `Secret`), keepalive, timeouts.
- Implement `russh::client::Handler` with `check_server_key` delegating to a known-hosts callback (T03).
- Open a session channel, request PTY (`xterm-256color`, initial cols/rows), request shell.

**Acceptance:** Integration test against a local OpenSSH server in Docker (`linuxserver/openssh-server` or similar), gated behind a feature/env var.

### S4-T02 — Authentication flow (M)
- Order: public key (if identity has a key) → password → keyboard-interactive.
- Encrypted private keys: decrypt with stored passphrase or prompt.
- Keyboard-interactive prompts are forwarded to the UI as `SessionEvent::AuthPrompt` and answered via `SessionCommand::AuthResponse`.
- Optional: try the system `ssh-agent` (`SSH_AUTH_SOCK`) if enabled in settings.

**Acceptance:** Each auth method tested; a wrong password yields a clear error, not a hang.

### S4-T03 — Known hosts / host key verification (M)
- On first connect: emit `HostKeyPrompt { fingerprint, key_type }`; user accepts (`y`) / rejects (`n`); accepted keys saved in `known_hosts`.
- On mismatch: **block** the connection with a prominent warning showing old vs new fingerprint; allow replacing only via an explicit action.
- Optional: also consult `~/.ssh/known_hosts` read-only.

**Acceptance:** Tests for unknown / matching / mismatched keys.

### S4-T04 — Session actor (L)
- One tokio task per session owning the `russh` handle and channel.
- `SessionCommand { Write(Bytes), Resize{cols,rows}, AuthResponse(..), HostKeyDecision(bool), Close }`.
- `SessionEvent { Connecting, HostKeyPrompt, AuthPrompt, Connected, Output(Bytes), Exited(code), Closed, Error(String) }`.
- Graceful shutdown, keepalives, and a configurable connect timeout.

**Acceptance:** Killing the remote sshd produces `Closed`/`Error`, never a panic or zombie task.

### S4-T05 — iced bridge (M)
- `SessionManager` in app state: `HashMap<SessionId, SessionHandle { tx: Sender<SessionCommand>, status }>`.
- A `Subscription` per active session (`Subscription::run_with_id` or equivalent) turning the event receiver into `Message::Session(id, SessionEvent)`.
- Batch/coalesce `Output` events (e.g. per frame) to avoid flooding `update` on large outputs.

**Acceptance:** `yes | head -c 50M` on the remote doesn't freeze the UI.

### S4-T06 — Connection UI states (M)
- Tab shows: connecting spinner → host-key modal → auth prompt modal → connected → disconnected (with `r` to reconnect).
- Errors mapped to human messages (DNS failure, refused, timeout, auth failed, host key mismatch).
- Update `last_connected_at` on success.

### S4-T07 — Quick connect string (S)
- In the host picker, typing `user@host:port` that doesn't match a saved host offers "Connect to user@host:port" and "Save as new host".

---

**Definition of Done:** Exit demo works against at least one real server and the Docker test server.
