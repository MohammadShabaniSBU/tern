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
| `Ctrl+Shift+1..9` (also `Ctrl+1..9` inside a terminal) | Jump to tab N |
| `Ctrl+Shift+R` / `Ctrl+Shift+D` | Rename / duplicate tab |
| `Ctrl+Shift+L` | Lock vault |
| `Ctrl+,` | Settings |
| `F1` / `?` (Normal) | Keybinding cheat sheet |

### SSH session

| Keys | Action |
|---|---|
| `Ctrl+Shift+O`, then `user@host[:port]` | Connect without saving, or "Save as new host" |
| `y` / `Enter` | Host-key dialog, first connection: trust and save |
| `n` / `Esc` | Host-key dialog: abort |
| `Shift+R` | Host-key dialog, **changed** key: replace the stored key (`y`/`Enter` do nothing here) |
| `Enter` (Normal) | Send keys to the remote again (Insert mode) |
| `Ctrl+Shift+Esc` | Leave the terminal (Normal mode); `Esc` then shows the host list. `Esc` while connecting cancels |
| `r` (Normal, disconnected) | Reconnect |
| `Ctrl+Shift+W` | Close the tab (asks first while connected) |

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
| `t` (on a host) | Start/stop all of its tunnels (on a tunnel: toggle it) |
| `Enter` (on a tunnel) | Start/stop the tunnel |

### Forms

| Keys | Action |
|---|---|
| `Tab` / `Shift+Tab` | Next / previous field |
| `Ctrl+S` | Save |
| `Esc` | Cancel (confirm if dirty) |
| `Ctrl+G` | Generate (key / password fields) |

### Terminal (Insert mode)

Everything except `Ctrl+Shift+<key>`, the leader, `Ctrl+Tab`, `Ctrl+1..9/0/=/-` and `Shift+PageUp/Down` goes to the remote.

| Keys | Action |
|---|---|
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | Copy selection / paste (multi-line paste asks first; `term.paste_warn` toggles) |
| Mouse drag / double / triple click | Select text / word / line |
| `Shift+PageUp` / `Shift+PageDown`, mouse wheel | Scrollback (sent through to full-screen apps) |
| `Ctrl+Shift+F` | Search scrollback; `Enter` next, `Ctrl+Shift+Alt+G` previous, `Esc` closes |
| `Ctrl+Shift+G` | Next search match |
| `Ctrl+Shift+Space` | Keyboard copy mode |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Font size up / down / reset |
| `Ctrl+Shift+Esc` | Back to Normal mode |

Copy mode: `h j k l`/arrows move, `w`/`b` word, `0`/`$` line ends, `g`/`G` top/bottom, `Ctrl+U`/`Ctrl+D` half page, `v` start/stop selection, `y` copy (the current line if nothing selected), `n`/`N` next/previous search match, `/` search, `Esc`/`q` exit.

Palette-only actions: cycle color scheme (dark / light / solarized), cycle cursor style, toggle visual bell. Further preferences live in the `settings` table: `term.font_size`, `term.scrollback` (default 10000), `term.font` (system family), `term.scheme`, `term.cursor`.

All bindings are overridable in **Settings → Keybindings** (stored in the `settings` table as JSON under `keymap`). Rebinding an action replaces all of its default keys with the one key pressed; multi-key sequences (`g g`) can still be written by hand in that JSON. Inside the settings screen: `Ctrl+Tab` / `Ctrl+Shift+Tab` switch section, `Ctrl+S` saves, `Esc` closes; in Keybindings `↑/↓` select, `Enter` rebind, `Del` unbind, `r` reset.

Other settings keys: `lock.idle_minutes` (default 15, 0 = off), `lock.keep_sessions` (`true` keeps terminals and tunnels alive while locked).
