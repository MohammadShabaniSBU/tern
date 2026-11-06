# Backlog (post-v1)

Ideas ranked roughly by value. Each can become its own sprint.

| ID | Item | Notes |
|---|---|---|
| B-01 | **SFTP browser** | `russh-sftp`; dual-pane keyboard file manager (`j/k`, `Enter` to descend, `y/p` to copy, upload/download queue). Reuses the connection pool. |
| B-02 | **Snippets** | Saved commands with variables (`{{port}}`), run in current tab via palette; stored in a `snippets` table. |
| B-03 | **Split panes** | Horizontal/vertical splits inside a tab (`Ctrl+Shift+\`, `Ctrl+Shift+-`), move focus with leader + `h/j/k/l`. |
| B-04 | **Broadcast input** | Type into several sessions at once. |
| B-05 | **Local shell tabs** | `portable-pty` backend for the same terminal widget. |
| B-06 | **Agent forwarding & built-in agent** | Expose tern's keys as an ssh-agent socket so CLI `ssh`/`git` can use them while the vault is unlocked. |
| B-07 | **Session logging** | Optional per-host recording of terminal output to file. |
| B-08 | **Port knocking / pre-connect commands** | Per-host hooks. |
| B-09 | **Hardware keys** | FIDO2 `sk-ssh-ed25519` keys, if/when supported by `russh`. |
| B-10 | **Mosh** | Large effort; probably not worth it. |
| B-11 | **Peer-to-peer sync without a server** | e.g. sync the encrypted backup file through a folder you already sync, with merge on unlock (builds on S7-T03 merge logic). |
| B-12 | **Theme editor & import of iTerm/Alacritty color schemes** | |
