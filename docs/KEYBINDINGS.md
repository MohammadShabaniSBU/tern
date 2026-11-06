# Keyboard model

## Modes

| Mode | Entered by | Who gets keys |
|---|---|---|
| Normal | `Esc` from anywhere, app start | App: navigation + single-key commands |
| Insert | Focusing a text input or a terminal tab | The widget (text field / remote shell). Only app-modifier chords and the leader key are intercepted. |
| Palette | `Ctrl+K` / `Ctrl+Shift+P` | Palette input; `Esc` closes |

The current mode is always shown in the status bar (like vim's `-- INSERT --`).

## Conflicts with the terminal

A remote shell needs `Ctrl+A`, `Ctrl+W`, `Ctrl+K`, etc. So while a terminal is focused:

- Plain `Ctrl+<key>` and `Alt+<key>` go to the remote.
- `Ctrl+Shift+<key>` is reserved for the app (same convention as most Linux terminal emulators).
- An optional **leader key** (default `Ctrl+Space`, configurable) arms a one-shot app command: `Ctrl+Space` then `n` = next tab, etc.
- `Ctrl+Shift+Esc` (configurable) leaves the terminal and returns to Normal mode.

## Default keymap

### Global (any mode except when a text field consumes the key)

| Keys | Action |
|---|---|
| `Ctrl+K` / `Ctrl+Shift+P` | Open command palette |
| `Ctrl+Shift+O` | Quick connect (fuzzy host picker) |
| `Ctrl+Shift+T` | New tab from host picker |
| `Ctrl+Shift+W` | Close current tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+Shift+1..9` | Jump to tab N |
| `Ctrl+Shift+L` | Lock vault |
| `Ctrl+,` | Settings |
| `F1` / `?` (Normal) | Keybinding cheat sheet |

### SSH session (Sprint 4; a single session until tabs arrive in Sprint 5)

| Keys | Action |
|---|---|
| `Ctrl+Shift+O`, then `user@host[:port]` | Connect without saving, or "Save as new host" |
| `y` / `Enter` | Host-key dialog, first connection: trust and save |
| `n` / `Esc` | Host-key dialog: abort |
| `Shift+R` | Host-key dialog, **changed** key: replace the stored key (`y`/`Enter` do nothing here) |
| `Enter` (Normal) | Focus the input line |
| `Esc` | Leave the input line; `Esc` again closes the session |
| `r` (Normal, disconnected) | Reconnect |
| `Ctrl+Shift+W` | Disconnect and close |

### Normal mode — lists (hosts, keys, identities, tunnels)

| Keys | Action |
|---|---|
| `j` / `k` or `↓` / `↑` | Move selection |
| `g g` / `G` | First / last item |
| `h` / `l` | Collapse / expand group, or move between panes |
| `Enter` | Connect (hosts) / open (others) |
| `/` | Filter current list |
| `a` | Add new item |
| `e` | Edit selected |
| `d d` | Delete selected (with confirm) |
| `y` | Duplicate selected |
| `1`..`4` | Switch section: Hosts, Keys, Identities, Tunnels |
| `t` (on a host) | Start/stop its tunnels |

### Forms

| Keys | Action |
|---|---|
| `Tab` / `Shift+Tab` | Next / previous field |
| `Ctrl+S` | Save |
| `Esc` | Cancel (confirm if dirty) |
| `Ctrl+G` | Generate (key / password fields) |

### Terminal (Insert mode)

| Keys | Action |
|---|---|
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | Copy / paste |
| `Shift+PageUp` / `Shift+PageDown` | Scrollback |
| `Ctrl+Shift+F` | Search scrollback |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Font size up / down / reset |
| `Ctrl+Shift+Esc` | Back to Normal mode |

All bindings are overridable in settings (stored in the `settings` table as JSON).
