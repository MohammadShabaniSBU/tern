//! Reusable keyboard list (S2-T05): selection, filter, groups, scroll-into-view.
//!
//! [`ListState`] is a pure model (no iced types except in `view`) so navigation, filtering and
//! collapse/expand are unit-tested. Items are a flat, display-ordered tree: a group is followed by
//! its children, which have a greater `depth`.

use iced::widget::{column, container, scrollable, space, text};
use iced::{Border, Center, Color, Element, Fill, Theme};

use crate::app::Message;

/// Fixed row height in logical pixels; makes scroll-into-view and windowing arithmetic exact.
pub const ROW_H: f32 = 26.0;
/// Rows rendered beyond the visible window on each side.
const OVERSCAN: usize = 8;
/// Viewport height assumed until the scrollable reports its real size.
const DEFAULT_VIEWPORT_H: f32 = 600.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub depth: u8,
    /// `Some(expanded)` for groups, `None` for leaves.
    pub group: Option<bool>,
}

impl Item {
    pub fn leaf(label: impl Into<String>, depth: u8) -> Self {
        Self {
            label: label.into(),
            depth,
            group: None,
        }
    }

    pub fn group(label: impl Into<String>, depth: u8, expanded: bool) -> Self {
        Self {
            label: label.into(),
            depth,
            group: Some(expanded),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListState {
    items: Vec<Item>,
    /// Indexes into `items` that are currently shown (after collapse + filter).
    visible: Vec<usize>,
    /// Index into `items` of the selected row. Always a visible item when any exist.
    selected: usize,
    pub filter: String,
    offset_y: f32,
    viewport_h: f32,
}

impl ListState {
    pub fn new(items: Vec<Item>) -> Self {
        let mut s = Self {
            items,
            visible: Vec::new(),
            selected: 0,
            filter: String::new(),
            offset_y: 0.0,
            viewport_h: DEFAULT_VIEWPORT_H,
        };
        s.refresh();
        s
    }

    pub fn len_visible(&self) -> usize {
        self.visible.len()
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.visible
            .contains(&self.selected)
            .then(|| &self.items[self.selected])
    }

    /// Position of the selection among visible rows.
    pub fn selected_row(&self) -> Option<usize> {
        self.visible.iter().position(|&i| i == self.selected)
    }

    pub fn rows_per_page(&self) -> usize {
        ((self.viewport_h / ROW_H) as usize)
            .saturating_sub(1)
            .max(1)
    }

    // ----- navigation -----

    pub fn move_by(&mut self, delta: isize) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let last = self.visible.len() as isize - 1;
        let target = (row as isize + delta).clamp(0, last) as usize;
        self.selected = self.visible[target];
    }

    pub fn first(&mut self) {
        if let Some(&i) = self.visible.first() {
            self.selected = i;
        }
    }

    pub fn last(&mut self) {
        if let Some(&i) = self.visible.last() {
            self.selected = i;
        }
    }

    pub fn page(&mut self, down: bool) {
        let n = self.rows_per_page() as isize;
        self.move_by(if down { n } else { -n });
    }

    /// `h`: collapse an expanded group, else select the parent group. `false` if neither applies
    /// (caller then moves focus to the sidebar).
    pub fn collapse(&mut self) -> bool {
        let Some(item) = self.selected_item() else {
            return false;
        };
        if item.group == Some(true) && self.filter.is_empty() {
            self.items[self.selected].group = Some(false);
            self.refresh();
            return true;
        }
        if let Some(parent) = self.parent_of(self.selected) {
            self.selected = parent;
            return true;
        }
        false
    }

    /// `l`: expand a collapsed group, or step into an expanded one. `false` on a leaf.
    pub fn expand(&mut self) -> bool {
        let Some(item) = self.selected_item() else {
            return false;
        };
        match item.group {
            Some(false) => {
                self.items[self.selected].group = Some(true);
                self.refresh();
                true
            }
            Some(true) => {
                let before = self.selected;
                self.move_by(1);
                // Only a real step into the group counts (an empty group stays put).
                self.selected != before
            }
            None => false,
        }
    }

    /// Remove the selected item (and a group's children). Returns its label.
    pub fn remove_selected(&mut self) -> Option<String> {
        self.selected_item()?;
        let end = self.subtree_end(self.selected);
        let label = self.items[self.selected].label.clone();
        self.items.drain(self.selected..end);
        self.refresh();
        Some(label)
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.refresh();
    }

    fn parent_of(&self, index: usize) -> Option<usize> {
        let depth = self.items[index].depth;
        if depth == 0 {
            return None;
        }
        (0..index)
            .rev()
            .find(|&i| self.items[i].depth < depth && self.items[i].group.is_some())
    }

    /// Exclusive end of the subtree rooted at `index`.
    fn subtree_end(&self, index: usize) -> usize {
        let depth = self.items[index].depth;
        (index + 1..self.items.len())
            .find(|&i| self.items[i].depth <= depth)
            .unwrap_or(self.items.len())
    }

    /// Recompute `visible` and repair the selection.
    fn refresh(&mut self) {
        let needle = self.filter.trim().to_lowercase();
        let mut visible = Vec::with_capacity(self.items.len());
        if needle.is_empty() {
            let mut i = 0;
            while i < self.items.len() {
                visible.push(i);
                i = if self.items[i].group == Some(false) {
                    self.subtree_end(i)
                } else {
                    i + 1
                };
            }
        } else {
            // Filtering ignores collapse state: show matches, their ancestors, and the whole
            // subtree of a matching group.
            let n = self.items.len();
            let mut keep = vec![false; n];
            for i in 0..n {
                if self.items[i].label.to_lowercase().contains(&needle) {
                    keep[i] = true;
                    let mut p = self.parent_of(i);
                    while let Some(a) = p {
                        keep[a] = true;
                        p = self.parent_of(a);
                    }
                    if self.items[i].group.is_some() {
                        let end = self.subtree_end(i);
                        keep[i..end].fill(true);
                    }
                }
            }
            visible.extend((0..n).filter(|&i| keep[i]));
        }
        self.visible = visible;
        if !self.visible.contains(&self.selected) {
            // Nearest visible item at or after the old selection, else the last one.
            self.selected = self
                .visible
                .iter()
                .copied()
                .find(|&i| i >= self.selected)
                .or_else(|| self.visible.last().copied())
                .unwrap_or(0);
        }
    }

    // ----- scrolling -----

    /// Record what the scrollable reports.
    pub fn viewport_changed(&mut self, offset_y: f32, viewport_h: f32) {
        self.offset_y = offset_y;
        if viewport_h > 0.0 {
            self.viewport_h = viewport_h;
        }
    }

    /// New scroll offset needed to bring the selection into view, or `None` if it already is.
    pub fn reveal_offset(&self) -> Option<f32> {
        let row = self.selected_row()?;
        let top = row as f32 * ROW_H;
        let bottom = top + ROW_H;
        if top < self.offset_y {
            Some(top)
        } else if bottom > self.offset_y + self.viewport_h {
            Some(bottom - self.viewport_h)
        } else {
            None
        }
    }

    pub fn offset(&self) -> f32 {
        self.offset_y
    }

    /// Remember an offset we just scrolled to (the scrollable only reports user scrolling).
    pub fn set_offset(&mut self, offset_y: f32) {
        self.offset_y = offset_y;
    }

    /// Window of visible-row positions to actually build widgets for.
    pub fn window(&self) -> std::ops::Range<usize> {
        let first = (self.offset_y / ROW_H) as usize;
        let count = (self.viewport_h / ROW_H).ceil() as usize + 1;
        let start = first.saturating_sub(OVERSCAN);
        let end = (first + count + OVERSCAN).min(self.visible.len());
        start.min(end)..end
    }

    pub fn visible_items(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (usize, &Item)> {
        range.map(|row| (row, &self.items[self.visible[row]]))
    }
}

/// Widget id of the list's scrollable.
pub const LIST_ID: &str = "main-list";

/// Render the list. `focused` draws the focus ring on the selected row.
pub fn view(list: &ListState, focused: bool) -> Element<'_, Message> {
    let window = list.window();
    let rows_above = window.start as f32 * ROW_H;
    let rows_below = (list.len_visible() - window.end) as f32 * ROW_H;
    let selected = list.selected_row();

    let mut col = column![space().height(rows_above)];
    for (row, item) in list.visible_items(window) {
        let marker = match item.group {
            Some(true) => "▾ ",
            Some(false) => "▸ ",
            None => "  ",
        };
        let label = format!(
            "{}{marker}{}",
            "    ".repeat(usize::from(item.depth)),
            item.label
        );
        let is_selected = selected == Some(row);
        col = col.push(
            container(text(label).size(14))
                .width(Fill)
                .height(ROW_H)
                .padding([0, 8])
                .align_y(Center)
                .style(move |theme: &Theme| row_style(theme, is_selected, focused)),
        );
    }
    col = col.push(space().height(rows_below));

    scrollable(col)
        .id(LIST_ID)
        .on_scroll(|v| Message::ListScrolled(v.absolute_offset().y, v.bounds().height))
        .height(Fill)
        .into()
}

fn row_style(theme: &Theme, selected: bool, focused: bool) -> container::Style {
    if !selected {
        return container::Style::default();
    }
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.primary.weak.color.into()),
        text_color: Some(palette.primary.weak.text),
        border: Border {
            // The ring is only drawn while this pane has focus.
            color: if focused {
                palette.primary.strong.color
            } else {
                Color::TRANSPARENT
            },
            width: 2.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}

/// Throwaway data for the exit demo; replaced by real repositories in Sprint 3.
pub fn dummy_items(section: crate::action::Section) -> Vec<Item> {
    use crate::action::Section;
    match section {
        Section::Hosts => {
            let mut v = Vec::new();
            for (group, names) in [
                ("prod", &["prod-db", "prod-web-1", "prod-web-2"][..]),
                ("staging", &["staging-db", "staging-web"][..]),
                ("dev", &["dev-box", "dev-vm"][..]),
            ] {
                v.push(Item::group(group, 0, true));
                v.extend(names.iter().map(|n| Item::leaf(*n, 1)));
            }
            v.push(Item::leaf("raspberry-pi", 0));
            v
        }
        Section::Keys => (1..=1000)
            .map(|n| Item::leaf(format!("key-{n:04}"), 0))
            .collect(),
        Section::Identities => ["deploy", "admin", "backup"]
            .iter()
            .map(|n| Item::leaf(*n, 0))
            .collect(),
        Section::Tunnels => ["db-forward", "socks-proxy"]
            .iter()
            .map(|n| Item::leaf(*n, 0))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Section;

    fn tree() -> ListState {
        ListState::new(dummy_items(Section::Hosts))
    }

    fn labels(l: &ListState) -> Vec<&str> {
        l.visible_items(0..l.len_visible())
            .map(|(_, i)| i.label.as_str())
            .collect()
    }

    fn sel(l: &ListState) -> &str {
        l.selected_item().map(|i| i.label.as_str()).unwrap_or("")
    }

    #[test]
    fn moves_and_clamps() {
        let mut l = tree();
        assert_eq!(sel(&l), "prod");
        l.move_by(-1);
        assert_eq!(sel(&l), "prod");
        l.move_by(1);
        assert_eq!(sel(&l), "prod-db");
        l.last();
        assert_eq!(sel(&l), "raspberry-pi");
        l.move_by(1);
        assert_eq!(sel(&l), "raspberry-pi");
        l.first();
        assert_eq!(sel(&l), "prod");
    }

    #[test]
    fn collapse_hides_children_and_expand_restores() {
        let mut l = tree();
        assert!(l.collapse());
        assert_eq!(labels(&l)[..2], ["prod", "staging"]);
        assert!(l.expand());
        assert_eq!(labels(&l)[..2], ["prod", "prod-db"]);
        // Expanded group: `l` steps into it.
        assert!(l.expand());
        assert_eq!(sel(&l), "prod-db");
    }

    #[test]
    fn h_on_a_child_selects_its_parent_then_leaf_at_top_reports_false() {
        let mut l = tree();
        l.move_by(2);
        assert_eq!(sel(&l), "prod-web-1");
        assert!(l.collapse());
        assert_eq!(sel(&l), "prod");
        assert!(l.collapse()); // collapses the group itself
        l.last();
        assert_eq!(sel(&l), "raspberry-pi");
        assert!(!l.collapse(), "top-level leaf: nothing to collapse");
        assert!(!l.expand());
    }

    #[test]
    fn selection_survives_collapsing_its_ancestor() {
        let mut l = tree();
        l.move_by(2); // prod-web-1
        // Collapse "prod" externally by selecting parent first, then selecting a hidden child
        // is impossible; instead check refresh repair directly.
        l.items[0].group = Some(false);
        l.refresh();
        assert!(l.selected_row().is_some());
        assert_eq!(sel(&l), "staging");
    }

    #[test]
    fn filter_keeps_matches_with_ancestors_and_ignores_collapse() {
        let mut l = tree();
        l.collapse();
        l.set_filter("web".into());
        assert_eq!(
            labels(&l),
            ["prod", "prod-web-1", "prod-web-2", "staging", "staging-web"]
        );
        assert!(l.selected_row().is_some());
        l.set_filter("STAGING".into());
        // Matching group shows its whole subtree.
        assert_eq!(labels(&l), ["staging", "staging-db", "staging-web"]);
        l.set_filter("zzz".into());
        assert_eq!(l.len_visible(), 0);
        l.move_by(1);
        l.first();
        assert!(l.selected_item().is_none());
        l.set_filter(String::new());
        assert!(l.selected_item().is_some());
    }

    #[test]
    fn thousand_items_selection_is_always_revealed() {
        let mut l = ListState::new(dummy_items(Section::Keys));
        assert_eq!(l.len_visible(), 1000);
        l.viewport_changed(0.0, 10.0 * ROW_H);
        assert_eq!(l.reveal_offset(), None);

        // Walk down: after applying each reveal offset the selected row is inside the viewport.
        for _ in 0..999 {
            l.move_by(1);
            if let Some(o) = l.reveal_offset() {
                l.set_offset(o);
            }
            let row = l.selected_row().unwrap_or(0) as f32 * ROW_H;
            assert!(row >= l.offset_y && row + ROW_H <= l.offset_y + l.viewport_h);
        }
        // Jump back to the top.
        l.first();
        assert_eq!(l.reveal_offset(), Some(0.0));
        l.set_offset(0.0);
        l.last();
        let o = l.reveal_offset().expect("must scroll");
        assert_eq!(o, 1000.0 * ROW_H - l.viewport_h);
    }

    #[test]
    fn removing_a_group_removes_its_children_and_keeps_a_selection() {
        let mut l = tree();
        assert_eq!(l.remove_selected().as_deref(), Some("prod"));
        assert_eq!(sel(&l), "staging");
        assert!(!labels(&l).contains(&"prod-db"));
        l.last();
        l.remove_selected();
        assert_eq!(sel(&l), "dev-vm");
        let mut empty = ListState::new(Vec::new());
        assert_eq!(empty.remove_selected(), None);
    }

    #[test]
    fn paging_moves_by_a_screenful() {
        let mut l = ListState::new(dummy_items(Section::Keys));
        l.viewport_changed(0.0, 10.0 * ROW_H);
        l.page(true);
        assert_eq!(l.selected_row(), Some(9));
        l.page(false);
        assert_eq!(l.selected_row(), Some(0));
    }

    #[test]
    fn window_is_small_even_for_big_lists() {
        let mut l = ListState::new(dummy_items(Section::Keys));
        l.viewport_changed(500.0 * ROW_H, 20.0 * ROW_H);
        let w = l.window();
        assert!(w.len() < 50, "{w:?}");
        assert!(w.contains(&510));
    }
}
