//! Keyboard-focusable select field for forms.
//!
//! iced's `pick_list` can neither take focus nor be driven from the keyboard. [`Select`] wraps a
//! `pick_list` (used only to draw the closed field) and adds:
//! - a `Focusable` state, so `focus_next` / `focus_previous` visit it in view order;
//! - Space / Enter while focused opens our own drop-down (an overlay, so it floats like the
//!   native one); Up / Down while closed pick the previous / next option;
//! - while open: Up/Down or k/j move the highlight, Enter/Space choose, Esc or a click away closes,
//!   Tab closes and moves on; the mouse hovers and clicks as usual.

use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{
    Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer,
};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::pick_list;
use iced::{
    Background, Border, Color, Element, Event, Fill, Length, Point, Rectangle, Size, Theme,
};
use iced::{Renderer, Vector, alignment};

use crate::app::Message;
use crate::theme::tokens::*;

/// What a form passes in: `labels[i]` is shown, `picks[i]` is sent when option `i` is chosen.
pub struct SelectSpec {
    pub id: &'static str,
    pub labels: Vec<String>,
    pub current: usize,
    pub picks: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Label(String, usize);

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn view<'a>(spec: SelectSpec) -> Element<'a, Message> {
    let labels = spec.labels;
    let options: Vec<Label> = labels
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, l)| Label(l, i))
        .collect();
    let selected = options.get(spec.current).cloned();
    let picks = spec.picks.clone();
    let inner = pick_list(options, selected, move |l: Label| {
        picks.get(l.1).cloned().unwrap_or(Message::Noop)
    })
    .text_size(14)
    .width(Fill);
    Element::new(Select {
        id: iced::widget::Id::new(spec.id),
        inner: inner.into(),
        labels,
        current: spec.current,
        picks: spec.picks,
    })
}

struct Select<'a> {
    id: iced::widget::Id,
    inner: Element<'a, Message>,
    labels: Vec<String>,
    current: usize,
    picks: Vec<Message>,
}

#[derive(Debug, Default)]
struct State {
    focused: bool,
    open: bool,
    highlight: usize,
    /// Pixel offset of the option list inside the menu.
    scroll: f32,
    /// Scroll the highlighted row into view on the next layout.
    reveal: bool,
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.focused = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
    }
}

impl<'a> Widget<Message, Theme, Renderer> for Select<'a> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.inner)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.inner));
    }

    fn size(&self) -> Size<Length> {
        self.inner.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.inner
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();
        operation.focusable(Some(&self.id), layout.bounds(), state);
        self.inner
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let (focused, open) = {
            let s = tree.state.downcast_ref::<State>();
            (s.focused, s.open)
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let s = tree.state.downcast_mut::<State>();
                s.focused = cursor.is_over(bounds);
                if s.focused {
                    s.open = true;
                    s.highlight = self.current;
                    s.scroll = 0.0;
                    s.reveal = true;
                    shell.capture_event();
                    return;
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if focused && modifiers.is_empty() && !open =>
            {
                match key.as_ref() {
                    Key::Named(Named::Space | Named::Enter) => {
                        let s = tree.state.downcast_mut::<State>();
                        s.open = true;
                        s.highlight = self.current;
                        s.scroll = 0.0;
                        s.reveal = true;
                        shell.capture_event();
                        return;
                    }
                    Key::Named(Named::ArrowUp) if self.current > 0 => {
                        if let Some(m) = self.picks.get(self.current - 1) {
                            shell.publish(m.clone());
                        }
                        shell.capture_event();
                        return;
                    }
                    Key::Named(Named::ArrowDown) => {
                        if let Some(m) = self.picks.get(self.current + 1) {
                            shell.publish(m.clone());
                        }
                        shell.capture_event();
                        return;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.inner.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.inner.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.inner.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if tree.state.downcast_ref::<State>().focused {
            use iced::advanced::Renderer as _;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: Border {
                        color: crate::theme::palette(theme).accent,
                        width: 1.0,
                        radius: RADIUS_MD.into(),
                    },
                    shadow: crate::theme::style::focus_glow(theme),
                    ..renderer::Quad::default()
                },
                Background::Color(Color::TRANSPARENT),
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.open {
            return None;
        }
        let field = layout.bounds() + translation;
        Some(overlay::Element::new(Box::new(Menu {
            state,
            labels: &self.labels,
            picks: &self.picks,
            field,
        })))
    }
}

const ROW_H: f32 = 28.0;
const MENU_PAD: f32 = 4.0;
/// The menu never grows past this many rows; longer lists scroll.
const MAX_VISIBLE_ROWS: f32 = 8.0;
const BAR_W: f32 = 4.0;

/// Scroll offset that brings `row` fully into a viewport of height `viewport_h`.
fn reveal(scroll: f32, row: usize, viewport_h: f32) -> f32 {
    let top = row as f32 * ROW_H;
    let bottom = top + ROW_H;
    if top < scroll {
        top
    } else if bottom > scroll + viewport_h {
        bottom - viewport_h
    } else {
        scroll
    }
}

fn clamp_scroll(scroll: f32, content: f32, viewport_h: f32) -> f32 {
    scroll.clamp(0.0, (content - viewport_h).max(0.0))
}

/// Scrollbar thumb as `(offset from track top, height)`; `None` when everything fits.
fn thumb(scroll: f32, content: f32, viewport_h: f32) -> Option<(f32, f32)> {
    if content <= viewport_h {
        return None;
    }
    let h = (viewport_h * viewport_h / content)
        .max(16.0)
        .min(viewport_h);
    let max_scroll = content - viewport_h;
    Some(((viewport_h - h) * (scroll / max_scroll), h))
}

/// The open option list.
struct Menu<'b> {
    state: &'b mut State,
    labels: &'b [String],
    picks: &'b [Message],
    /// The select field, in window coordinates.
    field: Rectangle,
}

impl Menu<'_> {
    fn row_at(&self, bounds: Rectangle, p: Point) -> Option<usize> {
        let y = p.y - bounds.y - MENU_PAD + self.state.scroll;
        (bounds.contains(p) && y >= 0.0)
            .then(|| (y / ROW_H) as usize)
            .filter(|i| *i < self.labels.len())
    }

    fn step(&mut self, delta: i32) {
        let len = self.labels.len() as i32;
        if len > 0 {
            self.state.highlight = (self.state.highlight as i32 + delta).rem_euclid(len) as usize;
            self.state.reveal = true;
        }
    }

    fn choose(&mut self, shell: &mut Shell<'_, Message>) {
        if let Some(m) = self.picks.get(self.state.highlight) {
            shell.publish(m.clone());
        }
        self.state.open = false;
    }
}

impl overlay::Overlay<Message, Theme, Renderer> for Menu<'_> {
    fn layout(&mut self, _renderer: &Renderer, bounds: Size) -> layout::Node {
        let content = self.labels.len() as f32 * ROW_H;
        let want = content.min(MAX_VISIBLE_ROWS * ROW_H) + 2.0 * MENU_PAD;
        let below = self.field.y + self.field.height + 2.0;
        let room_below = bounds.height - below;
        let room_above = self.field.y - 2.0;
        // Prefer below; flip above when it does not fit there; otherwise shrink to the roomier side.
        let (height, y) = if want <= room_below {
            (want, below)
        } else if want <= room_above {
            (want, self.field.y - 2.0 - want)
        } else if room_below >= room_above {
            (room_below.max(ROW_H + 2.0 * MENU_PAD), below)
        } else {
            let h = room_above.max(ROW_H + 2.0 * MENU_PAD);
            (h, self.field.y - 2.0 - h)
        };
        let viewport_h = height - 2.0 * MENU_PAD;
        if self.state.reveal {
            self.state.reveal = false;
            self.state.scroll = reveal(self.state.scroll, self.state.highlight, viewport_h);
        }
        self.state.scroll = clamp_scroll(self.state.scroll, content, viewport_h);
        layout::Node::new(Size::new(self.field.width, height)).move_to(Point::new(self.field.x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let p = crate::theme::palette(theme);
        let bounds = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: p.border_strong,
                    width: 1.0,
                    radius: RADIUS_MD.into(),
                },
                ..renderer::Quad::default()
            },
            Background::Color(p.bg_overlay),
        );
        let hovered = cursor.position().and_then(|c| self.row_at(bounds, c));
        let inner = Rectangle {
            x: bounds.x,
            y: bounds.y + MENU_PAD,
            width: bounds.width,
            height: bounds.height - 2.0 * MENU_PAD,
        };
        let content = self.labels.len() as f32 * ROW_H;
        let bar = thumb(self.state.scroll, content, inner.height);
        let text_w =
            bounds.width - 2.0 * MENU_PAD - 12.0 - if bar.is_some() { BAR_W + 4.0 } else { 0.0 };
        renderer.with_layer(inner, |renderer| {
            for (i, label) in self.labels.iter().enumerate() {
                let row = Rectangle {
                    x: bounds.x + MENU_PAD,
                    y: inner.y + i as f32 * ROW_H - self.state.scroll,
                    width: bounds.width - 2.0 * MENU_PAD,
                    height: ROW_H,
                };
                if row.y + row.height < inner.y || row.y > inner.y + inner.height {
                    continue;
                }
                let lit = i == self.state.highlight || hovered == Some(i);
                if lit {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: row,
                            border: Border::default().rounded(RADIUS_MD),
                            ..renderer::Quad::default()
                        },
                        Background::Color(p.selection_palette),
                    );
                }
                renderer.fill_text(
                    text::Text {
                        content: label.clone(),
                        bounds: Size::new(text_w, row.height),
                        size: TEXT_LABEL.into(),
                        line_height: text::LineHeight::default(),
                        font: renderer.default_font(),
                        align_x: text::Alignment::Default,
                        align_y: alignment::Vertical::Center,
                        shaping: text::Shaping::Basic,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(row.x + 6.0, row.center_y()),
                    p.text_primary,
                    row.intersection(&inner).unwrap_or(row),
                );
            }
        });
        if let Some((off, h)) = bar {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + bounds.width - BAR_W - 3.0,
                        y: inner.y + off,
                        width: BAR_W,
                        height: h,
                    },
                    border: Border::default().rounded(BAR_W / 2.0),
                    ..renderer::Quad::default()
                },
                Background::Color(p.border_strong),
            );
        }
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let bounds = layout.bounds();
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if modifiers.is_empty() =>
            {
                match key.as_ref() {
                    Key::Named(Named::ArrowDown) | Key::Character("j") => self.step(1),
                    Key::Named(Named::ArrowUp) | Key::Character("k") => self.step(-1),
                    Key::Named(Named::Enter | Named::Space) => self.choose(shell),
                    Key::Named(Named::Escape) => {
                        self.state.open = false;
                        shell.publish(Message::SelectEscaped);
                    }
                    Key::Named(Named::Tab) => {
                        // Close, and let the key through so focus moves on.
                        self.state.open = false;
                        return;
                    }
                    _ => {}
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * ROW_H,
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                let content = self.labels.len() as f32 * ROW_H;
                self.state.scroll = clamp_scroll(
                    self.state.scroll + dy,
                    content,
                    bounds.height - 2.0 * MENU_PAD,
                );
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(i) = self.row_at(bounds, *position) {
                    self.state.highlight = i;
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(i) = cursor.position().and_then(|c| self.row_at(bounds, c)) {
                    self.state.highlight = i;
                    self.choose(shell);
                } else {
                    self.state.open = false;
                }
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_scrolls_down_and_up() {
        let vp = 8.0 * ROW_H;
        assert_eq!(reveal(0.0, 3, vp), 0.0);
        assert_eq!(reveal(0.0, 10, vp), 11.0 * ROW_H - vp);
        assert_eq!(reveal(5.0 * ROW_H, 2, vp), 2.0 * ROW_H);
    }

    #[test]
    fn clamp_and_thumb() {
        let vp = 8.0 * ROW_H;
        assert_eq!(clamp_scroll(999.0, 32.0 * ROW_H, vp), 24.0 * ROW_H);
        assert_eq!(clamp_scroll(-5.0, 32.0 * ROW_H, vp), 0.0);
        assert_eq!(thumb(0.0, 5.0 * ROW_H, vp), None);
        let (off, h) = thumb(24.0 * ROW_H, 32.0 * ROW_H, vp).unwrap();
        assert!((off + h - vp).abs() < 0.001);
    }
}
