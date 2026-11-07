//! Shared widget styles built from tokens (docs/designs/10 Component specs).
//! Rules: focus = 1 px accent border (+ glow on inputs/buttons); selection = a fill; borders are
//! always 1 px so state changes colour, never layout.

use super::palette;
use super::tokens::*;
use iced::widget::{button as btn, container, text, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

fn border(color: Color, radius: f32) -> Border {
    Border {
        color,
        width: 1.0,
        radius: radius.into(),
    }
}

fn top_radius(r: f32) -> iced::border::Radius {
    iced::border::Radius {
        top_left: r,
        top_right: r,
        bottom_right: 0.0,
        bottom_left: 0.0,
    }
}

fn bottom_radius(r: f32) -> iced::border::Radius {
    iced::border::Radius {
        top_left: 0.0,
        top_right: 0.0,
        bottom_right: r,
        bottom_left: r,
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

fn glow(c: Color) -> Shadow {
    Shadow {
        color: with_alpha(c, 0.45),
        offset: Vector::ZERO,
        blur_radius: 6.0,
    }
}

// ---------------------------------------------------------------- containers

/// Window background behind the panes.
pub fn base(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_base.into()),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

/// A pane (sidebar / list / detail): surface with a 1 px border that turns accent when focused.
pub fn pane(focused: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: Some(p.bg_surface.into()),
            text_color: Some(p.text_primary),
            border: border(if focused { p.accent } else { p.border_subtle }, RADIUS_LG),
            ..Default::default()
        }
    }
}

/// Hairline divider / footer strip inside a pane.
pub fn divider(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(palette(theme).border_subtle.into()),
        ..Default::default()
    }
}

/// Bottom status bar.
pub fn status_bar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_surface.into()),
        text_color: Some(p.text_secondary),
        ..Default::default()
    }
}

/// Full-window dim behind a modal or the palette.
pub fn scrim(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(palette(theme).scrim.into()),
        ..Default::default()
    }
}

/// Modal / palette card.
pub fn modal_card(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_overlay.into()),
        text_color: Some(p.text_primary),
        border: border(p.border_strong, RADIUS_XL),
        shadow: Shadow {
            color: Color::from_rgba(
                0.063,
                0.094,
                0.157,
                if p.bg_base == super::LIGHT.bg_base {
                    0.18
                } else {
                    0.5
                },
            ),
            offset: Vector::new(0.0, 16.0),
            blur_radius: 40.0,
        },
        ..Default::default()
    }
}

/// Modal footer (hints left, buttons right).
pub fn modal_footer(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_raised.into()),
        border: Border {
            color: p.border_subtle,
            width: 0.0,
            radius: bottom_radius(RADIUS_XL),
        },
        ..Default::default()
    }
}

/// Toast card; errors get the error border.
pub fn toast(error: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: Some(p.bg_overlay.into()),
            text_color: Some(p.text_primary),
            border: border(
                if error {
                    p.error_border
                } else {
                    p.border_strong
                },
                RADIUS_LG,
            ),
            shadow: Shadow {
                color: Color::from_rgba(0.063, 0.094, 0.157, 0.45),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        }
    }
}

/// Row state in lists (docs "List row").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Idle,
    /// Selected (marked) row in a pane that is not focused.
    Selected,
    /// Keyboard cursor in the focused pane.
    Cursor,
    Failed,
}

pub fn row(state: RowState) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        let (bg, line) = match state {
            RowState::Idle => (None, Color::TRANSPARENT),
            RowState::Selected => (Some(p.selection_inactive), Color::TRANSPARENT),
            RowState::Cursor => (Some(p.selection_focused), p.accent),
            RowState::Failed => (Some(p.error_bg), Color::TRANSPARENT),
        };
        container::Style {
            background: bg.map(Background::from),
            border: border(line, RADIUS_MD),
            ..Default::default()
        }
    }
}

/// Palette item: selected = fill, no border.
pub fn palette_item(selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: selected.then(|| p.selection_palette.into()),
            border: Border::default().rounded(RADIUS_MD),
            ..Default::default()
        }
    }
}

/// Small tag chip (BG_HOVER, RADIUS_SM).
pub fn chip(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_hover.into()),
        text_color: Some(p.text_secondary),
        border: Border::default().rounded(RADIUS_SM),
        ..Default::default()
    }
}

/// Keyboard-key box: mono, 1 px strong border.
pub fn kbd(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        text_color: Some(p.text_muted),
        border: border(p.border_strong, RADIUS_SM),
        ..Default::default()
    }
}

/// Status pill: colour text on its `_bg` fill, fully rounded.
pub fn pill(fg: Color, bg: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(bg.into()),
        text_color: Some(fg),
        border: Border::default().rounded(RADIUS_FULL),
        ..Default::default()
    }
}

/// Status-bar mode pill (fixed width set by the caller).
pub fn mode_pill(bg: Color, locked: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: Some(bg.into()),
            text_color: Some(if locked { p.text_primary } else { p.on_accent }),
            ..Default::default()
        }
    }
}

/// Tab: active merges with the terminal surface and has a 2 px accent top edge.
pub fn tab(active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        container::Style {
            background: active.then(|| p.bg_terminal.into()),
            text_color: Some(if active {
                p.text_primary
            } else {
                p.text_secondary
            }),
            border: Border {
                color: if active { p.accent } else { Color::TRANSPARENT },
                width: 0.0,
                radius: top_radius(RADIUS_MD),
            },
            ..Default::default()
        }
    }
}

/// 2 px accent top edge drawn above the active tab.
pub fn tab_edge(active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: Some(
            if active {
                palette(theme).accent
            } else {
                Color::TRANSPARENT
            }
            .into(),
        ),
        border: Border::default().rounded(top_radius(RADIUS_MD)),
        ..Default::default()
    }
}

/// Tab strip background.
pub fn tab_bar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(p.bg_surface.into()),
        ..Default::default()
    }
}

/// Terminal / code surface.
pub fn terminal_surface(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(palette(theme).bg_terminal.into()),
        ..Default::default()
    }
}

/// Connection / state dot. Filled = live, hollow = not live (red ring failed, grey ring idle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Live,
    Connecting,
    Failed,
    Idle,
    Accent,
}

pub fn dot(kind: Dot, size: f32) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = palette(theme);
        let (fill, ring) = match kind {
            Dot::Live => (Some(p.success), p.success),
            Dot::Connecting => (Some(p.warning), p.warning),
            Dot::Accent => (Some(p.accent), p.accent),
            Dot::Failed => (None, p.error),
            Dot::Idle => (None, p.text_faint),
        };
        container::Style {
            background: fill.map(Background::from),
            border: Border {
                color: ring,
                width: 1.0,
                radius: (size / 2.0).into(),
            },
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------- text

pub fn text_primary(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).text_primary),
    }
}
pub fn text_secondary(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).text_secondary),
    }
}
pub fn text_muted(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).text_muted),
    }
}
pub fn text_faint(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).text_faint),
    }
}
pub fn text_accent(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).accent),
    }
}
pub fn text_success(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).success),
    }
}
pub fn text_warning(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).warning),
    }
}
pub fn text_error(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(palette(theme).error),
    }
}
/// Colour from a closure over the palette, for one-off tokens (e.g. mode colours).
pub fn text_color(pick: fn(&super::Palette) -> Color) -> impl Fn(&Theme) -> text::Style {
    move |theme| text::Style {
        color: Some(pick(palette(theme))),
    }
}

// -------------------------------------------------------------------- inputs

/// Text input: raised fill, 1 px strong border; focus = accent border + glow; error = error border.
pub fn input(error: bool) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |theme, status| {
        let p = palette(theme);
        let focused = matches!(status, text_input::Status::Focused { .. });
        let disabled = matches!(status, text_input::Status::Disabled);
        let line = if error {
            p.error
        } else if focused {
            p.accent
        } else {
            p.border_strong
        };
        text_input::Style {
            background: if disabled { p.bg_base } else { p.bg_raised }.into(),
            border: border(line, RADIUS_MD),
            icon: p.text_muted,
            placeholder: p.text_faint,
            value: if disabled {
                p.text_muted
            } else {
                p.text_primary
            },
            selection: p.selection_term,
        }
    }
}

// ------------------------------------------------------------------- buttons

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// ACCENT / ON_ACCENT. One per pane.
    Primary,
    /// BG_RAISED / BORDER_STRONG.
    Secondary,
    /// Transparent.
    Ghost,
    /// ERROR_BG / ERROR_BORDER / ERROR.
    Danger,
}

pub fn button(kind: ButtonKind) -> impl Fn(&Theme, btn::Status) -> btn::Style {
    move |theme, status| {
        let p = palette(theme);
        let hovered = matches!(status, btn::Status::Hovered | btn::Status::Pressed);
        let disabled = matches!(status, btn::Status::Disabled);
        let (bg, fg, line) = match kind {
            ButtonKind::Primary => (
                Some(if hovered { p.accent_hover } else { p.accent }),
                p.on_accent,
                Color::TRANSPARENT,
            ),
            ButtonKind::Secondary => (
                Some(if hovered { p.bg_hover } else { p.bg_raised }),
                p.text_primary,
                p.border_strong,
            ),
            ButtonKind::Ghost => (
                hovered.then_some(p.bg_hover),
                p.text_secondary,
                Color::TRANSPARENT,
            ),
            ButtonKind::Danger => (Some(p.error_bg), p.error, p.error_border),
        };
        let (bg, fg) = if disabled {
            (Some(p.bg_raised), p.text_faint)
        } else {
            (bg, fg)
        };
        btn::Style {
            background: bg.map(Background::from),
            text_color: fg,
            border: border(line, RADIUS_MD),
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

/// Focused variant of a button: accent border + glow.
pub fn button_focused(kind: ButtonKind) -> impl Fn(&Theme, btn::Status) -> btn::Style {
    let base = button(kind);
    move |theme, status| {
        let p = palette(theme);
        let mut s = base(theme, status);
        s.border = border(p.accent, RADIUS_MD);
        s.shadow = glow(p.focus_ring);
        s
    }
}

/// Toggle a flat transparent button used for clickable rows (hover fill only).
pub fn row_button(theme: &Theme, status: btn::Status) -> btn::Style {
    button(ButtonKind::Ghost)(theme, status)
}

/// Focus glow shadow for custom focus rings.
pub fn focus_glow(theme: &Theme) -> Shadow {
    glow(palette(theme).focus_ring)
}
