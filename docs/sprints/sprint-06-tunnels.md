# Sprint 6 — SSH Tunnels

**Goal:** Define, start, stop and monitor local, remote and dynamic (SOCKS5) port forwards, independently of terminal tabs.

**Exit demo:** Define a local tunnel `127.0.0.1:5433 → db.internal:5432` via a bastion and connect `psql` to `localhost:5433`; start a SOCKS tunnel on `1080` and browse through it with `curl --socks5-hostname`; stop both from the Tunnels screen.

**Depends on:** Sprint 4.

---

### S6-T01 — Tunnel CRUD (M)
- Tunnels screen (section `4`): list with kind, `bind → target`, host, status (stopped / starting / active / error), bytes transferred.
- Form: label, host, kind, bind addr/port, target host/port (hidden for dynamic), auto-start.
- Validation: port ranges; warn when binding to non-loopback addresses.

### S6-T02 — Connection sharing (M)
- A `ConnectionPool` in `tern-ssh`: one authenticated SSH connection per host, shared by terminal sessions and tunnels (ref-counted; closes when unused).

**Acceptance:** Opening a terminal and a tunnel to the same host authenticates only once.

### S6-T03 — Local forwarding `-L` (M)
- Bind a `TcpListener`; for each accepted socket open a `direct-tcpip` channel and copy bidirectionally (`tokio::io::copy_bidirectional`-style over channel streams).

**Acceptance:** Concurrent connections work; closing the listener closes in-flight connections.

### S6-T04 — Remote forwarding `-R` (M)
- Send `tcpip-forward` global request; handle `server_channel_open_forwarded_tcpip` in the client handler and connect to the local target.
- Clear error when the server disallows it (`GatewayPorts`/`AllowTcpForwarding`).

### S6-T05 — Dynamic forwarding `-D` / SOCKS5 (L)
- Minimal SOCKS5 server (no-auth; CONNECT command; IPv4/IPv6/domain address types) — hand-written or a small crate.
- Each CONNECT opens a `direct-tcpip` channel to the requested destination (remote DNS resolution).

**Acceptance:** `curl --socks5-hostname 127.0.0.1:1080 https://example.com` works.

### S6-T06 — Tunnel manager actor & UI bridge (M)
- Same actor/subscription pattern as sessions: `TunnelCommand { Start, Stop }`, `TunnelEvent { Active, Error, Stats { conns, bytes } }`.
- Auto-reconnect with backoff if the underlying SSH connection drops (configurable).

### S6-T07 — Keyboard UX & auto-start (S)
- `t` on a host toggles all its tunnels; `Enter` on a tunnel toggles it; palette actions "Start tunnel: X".
- Tunnels with `auto_start` start right after unlock.
- Status bar shows count of active tunnels.

---

**Definition of Done:** Exit demo works; tunnels survive switching/closing terminal tabs; locking the vault stops all tunnels.
