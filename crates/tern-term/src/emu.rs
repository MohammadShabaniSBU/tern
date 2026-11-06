//! Terminal emulation core: a thin adapter over `alacritty_terminal`.
//!
//! The rest of the app never sees alacritty types; it uses [`Emulator`] and the plain
//! [`Snapshot`] / [`Color`] types defined here.

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Direction, Line, Point};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as AColor, CursorShape as ACursorShape, NamedColor, Processor,
};

/// Cell colour before theme resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    DefaultFg,
    DefaultBg,
    /// xterm palette index 0..=255.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellFlags {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
    pub hidden: bool,
    pub strikeout: bool,
    /// Second half of a wide glyph; draw nothing.
    pub spacer: bool,
    pub wide: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub shape: CursorShape,
}

/// Terminal modes the UI cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modes {
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub alt_screen: bool,
    pub mouse_report: bool,
    pub sgr_mouse: bool,
    pub alternate_scroll: bool,
}

/// A match or selection in viewport coordinates: inclusive `(row, col)` span on one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSpan {
    pub row: usize,
    pub col_start: usize,
    pub col_end: usize,
}

/// Everything a renderer needs for one frame.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Cell>,
    /// `None` when hidden or scrolled out of view.
    pub cursor: Option<Cursor>,
    pub cursor_blinking: bool,
    pub display_offset: usize,
    pub history: usize,
    pub modes: Modes,
    pub selection: Vec<RowSpan>,
    pub matches: Vec<RowSpan>,
    pub current_match: Option<RowSpan>,
}

impl Snapshot {
    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        if row < self.rows && col < self.cols {
            self.cells.get(row * self.cols + col)
        } else {
            None
        }
    }

    /// Text of a viewport row, trailing blanks trimmed.
    pub fn row_text(&self, row: usize) -> String {
        let mut s = String::new();
        for col in 0..self.cols {
            if let Some(c) = self.cell(row, col)
                && !c.flags.spacer
            {
                s.push(c.ch);
            }
        }
        s.trim_end().to_string()
    }
}

#[derive(Default)]
struct Shared {
    replies: Vec<u8>,
    title: Option<String>,
    bell: bool,
}

#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Shared>>);

impl Listener {
    fn with<R>(&self, f: impl FnOnce(&mut Shared) -> R) -> R {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.with(|s| match event {
            Event::PtyWrite(text) => s.replies.extend_from_slice(text.as_bytes()),
            Event::Title(t) => s.title = Some(t),
            Event::ResetTitle => s.title = None,
            Event::Bell => s.bell = true,
            _ => {}
        });
    }
}

#[derive(Clone, Copy)]
struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// How a mouse selection grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKind {
    Simple,
    Word,
    Line,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchState {
    query: String,
    /// Grid coordinates: (line, col_start, col_end).
    matches: Vec<(i32, usize, usize)>,
    current: Option<usize>,
}

pub struct Emulator {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    size: Size,
    search: Option<SearchState>,
}

const MIN_COLS: usize = 2;
const MIN_ROWS: usize = 1;

impl Emulator {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let size = Size {
            cols: cols.max(MIN_COLS),
            rows: rows.max(MIN_ROWS),
        };
        let listener = Listener::default();
        let config = Config {
            scrolling_history: scrollback,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, listener.clone()),
            parser: Processor::new(),
            listener,
            size,
            search: None,
        }
    }

    pub fn cols(&self) -> usize {
        self.size.cols
    }

    pub fn rows(&self) -> usize {
        self.size.rows
    }

    /// Feed remote output into the emulator.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Bytes the terminal wants written back to the remote (e.g. DA replies).
    pub fn take_replies(&mut self) -> Vec<u8> {
        self.listener.with(|s| std::mem::take(&mut s.replies))
    }

    /// Title set via OSC 0/2, if any.
    pub fn title(&self) -> Option<String> {
        self.listener.with(|s| s.title.clone())
    }

    /// Returns true once per bell.
    pub fn take_bell(&mut self) -> bool {
        self.listener.with(|s| std::mem::take(&mut s.bell))
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let size = Size {
            cols: cols.max(MIN_COLS),
            rows: rows.max(MIN_ROWS),
        };
        if size.cols == self.size.cols && size.rows == self.size.rows {
            return;
        }
        self.size = size;
        self.term.resize(size);
        self.search = None;
    }

    pub fn set_scrollback(&mut self, lines: usize) {
        let config = Config {
            scrolling_history: lines,
            ..Config::default()
        };
        self.term.set_options(config);
    }

    /// Scroll the viewport; positive moves toward older output.
    pub fn scroll(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
    }

    pub fn scroll_pages(&mut self, pages: i32) {
        let rows = i32::try_from(self.size.rows.saturating_sub(1).max(1)).unwrap_or(1);
        self.scroll(pages.saturating_mul(rows));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub fn scroll_to_top(&mut self) {
        self.term.scroll_display(Scroll::Top);
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn modes(&self) -> Modes {
        let m = self.term.mode();
        Modes {
            app_cursor: m.contains(TermMode::APP_CURSOR),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            mouse_report: m.intersects(TermMode::MOUSE_MODE),
            sgr_mouse: m.contains(TermMode::SGR_MOUSE),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
        }
    }

    fn grid_point(&self, row: usize, col: usize) -> Point {
        let off = i32::try_from(self.display_offset()).unwrap_or(0);
        let row = i32::try_from(row.min(self.size.rows - 1)).unwrap_or(0);
        Point::new(Line(row - off), Column(col.min(self.size.cols - 1)))
    }

    // ---- selection ----

    pub fn start_selection(&mut self, kind: SelectKind, row: usize, col: usize) {
        let ty = match kind {
            SelectKind::Simple => SelectionType::Simple,
            SelectKind::Word => SelectionType::Semantic,
            SelectKind::Line => SelectionType::Lines,
            SelectKind::Block => SelectionType::Block,
        };
        let p = self.grid_point(row, col);
        self.term.selection = Some(Selection::new(ty, p, Direction::Left));
    }

    pub fn update_selection(&mut self, row: usize, col: usize) {
        let p = self.grid_point(row, col);
        if let Some(sel) = self.term.selection.as_mut() {
            sel.update(p, Direction::Right);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub fn has_selection(&self) -> bool {
        self.term.selection.is_some()
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|s| !s.is_empty())
    }

    // ---- search ----

    /// Search all history + screen (case-insensitive literal). Returns the match count and
    /// selects the match nearest the bottom.
    pub fn search(&mut self, query: &str) -> usize {
        if query.is_empty() {
            self.search = None;
            return 0;
        }
        let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
        let grid = self.term.grid();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        let cols = self.size.cols;
        let mut matches = Vec::new();
        for line in top..=bottom {
            let row = &grid[Line(line)];
            // One char per column keeps indices aligned with columns.
            let text: Vec<char> = (0..cols)
                .map(|c| {
                    let cell = &row[Column(c)];
                    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        ' '
                    } else {
                        cell.c.to_lowercase().next().unwrap_or(' ')
                    }
                })
                .collect();
            if text.len() < needle.len() {
                continue;
            }
            let mut i = 0;
            while i + needle.len() <= text.len() {
                if text[i..i + needle.len()] == needle[..] {
                    matches.push((line, i, i + needle.len() - 1));
                    i += needle.len();
                } else {
                    i += 1;
                }
            }
        }
        let count = matches.len();
        let current = count.checked_sub(1);
        self.search = Some(SearchState {
            query: query.to_string(),
            matches,
            current,
        });
        self.reveal_current_match();
        count
    }

    pub fn search_query(&self) -> Option<&str> {
        self.search.as_ref().map(|s| s.query.as_str())
    }

    /// `(current index 1-based, total)`.
    pub fn search_status(&self) -> Option<(usize, usize)> {
        let s = self.search.as_ref()?;
        Some((s.current.map_or(0, |c| c + 1), s.matches.len()))
    }

    pub fn search_step(&mut self, forward: bool) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        let n = s.matches.len();
        if n == 0 {
            return;
        }
        s.current = Some(match (s.current, forward) {
            (None, _) => 0,
            (Some(c), true) => (c + 1) % n,
            (Some(c), false) => (c + n - 1) % n,
        });
        self.reveal_current_match();
    }

    pub fn clear_search(&mut self) {
        self.search = None;
    }

    fn reveal_current_match(&mut self) {
        let Some(&(line, _, _)) = self
            .search
            .as_ref()
            .and_then(|s| s.current.and_then(|c| s.matches.get(c)))
        else {
            return;
        };
        self.term.scroll_to_point(Point::new(Line(line), Column(0)));
    }

    // ---- snapshot ----

    pub fn snapshot(&self) -> Snapshot {
        let grid = self.term.grid();
        let (cols, rows) = (self.size.cols, self.size.rows);
        let off = grid.display_offset();
        let off_i = i32::try_from(off).unwrap_or(0);
        let mut cells = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            let line = Line(i32::try_from(r).unwrap_or(0) - off_i);
            let row = &grid[line];
            for c in 0..cols {
                cells.push(convert_cell(&row[Column(c)]));
            }
        }

        let modes = self.term.mode();
        let style = self.term.cursor_style();
        let cur = grid.cursor.point;
        let cur_row = i64::from(cur.line.0) + i64::from(off_i);
        let cursor = if modes.contains(TermMode::SHOW_CURSOR) && (0..rows as i64).contains(&cur_row)
        {
            Some(Cursor {
                col: cur.column.0.min(cols - 1),
                row: cur_row as usize,
                shape: match style.shape {
                    ACursorShape::Underline => CursorShape::Underline,
                    ACursorShape::Beam => CursorShape::Bar,
                    _ => CursorShape::Block,
                },
            })
        } else {
            None
        };

        let to_view = |line: i32| -> Option<usize> {
            let r = i64::from(line) + i64::from(off_i);
            (0..rows as i64).contains(&r).then_some(r as usize)
        };

        let mut selection = Vec::new();
        if let Some(range) = self.term.renderable_content().selection {
            for r in 0..rows {
                let line = Line(i32::try_from(r).unwrap_or(0) - off_i);
                let mut first = None;
                let mut last = 0;
                for c in 0..cols {
                    if range.contains(Point::new(line, Column(c))) {
                        first.get_or_insert(c);
                        last = c;
                    }
                }
                if let Some(col_start) = first {
                    selection.push(RowSpan {
                        row: r,
                        col_start,
                        col_end: last,
                    });
                }
            }
        }

        let (mut matches, mut current_match) = (Vec::new(), None);
        if let Some(s) = &self.search {
            for (i, &(line, a, b)) in s.matches.iter().enumerate() {
                if let Some(row) = to_view(line) {
                    let span = RowSpan {
                        row,
                        col_start: a,
                        col_end: b,
                    };
                    if s.current == Some(i) {
                        current_match = Some(span);
                    }
                    matches.push(span);
                }
            }
        }

        Snapshot {
            cols,
            rows,
            cells,
            cursor,
            cursor_blinking: style.blinking,
            display_offset: off,
            history: grid.history_size(),
            modes: self.modes(),
            selection,
            matches,
            current_match,
        }
    }
}

fn convert_color(c: AColor, default: Color) -> Color {
    match c {
        AColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        AColor::Indexed(i) => Color::Indexed(i),
        AColor::Named(n) => match n {
            NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => {
                Color::DefaultFg
            }
            NamedColor::Background => Color::DefaultBg,
            NamedColor::DimForeground => Color::DefaultFg,
            other => {
                let i = other as usize;
                match i {
                    0..=15 => Color::Indexed(i as u8),
                    // Dim variants (DimBlack = 260 ..= DimWhite) map onto the base 8 colours.
                    _ => dim_index(other).map_or(default, Color::Indexed),
                }
            }
        },
    }
}

fn dim_index(n: NamedColor) -> Option<u8> {
    Some(match n {
        NamedColor::DimBlack => 0,
        NamedColor::DimRed => 1,
        NamedColor::DimGreen => 2,
        NamedColor::DimYellow => 3,
        NamedColor::DimBlue => 4,
        NamedColor::DimMagenta => 5,
        NamedColor::DimCyan => 6,
        NamedColor::DimWhite => 7,
        _ => return None,
    })
}

fn convert_cell(c: &alacritty_terminal::term::cell::Cell) -> Cell {
    let f = c.flags;
    Cell {
        ch: c.c,
        fg: convert_color(c.fg, Color::DefaultFg),
        bg: convert_color(c.bg, Color::DefaultBg),
        flags: CellFlags {
            bold: f.contains(Flags::BOLD),
            italic: f.contains(Flags::ITALIC),
            underline: f.intersects(Flags::ALL_UNDERLINES),
            inverse: f.contains(Flags::INVERSE),
            dim: f.contains(Flags::DIM),
            hidden: f.contains(Flags::HIDDEN),
            strikeout: f.contains(Flags::STRIKEOUT),
            spacer: f.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
            wide: f.contains(Flags::WIDE_CHAR),
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn emu(cols: usize, rows: usize) -> Emulator {
        Emulator::new(cols, rows, 100)
    }

    #[test]
    fn plain_text_and_newlines() {
        let mut e = emu(20, 5);
        e.feed(b"hello\r\nworld");
        let s = e.snapshot();
        assert_eq!(s.row_text(0), "hello");
        assert_eq!(s.row_text(1), "world");
        assert_eq!(s.cursor.map(|c| (c.row, c.col)), Some((1, 5)));
    }

    #[test]
    fn sgr_colors_and_attrs() {
        let mut e = emu(20, 3);
        e.feed(b"\x1b[1;31mR\x1b[0m\x1b[38;2;1;2;3mT\x1b[0m\x1b[48;5;200;4mI\x1b[0m");
        let s = e.snapshot();
        let r = s.cell(0, 0).unwrap();
        assert_eq!(r.fg, Color::Indexed(1));
        assert!(r.flags.bold);
        assert_eq!(s.cell(0, 1).unwrap().fg, Color::Rgb(1, 2, 3));
        let i = s.cell(0, 2).unwrap();
        assert_eq!(i.bg, Color::Indexed(200));
        assert!(i.flags.underline);
    }

    #[test]
    fn cursor_movement_and_clear() {
        let mut e = emu(20, 5);
        e.feed(b"abc\x1b[2;3Hx");
        assert_eq!(e.snapshot().cell(1, 2).unwrap().ch, 'x');
        e.feed(b"\x1b[2J\x1b[H");
        let s = e.snapshot();
        assert_eq!(s.row_text(0), "");
        assert_eq!(s.row_text(1), "");
        assert_eq!(s.cursor.map(|c| (c.row, c.col)), Some((0, 0)));
    }

    #[test]
    fn device_attribute_reply() {
        let mut e = emu(20, 3);
        e.feed(b"\x1b[c");
        let reply = e.take_replies();
        assert!(reply.starts_with(b"\x1b[?"), "{reply:?}");
        assert!(e.take_replies().is_empty());
    }

    #[test]
    fn cursor_position_report() {
        let mut e = emu(20, 5);
        e.feed(b"\x1b[3;4H\x1b[6n");
        assert_eq!(e.take_replies(), b"\x1b[3;4R");
    }

    #[test]
    fn modes_tracked() {
        let mut e = emu(20, 3);
        assert!(!e.modes().app_cursor);
        e.feed(b"\x1b[?1h\x1b[?2004h");
        let m = e.modes();
        assert!(m.app_cursor && m.bracketed_paste && !m.alt_screen);
        e.feed(b"\x1b[?1049h");
        assert!(e.modes().alt_screen);
    }

    #[test]
    fn title_and_bell() {
        let mut e = emu(20, 3);
        e.feed(b"\x1b]0;my title\x07\x07");
        assert_eq!(e.title().as_deref(), Some("my title"));
        assert!(e.take_bell());
        assert!(!e.take_bell());
    }

    #[test]
    fn resize_changes_dimensions() {
        let mut e = emu(20, 5);
        e.feed(b"abc");
        e.resize(40, 10);
        let s = e.snapshot();
        assert_eq!((s.cols, s.rows), (40, 10));
        assert_eq!(s.cells.len(), 400);
        assert_eq!(s.row_text(0), "abc");
    }

    #[test]
    fn scrollback_and_scroll() {
        let mut e = emu(10, 3);
        for i in 0..10 {
            e.feed(format!("line{i}\r\n").as_bytes());
        }
        let s = e.snapshot();
        assert!(s.history > 0);
        assert_eq!(s.display_offset, 0);
        e.scroll(3);
        let up = e.snapshot();
        assert_eq!(up.display_offset, 3);
        assert_ne!(up.row_text(0), s.row_text(0));
        e.scroll_to_bottom();
        assert_eq!(e.snapshot().row_text(0), s.row_text(0));
        e.scroll_to_top();
        assert_eq!(e.snapshot().row_text(0), "line0");
    }

    #[test]
    fn selection_text() {
        let mut e = emu(20, 3);
        e.feed(b"hello world");
        e.start_selection(SelectKind::Simple, 0, 0);
        e.update_selection(0, 4);
        assert_eq!(e.selection_text().as_deref(), Some("hello"));
        let spans = e.snapshot().selection;
        assert_eq!(
            spans,
            vec![RowSpan {
                row: 0,
                col_start: 0,
                col_end: 4
            }]
        );
        e.clear_selection();
        assert!(e.selection_text().is_none());
    }

    #[test]
    fn word_selection() {
        let mut e = emu(20, 3);
        e.feed(b"foo bar baz");
        e.start_selection(SelectKind::Word, 0, 5);
        e.update_selection(0, 5);
        assert_eq!(e.selection_text().as_deref(), Some("bar"));
    }

    #[test]
    fn search_finds_and_steps() {
        let mut e = emu(20, 3);
        for i in 0..8 {
            e.feed(format!("row {i} Foo\r\n").as_bytes());
        }
        assert_eq!(e.search("foo"), 8);
        assert_eq!(e.search_status(), Some((8, 8)));
        e.search_step(false);
        assert_eq!(e.search_status(), Some((7, 8)));
        e.search_step(true);
        e.search_step(true);
        assert_eq!(e.search_status(), Some((1, 8)));
        // Stepping to the first match scrolled history into view.
        assert!(e.snapshot().current_match.is_some());
        assert_eq!(e.search("nomatch"), 0);
        e.clear_search();
        assert!(e.search_status().is_none());
    }

    #[test]
    fn wide_chars_have_spacer() {
        let mut e = emu(10, 2);
        e.feed("日a".as_bytes());
        let s = e.snapshot();
        assert!(s.cell(0, 0).unwrap().flags.wide);
        assert!(s.cell(0, 1).unwrap().flags.spacer);
        assert_eq!(s.cell(0, 2).unwrap().ch, 'a');
    }
}
