# Sprint 2 — Keyboard-first UI Shell

**Goal:** The navigation framework every later screen plugs into: modes, focus, action registry, keymap, command palette, list navigation, toasts and modals.

**Exit demo:** With the mouse unplugged you can switch sections with `1`–`4`, move through a dummy list with `j`/`k`, open the palette with `Ctrl+K`, fuzzy-search "lock", hit `Enter`, and the vault locks.

**Depends on:** Sprint 1.

---

### S2-T01 — `Action` registry (M)
- `enum Action` covering every user command (navigation, CRUD, connect, tabs, lock, settings, ...).
- `ActionMeta { id: &'static str, title, category, default_keys, available_in: ModeMask }`.
- `fn dispatch(state, action) -> Task<Message>` is the single entry point used by both keymap and palette.

**Acceptance:** Adding an action requires touching one place; unit test checks all IDs are unique.

### S2-T02 — Mode & focus model (M)
- `enum Mode { Normal, Insert, Palette, Modal }` in app state; status bar displays it.
- Track focused pane (sidebar / list / detail / terminal tab) and focused widget id (use iced `widget::Id` + `focus`/`focus_next`).
- `Esc` always moves one step "outwards" (close modal → leave insert → clear filter).

**Acceptance:** Focus is never lost (always something highlighted); `Esc` behavior matches spec.

### S2-T03 — Keymap engine (L)
- Parse key chords from strings (`"ctrl+shift+t"`, `"g g"` sequences, leader `"<leader> n"`).
- Matcher with sequence timeout (e.g. 800 ms), mode-awareness, and the terminal pass-through rules from `KEYBINDINGS.md`.
- Load defaults, then user overrides from `settings` table. Detect conflicts and report them in the log + settings screen.

**Acceptance:** Unit tests for chord parsing, sequences, timeout, mode filtering, and that plain `Ctrl+W` is *not* intercepted in Insert mode.

### S2-T04 — Command palette (L)
- Overlay with input + result list; fuzzy match over action titles **and** hosts ("Connect: prod-db").
- Shows each action's keybinding on the right (teaches shortcuts).
- `↑`/`↓` or `Ctrl+N`/`Ctrl+P` to move, `Enter` to run, `Esc` to close; recently used actions ranked higher.

**Acceptance:** Opens in <50 ms with 500 hosts; fully keyboard operated.

### S2-T05 — Reusable keyboard list widget (M)
- Generic list view: selection index, `j/k`, `gg/G`, `PageUp/Down`, `/` filter, scroll-into-view on selection change.
- Supports grouped/tree items (expand/collapse with `h/l`).

**Acceptance:** Used by a dummy list of 1,000 items; selection always visible.

### S2-T06 — Modal & confirm dialogs (S)
- Generic modal stack; confirm dialog with `y`/`n`/`Enter`/`Esc`.

**Acceptance:** `d d` on a dummy item shows a confirm modal that's keyboard-operable.

### S2-T07 — Toasts / status messages (S)
- Non-blocking notifications (success, error) in the status bar area, auto-dismiss.

### S2-T08 — Layout & theme (M)
- Sidebar (sections), main list, detail pane, status bar. Dark + light theme, monospace font bundled.
- Visible focus rings on every focusable element — important for keyboard-first.

### S2-T09 — Keybinding cheat sheet (S)
- `F1`/`?` opens an overlay generated from the action registry, grouped by category, filtered by current mode.

---

**Definition of Done:** Exit demo works without a mouse; every action from this sprint appears in the palette and cheat sheet.
