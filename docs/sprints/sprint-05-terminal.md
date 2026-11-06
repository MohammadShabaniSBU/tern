# Sprint 5 — Terminal

**Goal:** A real terminal widget (colors, cursor, scrollback, selection) running remote shells in tabs. After this sprint the app is genuinely usable day to day.

**Exit demo:** Connect to two servers in two tabs, run `htop` and `vim` in them, resize the window, scroll back, copy text with `Ctrl+Shift+C`, switch tabs with `Ctrl+Tab`.

**Depends on:** Sprint 4.

---

### S5-T01 — Emulation core in `tern-term` (L)
- Wrap `alacritty_terminal::Term` with a no-op/forwarding `EventListener`; feed bytes through the ANSI `Processor`.
- API: `feed(&[u8])`, `resize(cols, rows)`, `renderable_content()`, `scroll(delta)`, selection helpers.
- Handle terminal → host replies (e.g. device attribute queries) by emitting bytes to write back to the session.

**Acceptance:** Unit tests feeding known escape sequences and asserting grid contents.

### S5-T02 — Rendering widget (L)
- Custom iced widget (or `canvas::Program`) drawing cell backgrounds, glyphs, cursor (block/underline/bar, blinking), bold/italic/underline/inverse, 256 colors + truecolor.
- Font metrics from the bundled monospace font; compute cols/rows from widget bounds.
- Cache: only redraw when the term has damage; use `canvas::Cache` per frame.

**Acceptance:** Smooth `htop`; `vim` renders correctly; 60 fps scroll on a typical machine.

### S5-T03 — Input encoding (M)
- Map iced key events → bytes: printable text, Enter, Backspace (`DEL`), Tab, arrows (respect application cursor mode), Home/End, PgUp/PgDn, F1–F12, `Ctrl+<letter>`, `Alt` as ESC prefix.
- Bracketed paste support.
- Respect the keymap pass-through rules (app modifier chords never reach the remote).

**Acceptance:** `showkey -a` / `cat -v` on the remote shows the expected sequences.

### S5-T04 — Resize (S)
- On widget size change → `term.resize` + `SessionCommand::Resize` (debounced ~50 ms).

### S5-T05 — Scrollback & search (M)
- Configurable scrollback (default 10,000 lines), `Shift+PgUp/PgDn`, mouse wheel.
- `Ctrl+Shift+F`: incremental search over scrollback with highlight, `n`/`N` to jump.

### S5-T06 — Selection & clipboard (M)
- Mouse selection (still nice to have) + **keyboard copy mode** (`Ctrl+Shift+Space`): vim-like cursor movement, `v` to select, `y` to copy — keyboard-first.
- `Ctrl+Shift+V` paste; warn before pasting multi-line text (configurable).

### S5-T07 — Tabs (M)
- Tab bar with host label + status dot; `Ctrl+Shift+T` new, `Ctrl+Shift+W` close (confirm if session alive), `Ctrl+Tab` cycle, `Ctrl+Shift+1..9` jump, rename tab.
- Optional: duplicate session (`Ctrl+Shift+D`).

### S5-T08 — Terminal settings (S)
- Font family/size, color scheme (ship 2–3 themes), cursor style, scrollback size, bell (visual).

---

**Definition of Done:** You can do a normal day's work (vim, tmux, htop, git) through tern without obvious rendering bugs.
