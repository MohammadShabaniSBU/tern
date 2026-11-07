//! Keymap engine (S2-T03): chord parsing, key sequences, mode-aware matching.
//!
//! Pure and window-free so it is unit-testable. `State` owns a [`Keymap`] (defaults from the
//! action registry plus user overrides from the `settings` table) and a [`Matcher`] and feeds
//! every key press through [`Matcher::feed`]; the resulting [`Action`] goes to `State::dispatch`,
//! the same entry point the command palette uses. Rules follow `docs/KEYBINDINGS.md`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::keyboard::key::Named;
use iced::keyboard::{self, Key, Modifiers};
use iced::{Event, Subscription, event};

use crate::action::{Action, ModeMask};
use crate::app::Message;

/// How long a half-typed sequence (`g` of `g g`) stays armed.
pub const SEQUENCE_TIMEOUT: Duration = Duration::from_millis(800);

/// The default leader chord (`docs/KEYBINDINGS.md`).
pub const DEFAULT_LEADER: &str = "ctrl+space";

/// The key part of a chord, normalised so parsed strings and real events compare equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyId {
    /// A lower-cased character.
    Char(char),
    Named(Named),
}

/// One key press with modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub mods: Modifiers,
    pub key: KeyId,
}

const MOD_MASK: Modifiers = Modifiers::CTRL
    .union(Modifiers::ALT)
    .union(Modifiers::SHIFT)
    .union(Modifiers::LOGO);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    UnknownKey(String),
    UnknownModifier(String),
    NoLeader,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty key sequence"),
            ParseError::UnknownKey(k) => write!(f, "unknown key '{k}'"),
            ParseError::UnknownModifier(m) => write!(f, "unknown modifier '{m}'"),
            ParseError::NoLeader => write!(f, "<leader> is not configured"),
        }
    }
}

impl Chord {
    /// Build a chord from a real key event. `None` for keys we never bind (bare modifiers, ...).
    pub fn from_event(key: &Key, modifiers: Modifiers) -> Option<Chord> {
        let mut mods = modifiers & MOD_MASK;
        let key = match key.as_ref() {
            Key::Character(c) => {
                let mut chars = c.chars();
                let ch = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                let lower = ch.to_lowercase().next().unwrap_or(ch);
                // Shift on a symbol ("?", "!") is already part of the character.
                if !lower.is_alphabetic() {
                    mods.remove(Modifiers::SHIFT);
                }
                KeyId::Char(lower)
            }
            Key::Named(n) => KeyId::Named(n),
            Key::Unidentified => return None,
        };
        Some(Chord { mods, key })
    }

    /// Parse `"ctrl+shift+t"`, `"j"`, `"?"`, `"f1"`. `+` alone is the plus key.
    pub fn parse(s: &str) -> Result<Chord, ParseError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ParseError::Empty);
        }
        let (mod_part, key_part) = match s.rfind('+') {
            // A trailing '+' is the key itself ("ctrl++").
            Some(i) if i + 1 == s.len() => (s[..i].trim_end_matches('+'), "+"),
            Some(i) => (&s[..i], &s[i + 1..]),
            None => ("", s),
        };
        let mut mods = Modifiers::empty();
        for m in mod_part.split('+').filter(|m| !m.is_empty()) {
            mods |= match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Modifiers::CTRL,
                "alt" => Modifiers::ALT,
                "shift" => Modifiers::SHIFT,
                "super" | "logo" | "cmd" => Modifiers::LOGO,
                other => return Err(ParseError::UnknownModifier(other.to_owned())),
            };
        }
        let key = parse_key(key_part)?;
        if let KeyId::Char(c) = &key
            && !c.is_alphabetic()
        {
            mods.remove(Modifiers::SHIFT);
        }
        Ok(Chord { mods, key })
    }

    /// The text form of this chord, parseable by [`Chord::parse`] (`"ctrl+shift+t"`).
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (flag, name) in [
            (Modifiers::CTRL, "ctrl"),
            (Modifiers::ALT, "alt"),
            (Modifiers::SHIFT, "shift"),
            (Modifiers::LOGO, "super"),
        ] {
            if self.mods.contains(flag) {
                out.push_str(name);
                out.push('+');
            }
        }
        match &self.key {
            KeyId::Char(c) => out.push(*c),
            KeyId::Named(n) => out.push_str(named_text(*n)),
        }
        out
    }

    fn is_leader_like(&self, leader: &Chord) -> bool {
        self == leader
    }
}

fn parse_key(s: &str) -> Result<KeyId, ParseError> {
    let lower = s.to_ascii_lowercase();
    let named = match lower.as_str() {
        "esc" | "escape" => Some(Named::Escape),
        "enter" | "return" => Some(Named::Enter),
        "tab" => Some(Named::Tab),
        "space" => Some(Named::Space),
        "backspace" => Some(Named::Backspace),
        "delete" | "del" => Some(Named::Delete),
        "up" => Some(Named::ArrowUp),
        "down" => Some(Named::ArrowDown),
        "left" => Some(Named::ArrowLeft),
        "right" => Some(Named::ArrowRight),
        "home" => Some(Named::Home),
        "end" => Some(Named::End),
        "pageup" => Some(Named::PageUp),
        "pagedown" => Some(Named::PageDown),
        "f1" => Some(Named::F1),
        "f2" => Some(Named::F2),
        "f3" => Some(Named::F3),
        "f4" => Some(Named::F4),
        "f5" => Some(Named::F5),
        "f6" => Some(Named::F6),
        "f7" => Some(Named::F7),
        "f8" => Some(Named::F8),
        "f9" => Some(Named::F9),
        "f10" => Some(Named::F10),
        "f11" => Some(Named::F11),
        "f12" => Some(Named::F12),
        _ => None,
    };
    if let Some(n) = named {
        return Ok(KeyId::Named(n));
    }
    let mut chars = lower.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(KeyId::Char(c)),
        _ => Err(ParseError::UnknownKey(s.to_owned())),
    }
}

fn named_text(n: Named) -> &'static str {
    match n {
        Named::Escape => "esc",
        Named::Enter => "enter",
        Named::Tab => "tab",
        Named::Space => "space",
        Named::Backspace => "backspace",
        Named::Delete => "delete",
        Named::ArrowUp => "up",
        Named::ArrowDown => "down",
        Named::ArrowLeft => "left",
        Named::ArrowRight => "right",
        Named::Home => "home",
        Named::End => "end",
        Named::PageUp => "pageup",
        Named::PageDown => "pagedown",
        Named::F1 => "f1",
        Named::F2 => "f2",
        Named::F3 => "f3",
        Named::F4 => "f4",
        Named::F5 => "f5",
        Named::F6 => "f6",
        Named::F7 => "f7",
        Named::F8 => "f8",
        Named::F9 => "f9",
        Named::F10 => "f10",
        Named::F11 => "f11",
        Named::F12 => "f12",
        _ => "?",
    }
}

/// Parse a whitespace-separated chord sequence. `<leader>` expands to `leader`.
pub fn parse_sequence(s: &str, leader: Option<&Chord>) -> Result<Vec<Chord>, ParseError> {
    let mut out = Vec::new();
    for part in s.split_whitespace() {
        if part.eq_ignore_ascii_case("<leader>") {
            out.push(leader.cloned().ok_or(ParseError::NoLeader)?);
        } else {
            out.push(Chord::parse(part)?);
        }
    }
    if out.is_empty() {
        return Err(ParseError::Empty);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub seq: Vec<Chord>,
    /// The text the binding was written as, for display (palette, cheat sheet).
    pub text: String,
    pub action: Action,
}

/// Two bindings that cannot both work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub keys: String,
    pub first: Action,
    pub second: Action,
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<Binding>,
    leader: Chord,
    /// Overlapping bindings found while building; shown in the log and later the settings screen.
    pub conflicts: Vec<Conflict>,
    /// Human-readable problems with user overrides (bad syntax, unknown action ids).
    pub warnings: Vec<String>,
}

impl Keymap {
    /// Defaults only.
    pub fn defaults() -> Keymap {
        Keymap::build(None, None)
    }

    /// Defaults, then `leader` (a chord string) and `overrides` (JSON `{"action.id": ["keys"]}`)
    /// from the settings table. An override replaces *all* default keys of that action; an
    /// empty list unbinds it. Bad input never fails: it is reported in `warnings` and skipped.
    pub fn build(leader: Option<&str>, overrides: Option<&str>) -> Keymap {
        let mut warnings = Vec::new();
        let leader = match leader.map(Chord::parse) {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                warnings.push(format!("invalid leader key: {e}"));
                default_leader()
            }
            None => default_leader(),
        };

        let mut overrides_map: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(json) = overrides {
            match serde_json::from_str(json) {
                Ok(m) => overrides_map = m,
                Err(e) => warnings.push(format!("keymap overrides ignored: {e}")),
            }
        }
        for id in overrides_map.keys() {
            if Action::from_id(id).is_none() {
                warnings.push(format!("keymap override for unknown action '{id}'"));
            }
        }

        let mut km = Keymap {
            bindings: Vec::new(),
            leader,
            conflicts: Vec::new(),
            warnings,
        };
        for action in Action::ALL.iter().copied() {
            let meta = action.meta();
            let keys: Vec<String> = match overrides_map.get(meta.id) {
                Some(list) => list.clone(),
                None => meta.default_keys.iter().map(|k| (*k).to_owned()).collect(),
            };
            for key in keys {
                match parse_sequence(&key, Some(&km.leader.clone())) {
                    Ok(seq) => km.add(Binding {
                        seq,
                        text: key,
                        action,
                    }),
                    Err(e) => km
                        .warnings
                        .push(format!("{}: invalid key '{key}': {e}", meta.id)),
                }
            }
        }
        for c in &km.conflicts {
            tracing::warn!(keys = %c.keys, first = c.first.meta().id, second = c.second.meta().id, "keymap conflict");
        }
        for w in &km.warnings {
            tracing::warn!("{w}");
        }
        km
    }

    fn add(&mut self, new: Binding) {
        for old in &self.bindings {
            let overlap = old
                .action
                .meta()
                .available_in
                .intersects(new.action.meta().available_in);
            if !overlap || old.action == new.action {
                continue;
            }
            let n = old.seq.len().min(new.seq.len());
            if old.seq[..n] == new.seq[..n] {
                self.conflicts.push(Conflict {
                    keys: format!("{} / {}", old.text, new.text),
                    first: old.action,
                    second: new.action,
                });
            }
        }
        self.bindings.push(new);
    }

    /// Actions whose bindings clash with a binding of `action` (as found while building).
    pub fn conflicts_with(&self, action: Action) -> Vec<Action> {
        let mut out: Vec<Action> = self
            .conflicts
            .iter()
            .filter_map(|c| {
                if c.first == action {
                    Some(c.second)
                } else if c.second == action {
                    Some(c.first)
                } else {
                    None
                }
            })
            .collect();
        out.dedup();
        out
    }

    pub fn leader(&self) -> &Chord {
        &self.leader
    }

    /// Display strings of an action's bindings, e.g. for the palette's right-hand column.
    pub fn keys_for(&self, action: Action) -> Vec<&str> {
        self.bindings
            .iter()
            .filter(|b| b.action == action)
            .map(|b| b.text.as_str())
            .collect()
    }
}

fn default_leader() -> Chord {
    Chord::parse(DEFAULT_LEADER).unwrap_or(Chord {
        mods: Modifiers::CTRL,
        key: KeyId::Named(Named::Space),
    })
}

/// What the matcher needs to know about the app right now.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// Exactly one of the `ModeMask` mode bits.
    pub mode: ModeMask,
    /// A remote terminal has focus (stricter pass-through rules).
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Action(Action),
    /// A prefix of a longer sequence; the key is consumed and we wait for more.
    Pending,
    /// Not ours: let the focused widget / remote shell have it.
    Unhandled,
}

#[derive(Debug, Default)]
pub struct Matcher {
    pending: Vec<Chord>,
    last: Option<Instant>,
}

impl Matcher {
    /// Whether a partial sequence is armed (for a status bar hint).
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.last = None;
    }

    /// Feed one chord. `now` is injected so the timeout is testable.
    pub fn feed(&mut self, km: &Keymap, ctx: Context, chord: Chord, now: Instant) -> Outcome {
        if let Some(last) = self.last
            && now.duration_since(last) > SEQUENCE_TIMEOUT
        {
            self.reset();
        }
        // First try continuing the armed sequence; if that goes nowhere, treat the key fresh.
        if self.is_pending() {
            let mut seq = self.pending.clone();
            seq.push(chord.clone());
            match lookup(km, ctx, &seq) {
                Lookup::Exact(a) => {
                    self.reset();
                    return Outcome::Action(a);
                }
                Lookup::Prefix => {
                    self.pending = seq;
                    self.last = Some(now);
                    return Outcome::Pending;
                }
                Lookup::None => self.reset(),
            }
        }
        match lookup(km, ctx, std::slice::from_ref(&chord)) {
            Lookup::Exact(a) => Outcome::Action(a),
            Lookup::Prefix => {
                self.pending = vec![chord];
                self.last = Some(now);
                Outcome::Pending
            }
            Lookup::None => Outcome::Unhandled,
        }
    }
}

enum Lookup {
    Exact(Action),
    Prefix,
    None,
}

/// May a binding starting with `first` fire in this context at all?
fn intercepts(km: &Keymap, ctx: Context, first: &Chord) -> bool {
    let app_mods = first.mods.control() || first.mods.alt() || first.mods.logo();
    if ctx.terminal {
        // The remote shell owns plain Ctrl/Alt chords; only Ctrl+Shift+<key> and the leader
        // belong to the app.
        return first.is_leader_like(km.leader())
            || (first.mods.control() && first.mods.shift())
            || terminal_shortcut(first);
    }
    if ctx.mode == ModeMask::INSERT {
        // A text field owns plain characters; app chords need a modifier (or be a named key
        // like F1/Esc).
        return app_mods || matches!(first.key, KeyId::Named(_));
    }
    true
}

/// A few non-Ctrl+Shift chords the app keeps even while a remote terminal has focus
/// (docs/KEYBINDINGS.md): Ctrl+Tab, Ctrl+1..9 / Ctrl+0 / Ctrl+= / Ctrl+- (shift is dropped from
/// symbol chords), and Shift+PageUp/PageDown for scrollback.
fn terminal_shortcut(first: &Chord) -> bool {
    match &first.key {
        KeyId::Named(Named::Tab) => first.mods.control() && !first.mods.alt(),
        KeyId::Named(Named::PageUp | Named::PageDown) => first.mods == Modifiers::SHIFT,
        KeyId::Char(c) => {
            (first.mods == Modifiers::CTRL
                && (c.is_ascii_digit() || matches!(c, '=' | '-' | '+')))
                // Font reset.
                || (first.mods == (Modifiers::CTRL | Modifiers::ALT) && *c == '0')
        }
        _ => false,
    }
}

fn lookup(km: &Keymap, ctx: Context, seq: &[Chord]) -> Lookup {
    let Some(first) = seq.first() else {
        return Lookup::None;
    };
    if !intercepts(km, ctx, first) {
        return Lookup::None;
    }
    let mut prefix = false;
    for b in &km.bindings {
        if !b.action.meta().available_in.contains(ctx.mode) || !b.seq.starts_with(seq) {
            continue;
        }
        if b.seq.len() == seq.len() {
            return Lookup::Exact(b.action);
        }
        prefix = true;
    }
    if prefix { Lookup::Prefix } else { Lookup::None }
}

/// Auth-screen key handling: Enter acknowledges the recovery warning, Tab moves between fields.
pub fn auth_message_for_key(key: &Key, modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::Enter) if modifiers.is_empty() => Some(Message::Acknowledge),
        // Tab / Shift+Tab move between form fields (docs/KEYBINDINGS.md, "Forms").
        Key::Named(Named::Tab) if modifiers == Modifiers::SHIFT => Some(Message::FocusPrevious),
        Key::Named(Named::Tab) if modifiers.is_empty() => Some(Message::FocusNext),
        _ => None,
    }
}

/// Subscription for the auth screens.
pub fn auth_subscription() -> Subscription<Message> {
    event::listen_with(|event, _status, _window| match event {
        Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            auth_message_for_key(&key, modifiers)
        }
        _ => None,
    })
}

/// Subscription for the main screen: every key press goes to the matcher.
///
/// The event status is ignored on purpose: global chords must work even when a widget has focus;
/// the matcher's mode rules decide what is ours.
pub fn main_subscription() -> Subscription<Message> {
    event::listen_with(|event, _status, _window| match event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        }) => {
            let chord = Chord::from_event(&key, modifiers)?;
            let raw = RawKey {
                key,
                modifiers,
                text: text.map(|t| t.to_string()),
            };
            Some(Message::Key(chord, raw))
        }
        Event::Mouse(
            iced::mouse::Event::ButtonPressed(_) | iced::mouse::Event::WheelScrolled { .. },
        ) => Some(Message::Activity),
        _ => None,
    })
}

/// The unprocessed key press, kept so a focused terminal can encode it for the remote.
#[derive(Debug, Clone)]
pub struct RawKey {
    pub key: Key,
    pub modifiers: Modifiers,
    pub text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_text_round_trips() {
        for t in [
            "j",
            "ctrl+shift+t",
            "alt+left",
            "ctrl+tab",
            "f1",
            "?",
            "ctrl++",
            "space",
            "esc",
            "shift+g",
        ] {
            let c = Chord::parse(t).unwrap();
            assert_eq!(Chord::parse(&c.to_text()).unwrap(), c, "{t}");
        }
    }

    #[test]
    fn rebinding_onto_a_taken_key_reports_the_other_action() {
        let json = r#"{"item.add": ["j"]}"#;
        let km = Keymap::build(None, Some(json));
        assert_eq!(km.conflicts_with(Action::Add), vec![Action::MoveDown]);
        assert!(Keymap::defaults().conflicts_with(Action::Add).is_empty());
    }

    const NORMAL: Context = Context {
        mode: ModeMask::NORMAL,
        terminal: false,
    };
    const INSERT: Context = Context {
        mode: ModeMask::INSERT,
        terminal: false,
    };
    const TERMINAL: Context = Context {
        mode: ModeMask::INSERT,
        terminal: true,
    };

    fn c(s: &str) -> Chord {
        Chord::parse(s).expect("valid chord")
    }

    fn feed(m: &mut Matcher, km: &Keymap, ctx: Context, key: &str, t: Instant) -> Outcome {
        m.feed(km, ctx, c(key), t)
    }

    #[test]
    fn parses_chords() {
        assert_eq!(
            c("ctrl+shift+t"),
            Chord {
                mods: Modifiers::CTRL | Modifiers::SHIFT,
                key: KeyId::Char('t')
            }
        );
        assert_eq!(c("Ctrl+K"), c("ctrl+k"));
        assert_eq!(c("F1").key, KeyId::Named(Named::F1));
        assert_eq!(c("esc").key, KeyId::Named(Named::Escape));
        assert_eq!(c("ctrl+space").key, KeyId::Named(Named::Space));
        assert_eq!(c("ctrl+,").key, KeyId::Char(','));
        assert_eq!(c("ctrl++").key, KeyId::Char('+'));
        // Shift is implicit in symbols but significant for letters.
        assert_eq!(c("shift+?"), c("?"));
        assert_ne!(c("shift+g"), c("g"));
    }

    #[test]
    fn rejects_bad_chords() {
        assert_eq!(Chord::parse(""), Err(ParseError::Empty));
        assert!(matches!(
            Chord::parse("ctrl+foo"),
            Err(ParseError::UnknownKey(_))
        ));
        assert!(matches!(
            Chord::parse("hyper+k"),
            Err(ParseError::UnknownModifier(_))
        ));
    }

    #[test]
    fn parses_sequences_and_leader() {
        let leader = c("ctrl+space");
        let seq = parse_sequence("<leader> n", Some(&leader)).unwrap_or_default();
        assert_eq!(seq, vec![leader.clone(), c("n")]);
        assert_eq!(parse_sequence("g g", None).map(|s| s.len()), Ok(2));
        assert_eq!(
            parse_sequence("<leader> n", None),
            Err(ParseError::NoLeader)
        );
        assert_eq!(parse_sequence("  ", None), Err(ParseError::Empty));
    }

    #[test]
    fn event_chords_match_parsed_chords() {
        let ev = |k: &str, m| Chord::from_event(&Key::Character(k.into()), m);
        assert_eq!(ev("K", Modifiers::CTRL), Some(c("ctrl+k")));
        assert_eq!(ev("G", Modifiers::SHIFT), Some(c("shift+g")));
        assert_eq!(ev("?", Modifiers::SHIFT), Some(c("?")));
        assert_eq!(
            Chord::from_event(&Key::Named(Named::F1), Modifiers::empty()),
            Some(c("f1"))
        );
        assert_eq!(
            Chord::from_event(&Key::Unidentified, Modifiers::empty()),
            None
        );
    }

    #[test]
    fn default_keymap_is_clean() {
        let km = Keymap::defaults();
        assert!(km.warnings.is_empty(), "{:?}", km.warnings);
        assert!(km.conflicts.is_empty(), "{:?}", km.conflicts);
        // Every default key of every action parses.
        for a in Action::ALL {
            assert_eq!(km.keys_for(*a).len(), a.meta().default_keys.len());
        }
    }

    #[test]
    fn single_chord_matches_in_its_mode() {
        let km = Keymap::defaults();
        let mut m = Matcher::default();
        let t = Instant::now();
        assert_eq!(
            feed(&mut m, &km, NORMAL, "j", t),
            Outcome::Action(Action::MoveDown)
        );
        assert_eq!(
            feed(&mut m, &km, NORMAL, "ctrl+k", t),
            Outcome::Action(Action::OpenPalette)
        );
    }

    #[test]
    fn plain_keys_are_not_intercepted_in_insert_mode() {
        let km = Keymap::defaults();
        let mut m = Matcher::default();
        let t = Instant::now();
        assert_eq!(feed(&mut m, &km, INSERT, "j", t), Outcome::Unhandled);
        assert_eq!(feed(&mut m, &km, INSERT, "?", t), Outcome::Unhandled);
        // App chords still work.
        assert_eq!(
            feed(&mut m, &km, INSERT, "ctrl+k", t),
            Outcome::Action(Action::OpenPalette)
        );
        assert_eq!(
            feed(&mut m, &km, INSERT, "esc", t),
            Outcome::Action(Action::Back)
        );
    }

    #[test]
    fn mode_filter_blocks_normal_only_actions() {
        let km = Keymap::defaults();
        let mut m = Matcher::default();
        let palette = Context {
            mode: ModeMask::PALETTE,
            terminal: false,
        };
        let t = Instant::now();
        assert_eq!(feed(&mut m, &km, palette, "j", t), Outcome::Unhandled);
        assert_eq!(
            feed(&mut m, &km, palette, "esc", t),
            Outcome::Action(Action::Back)
        );
    }

    #[test]
    fn plain_ctrl_w_is_not_intercepted_in_a_terminal() {
        // Even if the user binds plain Ctrl+W, a focused terminal keeps it.
        let km = Keymap::build(None, Some(r#"{"tab.close": ["ctrl+w", "ctrl+shift+w"]}"#));
        let mut m = Matcher::default();
        let t = Instant::now();
        assert_eq!(feed(&mut m, &km, TERMINAL, "ctrl+w", t), Outcome::Unhandled);
        assert_eq!(feed(&mut m, &km, TERMINAL, "alt+j", t), Outcome::Unhandled);
        assert_eq!(
            feed(&mut m, &km, TERMINAL, "ctrl+shift+w", t),
            Outcome::Action(Action::CloseTab)
        );
        // Outside a terminal (Normal mode) the same override works.
        assert_eq!(
            feed(&mut m, &km, NORMAL, "ctrl+w", t),
            Outcome::Action(Action::CloseTab)
        );
    }

    #[test]
    fn leader_sequence_works_in_a_terminal() {
        let km = Keymap::build(None, Some(r#"{"tab.next": ["<leader> n"]}"#));
        let mut m = Matcher::default();
        let t = Instant::now();
        assert_eq!(
            feed(&mut m, &km, TERMINAL, "ctrl+space", t),
            Outcome::Pending
        );
        assert_eq!(
            feed(&mut m, &km, TERMINAL, "n", t),
            Outcome::Action(Action::NextTab)
        );
    }

    #[test]
    fn sequences_complete_and_time_out() {
        let km = Keymap::defaults();
        let mut m = Matcher::default();
        let t = Instant::now();

        assert_eq!(feed(&mut m, &km, NORMAL, "g", t), Outcome::Pending);
        assert!(m.is_pending());
        assert_eq!(
            feed(&mut m, &km, NORMAL, "g", t + Duration::from_millis(100)),
            Outcome::Action(Action::First)
        );
        assert!(!m.is_pending());

        // Second key too late: the sequence restarts, so the late `g` is just a new prefix.
        assert_eq!(feed(&mut m, &km, NORMAL, "g", t), Outcome::Pending);
        assert_eq!(
            feed(
                &mut m,
                &km,
                NORMAL,
                "g",
                t + SEQUENCE_TIMEOUT + Duration::from_millis(1)
            ),
            Outcome::Pending
        );
    }

    #[test]
    fn broken_sequence_falls_back_to_a_fresh_key() {
        let km = Keymap::defaults();
        let mut m = Matcher::default();
        let t = Instant::now();
        assert_eq!(feed(&mut m, &km, NORMAL, "d", t), Outcome::Pending);
        // `j` does not continue `d`, but is a valid key on its own.
        assert_eq!(
            feed(&mut m, &km, NORMAL, "j", t),
            Outcome::Action(Action::MoveDown)
        );
        assert!(!m.is_pending());
    }

    #[test]
    fn overrides_replace_defaults_and_can_unbind() {
        let km = Keymap::build(None, Some(r#"{"list.down": ["n"], "item.delete": []}"#));
        let mut m = Matcher::default();
        let t = Instant::now();
        assert_eq!(
            feed(&mut m, &km, NORMAL, "n", t),
            Outcome::Action(Action::MoveDown)
        );
        assert_eq!(feed(&mut m, &km, NORMAL, "j", t), Outcome::Unhandled);
        assert_eq!(feed(&mut m, &km, NORMAL, "d", t), Outcome::Unhandled);
        assert!(km.warnings.is_empty());
    }

    #[test]
    fn bad_overrides_are_reported_not_fatal() {
        let km = Keymap::build(
            Some("bogus+x"),
            Some(r#"{"nav.hosts": ["ctrl+nope"], "no.such": ["x"]}"#),
        );
        assert_eq!(km.warnings.len(), 3, "{:?}", km.warnings);
        assert_eq!(km.leader(), &c(DEFAULT_LEADER));
        let km = Keymap::build(None, Some("not json"));
        assert_eq!(km.warnings.len(), 1);
        // Defaults still intact.
        assert!(!km.keys_for(Action::OpenPalette).is_empty());
    }

    #[test]
    fn conflicts_are_detected() {
        // Same keys on two Normal-mode actions, and a prefix clash ("g" vs "g g").
        let km = Keymap::build(None, Some(r#"{"list.up": ["j"], "nav.keys": ["g"]}"#));
        assert_eq!(km.conflicts.len(), 2, "{:?}", km.conflicts);
    }

    #[test]
    fn same_keys_in_disjoint_modes_do_not_conflict() {
        // palette-only vs normal-only actions may share keys; checked via the registry defaults.
        assert!(Keymap::defaults().conflicts.is_empty());
    }

    #[test]
    fn auth_keys() {
        let tab = Key::Named(Named::Tab);
        assert!(matches!(
            auth_message_for_key(&tab, Modifiers::empty()),
            Some(Message::FocusNext)
        ));
        assert!(matches!(
            auth_message_for_key(&tab, Modifiers::SHIFT),
            Some(Message::FocusPrevious)
        ));
        assert!(auth_message_for_key(&tab, Modifiers::CTRL).is_none());
        assert!(matches!(
            auth_message_for_key(&Key::Named(Named::Enter), Modifiers::empty()),
            Some(Message::Acknowledge)
        ));
        assert!(auth_message_for_key(&Key::Character("k".into()), Modifiers::CTRL).is_none());
    }

    #[test]
    fn jump_home_does_not_clash_with_font_reset() {
        let km = Keymap::defaults();
        assert!(km.conflicts_with(Action::JumpHome).is_empty());
        assert!(km.conflicts_with(Action::FontReset).is_empty());
    }
}
