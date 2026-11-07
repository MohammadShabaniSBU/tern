//! Outline icons from the designs (Lucide-style, `currentColor` stroke), embedded as SVG and
//! tinted through the theme palette.

use iced::widget::svg;
use iced::{Color, Element, Length, Theme};

use crate::theme::{Palette, palette};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Server,
    Key,
    Tunnel,
    Lock,
    LockOpen,
    Search,
    ChevronDown,
    ChevronRight,
    Folder,
    Plug,
    Pencil,
    Plus,
    AlertCircle,
    Check,
}

impl Icon {
    #[cfg(test)]
    const ALL: [Icon; 14] = [
        Icon::Server,
        Icon::Key,
        Icon::Tunnel,
        Icon::Lock,
        Icon::LockOpen,
        Icon::Search,
        Icon::ChevronDown,
        Icon::ChevronRight,
        Icon::Folder,
        Icon::Plug,
        Icon::Pencil,
        Icon::Plus,
        Icon::AlertCircle,
        Icon::Check,
    ];

    fn bytes(self) -> &'static [u8] {
        match self {
            Icon::Server => include_bytes!("../assets/icons/server.svg"),
            Icon::Key => include_bytes!("../assets/icons/key.svg"),
            Icon::Tunnel => include_bytes!("../assets/icons/tunnel.svg"),
            Icon::Lock => include_bytes!("../assets/icons/lock.svg"),
            Icon::LockOpen => include_bytes!("../assets/icons/lock-open.svg"),
            Icon::Search => include_bytes!("../assets/icons/search.svg"),
            Icon::ChevronDown => include_bytes!("../assets/icons/chevron-down.svg"),
            Icon::ChevronRight => include_bytes!("../assets/icons/chevron-right.svg"),
            Icon::Folder => include_bytes!("../assets/icons/folder.svg"),
            Icon::Plug => include_bytes!("../assets/icons/plug.svg"),
            Icon::Pencil => include_bytes!("../assets/icons/pencil.svg"),
            Icon::Plus => include_bytes!("../assets/icons/plus.svg"),
            Icon::AlertCircle => include_bytes!("../assets/icons/alert-circle.svg"),
            Icon::Check => include_bytes!("../assets/icons/check.svg"),
        }
    }
}

/// A square icon of `size` px, coloured from the palette by `pick`.
pub fn icon<'a, M: 'a>(icon: Icon, size: f32, pick: fn(&Palette) -> Color) -> Element<'a, M> {
    svg(svg::Handle::from_memory(icon.bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |theme: &Theme, _| svg::Style {
            color: Some(pick(palette(theme))),
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_an_svg_document() {
        for i in Icon::ALL {
            let s = std::str::from_utf8(i.bytes()).expect("utf8");
            assert!(
                s.starts_with("<svg") && s.trim_end().ends_with("</svg>"),
                "{i:?}"
            );
        }
    }
}
