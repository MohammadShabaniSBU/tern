//! Terminal preferences (S5-T08), stored as JSON text in the `settings` table under `term.*`.

use iced::Font;
use tern_term::CursorShape;
use tern_term::theme::SchemeId;

pub const FONT_SIZE: &str = "term.font_size";
pub const SCHEME: &str = "term.scheme";
pub const CURSOR: &str = "term.cursor";
pub const SCROLLBACK: &str = "term.scrollback";
pub const VISUAL_BELL: &str = "term.visual_bell";
pub const CONFIRM_PASTE: &str = "term.confirm_paste";
pub const FONT: &str = "term.font";

pub const KEYS: [&str; 7] = [
    FONT_SIZE,
    SCHEME,
    CURSOR,
    SCROLLBACK,
    VISUAL_BELL,
    CONFIRM_PASTE,
    FONT,
];

pub const DEFAULT_FONT_SIZE: f32 = 14.0;
pub const MIN_FONT_SIZE: f32 = 8.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
pub const DEFAULT_SCROLLBACK: usize = 10_000;

/// Cursor shape preference: follow the remote, or force one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorPref {
    #[default]
    Auto,
    Block,
    Underline,
    Bar,
}

impl CursorPref {
    pub fn id(self) -> &'static str {
        match self {
            CursorPref::Auto => "auto",
            CursorPref::Block => "block",
            CursorPref::Underline => "underline",
            CursorPref::Bar => "bar",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        [Self::Auto, Self::Block, Self::Underline, Self::Bar]
            .into_iter()
            .find(|c| c.id() == s)
    }

    pub fn next(self) -> Self {
        match self {
            CursorPref::Auto => CursorPref::Block,
            CursorPref::Block => CursorPref::Underline,
            CursorPref::Underline => CursorPref::Bar,
            CursorPref::Bar => CursorPref::Auto,
        }
    }

    pub fn shape(self) -> Option<CursorShape> {
        match self {
            CursorPref::Auto => None,
            CursorPref::Block => Some(CursorShape::Block),
            CursorPref::Underline => Some(CursorShape::Underline),
            CursorPref::Bar => Some(CursorShape::Bar),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TermSettings {
    pub font_size: f32,
    pub scheme: SchemeId,
    pub cursor: CursorPref,
    pub scrollback: usize,
    pub visual_bell: bool,
    pub confirm_paste: bool,
    /// System font family; `None` uses the bundled JetBrains Mono.
    pub font_family: Option<&'static str>,
}

impl Default for TermSettings {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
            scheme: SchemeId::Dark,
            cursor: CursorPref::Auto,
            scrollback: DEFAULT_SCROLLBACK,
            visual_bell: true,
            confirm_paste: true,
            font_family: None,
        }
    }
}

impl TermSettings {
    /// Apply one stored `key` / JSON `value`; unknown or invalid values keep the current setting.
    pub fn apply(&mut self, key: &str, value: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(value) else {
            return;
        };
        match key {
            FONT_SIZE => {
                if let Some(n) = v.as_f64() {
                    self.font_size = (n as f32).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
                }
            }
            SCHEME => {
                if let Some(id) = v.as_str().and_then(SchemeId::from_id) {
                    self.scheme = id;
                }
            }
            CURSOR => {
                if let Some(c) = v.as_str().and_then(CursorPref::from_id) {
                    self.cursor = c;
                }
            }
            SCROLLBACK => {
                if let Some(n) = v.as_u64() {
                    self.scrollback = (n as usize).clamp(100, 200_000);
                }
            }
            VISUAL_BELL => {
                if let Some(b) = v.as_bool() {
                    self.visual_bell = b;
                }
            }
            CONFIRM_PASTE => {
                if let Some(b) = v.as_bool() {
                    self.confirm_paste = b;
                }
            }
            FONT => {
                if let Some(name) = v.as_str().map(str::trim).filter(|n| !n.is_empty()) {
                    // Font names must be 'static for iced; settings are loaded rarely.
                    self.font_family = Some(Box::leak(name.to_owned().into_boxed_str()));
                }
            }
            _ => {}
        }
    }

    /// JSON text to store for `key`.
    pub fn serialize(&self, key: &str) -> Option<String> {
        let v = match key {
            FONT_SIZE => serde_json::json!(self.font_size),
            SCHEME => serde_json::json!(self.scheme.id()),
            CURSOR => serde_json::json!(self.cursor.id()),
            SCROLLBACK => serde_json::json!(self.scrollback),
            VISUAL_BELL => serde_json::json!(self.visual_bell),
            CONFIRM_PASTE => serde_json::json!(self.confirm_paste),
            FONT => serde_json::json!(self.font_family?),
            _ => return None,
        };
        Some(v.to_string())
    }

    pub fn font(&self) -> Font {
        match self.font_family {
            Some(name) => Font::with_name(name),
            None => crate::app::MONO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_round_trip() {
        let mut s = TermSettings::default();
        assert_eq!(s.scrollback, 10_000);
        s.scheme = SchemeId::Solarized;
        s.font_size = 17.0;
        s.cursor = CursorPref::Bar;
        let mut t = TermSettings::default();
        for k in KEYS {
            if let Some(v) = s.serialize(k) {
                t.apply(k, &v);
            }
        }
        assert_eq!(t, s);
    }

    #[test]
    fn invalid_values_are_ignored_and_clamped() {
        let mut s = TermSettings::default();
        s.apply(FONT_SIZE, "\"big\"");
        s.apply(SCHEME, "\"nope\"");
        s.apply(SCROLLBACK, "5");
        s.apply(FONT_SIZE, "500");
        s.apply("junk", "1");
        assert_eq!(s.scheme, SchemeId::Dark);
        assert_eq!(s.scrollback, 100);
        assert_eq!(s.font_size, MAX_FONT_SIZE);
    }
}
