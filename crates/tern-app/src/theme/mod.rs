//! Design-system theme: colour palette tokens, size tokens, fonts and shared widget styles.
#![allow(dead_code)] // tokens/styles are adopted screen by screen; tighten when migration is done
//! Views refer to tokens by name (`palette(theme).accent`, `tokens::SPACE_8`, `style::button(..)`).

pub mod colors;
pub mod style;
pub mod tokens;

pub use colors::{DARK, LIGHT, Palette};
use iced::font::{Family, Weight};
use iced::{Font, Theme};

pub const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
pub const INTER_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.ttf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");

/// UI font, 400.
pub const UI: Font = Font {
    family: Family::Name("Inter"),
    weight: Weight::Normal,
    ..Font::DEFAULT
};
/// UI font, 500 (labels, buttons).
pub const UI_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..UI
};
/// UI font, 600 (titles, captions).
pub const UI_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..UI
};
/// Mono font, 400 (addresses, kbd, terminal).
pub const MONO: Font = crate::app::MONO;
/// Mono font, 600 (mode pill, matched characters).
pub const MONO_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..crate::app::MONO
};

/// The iced [`Theme`] for a palette, so stock widgets inherit sane defaults.
pub fn theme(dark: bool) -> Theme {
    let p = if dark { &DARK } else { &LIGHT };
    Theme::custom(
        if dark { "tern-dark" } else { "tern-light" },
        iced::theme::Palette {
            background: p.bg_base,
            text: p.text_primary,
            primary: p.accent,
            success: p.success,
            warning: p.warning,
            danger: p.error,
        },
    )
}

/// Resolve the token palette for the active iced theme.
pub fn palette(theme: &Theme) -> &'static Palette {
    if theme.palette().background == LIGHT.bg_base {
        &LIGHT
    } else {
        &DARK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_follows_theme() {
        assert_eq!(palette(&theme(true)), &DARK);
        assert_eq!(palette(&theme(false)), &LIGHT);
    }
}
