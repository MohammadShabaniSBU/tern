//! Terminal colour schemes and palette resolution.

use crate::emu::Color as TermColor;
use iced::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SchemeId {
    #[default]
    Dark,
    Light,
    Solarized,
}

impl SchemeId {
    pub const ALL: [SchemeId; 3] = [SchemeId::Dark, SchemeId::Light, SchemeId::Solarized];

    pub fn id(self) -> &'static str {
        match self {
            SchemeId::Dark => "dark",
            SchemeId::Light => "light",
            SchemeId::Solarized => "solarized",
        }
    }

    pub fn from_id(s: &str) -> Option<SchemeId> {
        Self::ALL
            .into_iter()
            .find(|x| x.id() == s.trim().trim_matches('"'))
    }

    pub fn next(self) -> SchemeId {
        match self {
            SchemeId::Dark => SchemeId::Light,
            SchemeId::Light => SchemeId::Solarized,
            SchemeId::Solarized => SchemeId::Dark,
        }
    }

    pub fn scheme(self) -> Scheme {
        match self {
            SchemeId::Dark => DARK,
            SchemeId::Light => LIGHT,
            SchemeId::Solarized => SOLARIZED,
        }
    }
}

impl std::fmt::Display for SchemeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SchemeId::Dark => "Dark",
            SchemeId::Light => "Light",
            SchemeId::Solarized => "Solarized",
        })
    }
}

type Rgb = (u8, u8, u8);

#[derive(Debug, Clone, Copy)]
pub struct Scheme {
    pub fg: Rgb,
    pub bg: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub search: Rgb,
    pub search_current: Rgb,
    pub ansi: [Rgb; 16],
}

const DARK: Scheme = Scheme {
    fg: (0xd4, 0xd4, 0xd4),
    bg: (0x1e, 0x1e, 0x1e),
    cursor: (0xe0, 0xe0, 0xe0),
    selection: (0x26, 0x4f, 0x78),
    search: (0x6b, 0x56, 0x1c),
    search_current: (0xc0, 0x80, 0x20),
    ansi: [
        (0x00, 0x00, 0x00),
        (0xcd, 0x31, 0x31),
        (0x0d, 0xbc, 0x79),
        (0xe5, 0xe5, 0x10),
        (0x24, 0x72, 0xc8),
        (0xbc, 0x3f, 0xbc),
        (0x11, 0xa8, 0xcd),
        (0xe5, 0xe5, 0xe5),
        (0x66, 0x66, 0x66),
        (0xf1, 0x4c, 0x4c),
        (0x23, 0xd1, 0x8b),
        (0xf5, 0xf5, 0x43),
        (0x3b, 0x8e, 0xea),
        (0xd6, 0x70, 0xd6),
        (0x29, 0xb8, 0xdb),
        (0xff, 0xff, 0xff),
    ],
};

const LIGHT: Scheme = Scheme {
    fg: (0x33, 0x33, 0x33),
    bg: (0xfa, 0xfa, 0xfa),
    cursor: (0x33, 0x33, 0x33),
    selection: (0xb5, 0xd5, 0xff),
    search: (0xf5, 0xe0, 0x8a),
    search_current: (0xff, 0xb3, 0x47),
    ansi: [
        (0x00, 0x00, 0x00),
        (0xc9, 0x1b, 0x00),
        (0x00, 0x8a, 0x2e),
        (0x99, 0x7a, 0x00),
        (0x1f, 0x4f, 0xc9),
        (0xa3, 0x1d, 0xa3),
        (0x00, 0x7f, 0x95),
        (0xc7, 0xc7, 0xc7),
        (0x68, 0x68, 0x68),
        (0xe0, 0x4a, 0x4a),
        (0x00, 0xa8, 0x3c),
        (0xb5, 0x8f, 0x00),
        (0x3a, 0x6f, 0xea),
        (0xc5, 0x3c, 0xc5),
        (0x00, 0x9c, 0xb5),
        (0xff, 0xff, 0xff),
    ],
};

const SOLARIZED: Scheme = Scheme {
    fg: (0x83, 0x94, 0x96),
    bg: (0x00, 0x2b, 0x36),
    cursor: (0x93, 0xa1, 0xa1),
    selection: (0x07, 0x36, 0x42),
    search: (0x58, 0x6e, 0x75),
    search_current: (0xb5, 0x89, 0x00),
    ansi: [
        (0x07, 0x36, 0x42),
        (0xdc, 0x32, 0x2f),
        (0x85, 0x99, 0x00),
        (0xb5, 0x89, 0x00),
        (0x26, 0x8b, 0xd2),
        (0xd3, 0x36, 0x82),
        (0x2a, 0xa1, 0x98),
        (0xee, 0xe8, 0xd5),
        (0x00, 0x2b, 0x36),
        (0xcb, 0x4b, 0x16),
        (0x58, 0x6e, 0x75),
        (0x65, 0x7b, 0x83),
        (0x83, 0x94, 0x96),
        (0x6c, 0x71, 0xc4),
        (0x93, 0xa1, 0xa1),
        (0xfd, 0xf6, 0xe3),
    ],
};

fn rgb((r, g, b): Rgb) -> Color {
    Color::from_rgb8(r, g, b)
}

impl Scheme {
    pub fn fg(&self) -> Color {
        rgb(self.fg)
    }
    pub fn bg(&self) -> Color {
        rgb(self.bg)
    }
    pub fn cursor(&self) -> Color {
        rgb(self.cursor)
    }
    pub fn selection(&self) -> Color {
        rgb(self.selection)
    }
    pub fn search(&self) -> Color {
        rgb(self.search)
    }
    pub fn search_current(&self) -> Color {
        rgb(self.search_current)
    }

    /// Resolve a cell colour to RGB.
    pub fn resolve(&self, c: TermColor) -> Rgb {
        match c {
            TermColor::DefaultFg => self.fg,
            TermColor::DefaultBg => self.bg,
            TermColor::Rgb(r, g, b) => (r, g, b),
            TermColor::Indexed(i) => self.indexed(i),
        }
    }

    pub fn resolve_color(&self, c: TermColor) -> Color {
        rgb(self.resolve(c))
    }

    fn indexed(&self, i: u8) -> Rgb {
        match i {
            0..=15 => self.ansi[usize::from(i)],
            16..=231 => {
                let n = i - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
                (level(n / 36), level((n / 6) % 6), level(n % 6))
            }
            232..=255 => {
                let v = 8 + 10 * (i - 232);
                (v, v, v)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_cube_and_gray() {
        let s = DARK;
        assert_eq!(s.resolve(TermColor::Indexed(16)), (0, 0, 0));
        assert_eq!(s.resolve(TermColor::Indexed(196)), (255, 0, 0));
        assert_eq!(s.resolve(TermColor::Indexed(21)), (0, 0, 255));
        assert_eq!(s.resolve(TermColor::Indexed(231)), (255, 255, 255));
        assert_eq!(s.resolve(TermColor::Indexed(232)), (8, 8, 8));
        assert_eq!(s.resolve(TermColor::Indexed(255)), (238, 238, 238));
        assert_eq!(s.resolve(TermColor::Indexed(1)), s.ansi[1]);
        assert_eq!(s.resolve(TermColor::Rgb(1, 2, 3)), (1, 2, 3));
    }

    #[test]
    fn scheme_ids_round_trip() {
        for id in SchemeId::ALL {
            assert_eq!(SchemeId::from_id(id.id()), Some(id));
        }
        assert_eq!(SchemeId::from_id("\"light\""), Some(SchemeId::Light));
        assert_eq!(SchemeId::from_id("nope"), None);
    }
}
