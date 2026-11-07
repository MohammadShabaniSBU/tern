//! Composite widgets shared by modal screens.

use iced::widget::container;
use iced::{Element, Fill};

use super::style;
use super::tokens::*;

/// A centred modal card of `width` over the scrim (docs "Modal").
pub fn dialog<'a, M: 'a>(body: impl Into<Element<'a, M>>, width: f32) -> Element<'a, M> {
    container(
        container(body)
            .padding(SPACE_20)
            .max_width(width)
            .style(style::modal_card),
    )
    .center(Fill)
    .style(style::scrim)
    .into()
}

/// A card pinned near the top (command palette).
pub fn top_dialog<'a, M: 'a>(body: impl Into<Element<'a, M>>, width: f32) -> Element<'a, M> {
    container(
        container(body)
            .padding(SPACE_8)
            .width(width)
            .style(style::modal_card),
    )
    .center_x(Fill)
    .padding(iced::Padding::ZERO.top(80))
    .height(Fill)
    .style(style::scrim)
    .into()
}
