//! iced canvas widget that draws an [`Emulator`] snapshot.

use std::time::{Duration, Instant};

use iced::advanced::mouse::{Click, click};
use iced::mouse::{self, Button, ScrollDelta};
use iced::widget::canvas::{self, Action, Cache, Canvas, Frame, Geometry, Stroke, Text};
use iced::{Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer, Size, Theme};
use iced::{alignment, font, window};

use crate::emu::{CellFlags, CursorShape, SelectKind, Snapshot};
use crate::theme::Scheme;

/// JetBrains Mono metrics (units per em = 1000): advance 600, ascent+descent 1320.
const ADVANCE_EM: f32 = 0.6;
const LINE_EM: f32 = 1.32;

/// Pixel size of one terminal cell for a font size.
pub fn cell_size(font_size: f32) -> Size {
    Size::new(
        (font_size * ADVANCE_EM * 100.0).round() / 100.0,
        (font_size * LINE_EM).round(),
    )
}

/// Padding around the grid, in pixels.
pub const PADDING: f32 = 4.0;

/// Columns/rows that fit in a widget of `bounds`.
pub fn grid_for(bounds: Size, font_size: f32) -> (usize, usize) {
    let cell = cell_size(font_size);
    let w = (bounds.width - 2.0 * PADDING).max(0.0);
    let h = (bounds.height - 2.0 * PADDING).max(0.0);
    (
        ((w / cell.width) as usize).max(2),
        ((h / cell.height) as usize).max(1),
    )
}

/// What the widget reports to the app.
#[derive(Debug, Clone, PartialEq)]
pub enum TermEvent {
    /// Widget size changed; new grid dimensions.
    Resized {
        cols: usize,
        rows: usize,
    },
    /// Wheel scroll in lines; positive = toward older output.
    Scroll(i32),
    SelectStart {
        kind: SelectKind,
        row: usize,
        col: usize,
    },
    SelectDrag {
        row: usize,
        col: usize,
    },
    SelectEnd,
}

/// Everything the widget needs to draw one frame. Borrowed from the app state.
pub struct TerminalView<'a, Message> {
    pub snapshot: &'a Snapshot,
    pub cache: &'a Cache,
    pub scheme: Scheme,
    pub font: Font,
    pub font_size: f32,
    /// Cursor blink phase (true = drawn).
    pub cursor_on: bool,
    pub focused: bool,
    /// Brief inverse flash (visual bell).
    pub flash: bool,
    /// Overrides the cursor shape requested by the remote.
    pub cursor_shape: Option<CursorShape>,
    pub on_event: Box<dyn Fn(TermEvent) -> Message + 'a>,
}

impl<'a, Message: 'a> TerminalView<'a, Message> {
    pub fn into_element(self) -> Element<'a, Message> {
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn pos_to_cell(&self, bounds: Rectangle, p: Point) -> (usize, usize) {
        let cell = cell_size(self.font_size);
        let col = ((p.x - bounds.x - PADDING) / cell.width).floor().max(0.0) as usize;
        let row = ((p.y - bounds.y - PADDING) / cell.height).floor().max(0.0) as usize;
        (
            row.min(self.snapshot.rows.saturating_sub(1)),
            col.min(self.snapshot.cols.saturating_sub(1)),
        )
    }
}

#[derive(Default)]
pub struct State {
    pending: Option<((usize, usize), (usize, usize))>,
    dragging: bool,
    last_click: Option<Click>,
    wheel_remainder: f32,
}

impl<Message> canvas::Program<Message> for TerminalView<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Window(window::Event::RedrawRequested(_)) => {
                let dims = grid_for(bounds.size(), self.font_size);
                let current = (self.snapshot.cols, self.snapshot.rows);
                if dims == current {
                    state.pending = None;
                    return None;
                }
                // Ask once per (target, current) pair so repeated redraws don't spam.
                if state.pending == Some((dims, current)) {
                    return None;
                }
                state.pending = Some((dims, current));
                Some(Action::publish((self.on_event)(TermEvent::Resized {
                    cols: dims.0,
                    rows: dims.1,
                })))
            }
            Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let abs = Point::new(p.x + bounds.x, p.y + bounds.y);
                let click = Click::new(abs, Button::Left, state.last_click);
                let kind = match click.kind() {
                    click::Kind::Single => SelectKind::Simple,
                    click::Kind::Double => SelectKind::Word,
                    click::Kind::Triple => SelectKind::Line,
                };
                state.last_click = Some(click);
                state.dragging = true;
                let (row, col) = self.pos_to_cell(bounds, abs);
                Some(
                    Action::publish((self.on_event)(TermEvent::SelectStart { kind, row, col }))
                        .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                let p = cursor.position()?;
                let (row, col) = self.pos_to_cell(bounds, p);
                Some(Action::publish((self.on_event)(TermEvent::SelectDrag {
                    row,
                    col,
                })))
            }
            Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) if state.dragging => {
                state.dragging = false;
                Some(Action::publish((self.on_event)(TermEvent::SelectEnd)))
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match *delta {
                    ScrollDelta::Lines { y, .. } => y * 3.0,
                    ScrollDelta::Pixels { y, .. } => y / cell_size(self.font_size).height,
                };
                state.wheel_remainder += lines;
                let whole = state.wheel_remainder.trunc();
                state.wheel_remainder -= whole;
                if whole == 0.0 {
                    return Some(Action::capture());
                }
                Some(
                    Action::publish((self.on_event)(TermEvent::Scroll(whole as i32))).and_capture(),
                )
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            self.paint(frame, bounds.size());
        });
        // The cursor is drawn outside the cache so blinking doesn't invalidate the grid.
        let mut overlay = Frame::new(renderer, bounds.size());
        self.paint_cursor(&mut overlay);
        vec![geometry, overlay.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<Message> TerminalView<'_, Message> {
    fn paint(&self, frame: &mut Frame, size: Size) {
        let snap = self.snapshot;
        let cell = cell_size(self.font_size);
        let sc = &self.scheme;
        let (bg, fg) = if self.flash {
            (sc.fg(), sc.bg())
        } else {
            (sc.bg(), sc.fg())
        };
        let _ = fg;
        frame.fill_rectangle(Point::ORIGIN, size, bg);

        let x_of = |col: usize| PADDING + col as f32 * cell.width;
        let y_of = |row: usize| PADDING + row as f32 * cell.height;

        for row in 0..snap.rows {
            // Backgrounds: merge horizontal runs of the same colour.
            let mut run: Option<(usize, iced::Color)> = None;
            let flush = |frame: &mut Frame, start: usize, end: usize, color: iced::Color| {
                frame.fill_rectangle(
                    Point::new(x_of(start), y_of(row)),
                    Size::new((end - start) as f32 * cell.width, cell.height),
                    color,
                );
            };
            for col in 0..snap.cols {
                let Some(c) = snap.cell(row, col) else {
                    continue;
                };
                let (_, cbg) = cell_colors(sc, c.fg, c.bg, c.flags);
                let default_bg = cbg == sc.bg() && !self.flash;
                match (&mut run, default_bg) {
                    (Some((_, color)), false) if *color == cbg => {}
                    (Some((s, color)), _) => {
                        flush(frame, *s, col, *color);
                        run = (!default_bg).then_some((col, cbg));
                    }
                    (None, false) => run = Some((col, cbg)),
                    (None, true) => {}
                }
            }
            if let Some((s, color)) = run {
                flush(frame, s, snap.cols, color);
            }

            // Selection and search highlights.
            for m in &snap.matches {
                if m.row == row {
                    let color = if snap.current_match == Some(*m) {
                        sc.search_current()
                    } else {
                        sc.search()
                    };
                    highlight(
                        frame,
                        x_of(m.col_start),
                        y_of(row),
                        m.col_end - m.col_start + 1,
                        cell,
                        color,
                    );
                }
            }
            for s in &snap.selection {
                if s.row == row {
                    highlight(
                        frame,
                        x_of(s.col_start),
                        y_of(row),
                        s.col_end - s.col_start + 1,
                        cell,
                        sc.selection(),
                    );
                }
            }

            // Glyphs: batch consecutive narrow cells with identical style.
            let mut text = String::new();
            let mut style: Option<(iced::Color, CellFlags)> = None;
            let mut start = 0;
            let emit = |frame: &mut Frame,
                        text: &mut String,
                        style: Option<(iced::Color, CellFlags)>,
                        start: usize| {
                if let Some((color, flags)) = style
                    && text.chars().any(|c| c != ' ')
                {
                    self.draw_text(
                        frame,
                        std::mem::take(text),
                        x_of(start),
                        y_of(row),
                        color,
                        flags,
                    );
                }
                text.clear();
            };
            for col in 0..snap.cols {
                let Some(c) = snap.cell(row, col) else {
                    continue;
                };
                if c.flags.spacer {
                    continue;
                }
                let (cfg, _) = cell_colors(sc, c.fg, c.bg, c.flags);
                let visible = !c.flags.hidden && c.ch != '\0';
                let ch = if visible { c.ch } else { ' ' };
                let this = (cfg, c.flags);
                if c.flags.wide {
                    emit(frame, &mut text, style.take(), start);
                    self.draw_text(frame, ch.to_string(), x_of(col), y_of(row), cfg, c.flags);
                    continue;
                }
                match style {
                    Some(s) if s == this => text.push(ch),
                    _ => {
                        emit(frame, &mut text, style.take(), start);
                        style = Some(this);
                        start = col;
                        text.push(ch);
                    }
                }
            }
            emit(frame, &mut text, style.take(), start);

            // Underline / strikeout decorations.
            for col in 0..snap.cols {
                let Some(c) = snap.cell(row, col) else {
                    continue;
                };
                if c.flags.spacer || !(c.flags.underline || c.flags.strikeout) {
                    continue;
                }
                let (cfg, _) = cell_colors(sc, c.fg, c.bg, c.flags);
                let w = if c.flags.wide {
                    2.0 * cell.width
                } else {
                    cell.width
                };
                if c.flags.underline {
                    frame.fill_rectangle(
                        Point::new(x_of(col), y_of(row) + cell.height - 2.0),
                        Size::new(w, 1.0),
                        cfg,
                    );
                }
                if c.flags.strikeout {
                    frame.fill_rectangle(
                        Point::new(x_of(col), y_of(row) + cell.height / 2.0),
                        Size::new(w, 1.0),
                        cfg,
                    );
                }
            }
        }
    }

    fn draw_text(
        &self,
        frame: &mut Frame,
        content: String,
        x: f32,
        y: f32,
        color: iced::Color,
        flags: CellFlags,
    ) {
        let color = if flags.dim {
            iced::Color {
                a: color.a * 0.6,
                ..color
            }
        } else {
            color
        };
        let mut font = self.font;
        if flags.bold {
            font.weight = font::Weight::Bold;
        }
        if flags.italic {
            font.style = font::Style::Italic;
        }
        frame.fill_text(Text {
            content,
            position: Point::new(x, y),
            color,
            size: Pixels(self.font_size),
            line_height: iced::widget::text::LineHeight::Absolute(Pixels(
                cell_size(self.font_size).height,
            )),
            font,
            align_x: iced::advanced::text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            // Advanced shaping is what applies the font's ligatures (`calt`), like Ghostty does.
            shaping: iced::widget::text::Shaping::Advanced,
            ..Text::default()
        });
    }

    fn paint_cursor(&self, frame: &mut Frame) {
        let snap = self.snapshot;
        let Some(cur) = snap.cursor else { return };
        let cell = cell_size(self.font_size);
        let x = PADDING + cur.col as f32 * cell.width;
        let y = PADDING + cur.row as f32 * cell.height;
        let color = self.scheme.cursor();
        if !self.focused {
            // Hollow block when the terminal isn't focused.
            frame.stroke_rectangle(
                Point::new(x + 0.5, y + 0.5),
                Size::new(cell.width - 1.0, cell.height - 1.0),
                Stroke::default().with_color(color).with_width(1.0),
            );
            return;
        }
        if !snap.copy_mode && snap.cursor_blinking && !self.cursor_on {
            return;
        }
        match self
            .cursor_shape
            .filter(|_| !snap.copy_mode)
            .unwrap_or(cur.shape)
        {
            CursorShape::Block => {
                frame.fill_rectangle(Point::new(x, y), cell, color);
                if let Some(c) = snap.cell(cur.row, cur.col)
                    && c.ch != ' '
                    && c.ch != '\0'
                {
                    self.draw_text(frame, c.ch.to_string(), x, y, self.scheme.bg(), c.flags);
                }
            }
            CursorShape::Underline => {
                frame.fill_rectangle(
                    Point::new(x, y + cell.height - 2.0),
                    Size::new(cell.width, 2.0),
                    color,
                );
            }
            CursorShape::Bar => {
                frame.fill_rectangle(Point::new(x, y), Size::new(2.0, cell.height), color);
            }
        }
    }
}

fn highlight(frame: &mut Frame, x: f32, y: f32, cols: usize, cell: Size, color: iced::Color) {
    frame.fill_rectangle(
        Point::new(x, y),
        Size::new(cols as f32 * cell.width, cell.height),
        color,
    );
}

/// `(foreground, background)` after inverse handling.
fn cell_colors(
    sc: &Scheme,
    fg: crate::emu::Color,
    bg: crate::emu::Color,
    flags: CellFlags,
) -> (iced::Color, iced::Color) {
    let (f, b) = (sc.resolve_color(fg), sc.resolve_color(bg));
    if flags.inverse { (b, f) } else { (f, b) }
}

/// Blink half-period.
pub const BLINK: Duration = Duration::from_millis(530);

/// Helper for the app's blink phase.
pub fn blink_phase(start: Instant, now: Instant) -> bool {
    (now.duration_since(start).as_millis() / BLINK.as_millis()).is_multiple_of(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_from_bounds() {
        let (cols, rows) = grid_for(Size::new(808.0, 424.0), 14.0);
        let cell = cell_size(14.0);
        assert_eq!(cols, ((808.0 - 8.0) / cell.width) as usize);
        assert_eq!(rows, ((424.0 - 8.0) / cell.height) as usize);
        assert_eq!(grid_for(Size::new(0.0, 0.0), 14.0), (2, 1));
    }

    #[test]
    fn blink_alternates() {
        let t0 = Instant::now();
        assert!(blink_phase(t0, t0));
        assert!(!blink_phase(t0, t0 + BLINK));
        assert!(blink_phase(t0, t0 + BLINK * 2));
    }
}
