//! Colour tokens (docs/designs/09 Design tokens). One field per token row; widgets read
//! these by name through `theme::palette(theme)`.

use iced::Color;

/// `0xRRGGBB` → [`Color`].
pub const fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// `0xRRGGBB` plus alpha in percent.
pub const fn rgba(hex: u32, percent: u8) -> Color {
    let c = rgb(hex);
    Color {
        a: percent as f32 / 100.0,
        ..c
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    // Surfaces
    pub bg_base: Color,
    pub bg_surface: Color,
    pub bg_raised: Color,
    pub bg_overlay: Color,
    pub bg_terminal: Color,
    pub bg_hover: Color,
    pub scrim: Color,
    // Borders
    pub border_subtle: Color,
    pub border_strong: Color,
    // Text
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    /// Meta text on a cursor row (keeps 4.5:1 on `selection_focused`).
    pub text_on_selection: Color,
    // Accent
    pub accent: Color,
    pub accent_hover: Color,
    pub on_accent: Color,
    // Selection
    pub selection_focused: Color,
    pub selection_inactive: Color,
    pub selection_term: Color,
    /// Selected palette item (no border; the palette always has focus).
    pub selection_palette: Color,
    pub focus_ring: Color,
    // Status
    pub success: Color,
    pub success_bg: Color,
    pub warning: Color,
    pub warning_bg: Color,
    pub error: Color,
    pub error_bg: Color,
    pub error_border: Color,
    // Modes (status bar pill)
    pub mode_normal: Color,
    pub mode_insert: Color,
    pub mode_palette: Color,
    pub mode_terminal: Color,
    pub mode_locked: Color,
}

pub const DARK: Palette = Palette {
    bg_base: rgb(0x0F1115),
    bg_surface: rgb(0x15181D),
    bg_raised: rgb(0x1B1F26),
    bg_overlay: rgb(0x20252D),
    bg_terminal: rgb(0x0B0D10),
    bg_hover: rgb(0x232832),
    scrim: rgba(0x050608, 62),
    border_subtle: rgb(0x272C35),
    border_strong: rgb(0x363C47),
    text_primary: rgb(0xE6E8EC),
    text_secondary: rgb(0xA7AEBA),
    text_muted: rgb(0x7D8593),
    text_faint: rgb(0x5A616D),
    text_on_selection: rgb(0xC9D3EA),
    accent: rgb(0x7C9CFF),
    accent_hover: rgb(0x98B1FF),
    on_accent: rgb(0x0B1020),
    selection_focused: rgb(0x1C2541),
    selection_inactive: rgb(0x232832),
    selection_term: rgb(0x2E3D66),
    selection_palette: rgb(0x2A3352),
    focus_ring: rgb(0x7C9CFF),
    success: rgb(0x4CC38A),
    success_bg: rgb(0x14271F),
    warning: rgb(0xE5B454),
    warning_bg: rgb(0x261F12),
    error: rgb(0xF07178),
    error_bg: rgb(0x22181B),
    error_border: rgb(0x5A2B31),
    mode_normal: rgb(0x7C9CFF),
    mode_insert: rgb(0x4CC38A),
    mode_palette: rgb(0xC59BFF),
    mode_terminal: rgb(0x5FC9D6),
    mode_locked: rgb(0x363C47),
};

pub const LIGHT: Palette = Palette {
    bg_base: rgb(0xF3F4F6),
    bg_surface: rgb(0xFFFFFF),
    bg_raised: rgb(0xF8F9FB),
    bg_overlay: rgb(0xFFFFFF),
    bg_terminal: rgb(0xFCFCFD),
    bg_hover: rgb(0xEEF0F4),
    scrim: rgba(0x14181E, 32),
    border_subtle: rgb(0xE3E6EB),
    border_strong: rgb(0xCDD2DA),
    text_primary: rgb(0x15181E),
    text_secondary: rgb(0x4A5160),
    text_muted: rgb(0x697180),
    text_faint: rgb(0x9AA1AD),
    text_on_selection: rgb(0x3A4560),
    accent: rgb(0x3B5BDB),
    accent_hover: rgb(0x2F4AC0),
    on_accent: rgb(0xFFFFFF),
    selection_focused: rgb(0xE6ECFF),
    selection_inactive: rgb(0xEEF0F4),
    selection_term: rgb(0xC9D6FF),
    selection_palette: rgb(0xDCE4FF),
    focus_ring: rgb(0x3B5BDB),
    success: rgb(0x1A7F50),
    success_bg: rgb(0xE3F5EC),
    warning: rgb(0x9A6200),
    warning_bg: rgb(0xFBF1DE),
    error: rgb(0xD03B47),
    error_bg: rgb(0xFCE8EA),
    error_border: rgb(0xF2B8BE),
    mode_normal: rgb(0x3B5BDB),
    mode_insert: rgb(0x1A7F50),
    mode_palette: rgb(0x7A4FD6),
    mode_terminal: rgb(0x0E7C88),
    mode_locked: rgb(0xCDD2DA),
};

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color) -> f32 {
        let f = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }

    fn contrast(a: Color, b: Color) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn rgb_decodes_hex() {
        let c = rgb(0x7C9CFF);
        assert_eq!(c, Color::from_rgb8(0x7C, 0x9C, 0xFF));
        assert!((rgba(0x000000, 62).a - 0.62).abs() < 1e-6);
    }

    #[test]
    fn primary_text_is_readable_in_both_palettes() {
        for p in [DARK, LIGHT] {
            assert!(contrast(p.text_primary, p.bg_base) >= 7.0);
            assert!(contrast(p.text_secondary, p.bg_surface) >= 4.5);
            assert!(contrast(p.text_on_selection, p.selection_focused) >= 4.5);
            assert!(contrast(p.on_accent, p.accent) >= 4.5);
        }
    }

    #[test]
    fn palettes_differ() {
        assert_ne!(DARK, LIGHT);
    }
}
