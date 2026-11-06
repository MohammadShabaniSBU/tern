//! Keybinding cheat sheet (S2-T09), generated from the action registry.

use iced::widget::{column, container, scrollable, space, text};
use iced::{Color, Element, Fill, Theme};

use crate::action::{Action, Category, ModeMask};
use crate::app::Message;
use crate::keymap::Keymap;
use crate::palette::pretty_keys;

/// One line of the sheet.
#[derive(Debug, PartialEq, Eq)]
pub struct Row {
    pub title: &'static str,
    /// Effective bindings (defaults plus user overrides), or empty if unbound.
    pub keys: String,
}

/// Actions available in `mode`, grouped by category in registry order.
pub fn sections(keymap: &Keymap, mode: ModeMask) -> Vec<(Category, Vec<Row>)> {
    let mut out: Vec<(Category, Vec<Row>)> = Vec::new();
    for action in Action::ALL.iter().copied() {
        let meta = action.meta();
        if !meta.available_in.contains(mode) {
            continue;
        }
        let keys = keymap
            .keys_for(action)
            .iter()
            .map(|k| pretty_keys(k))
            .collect::<Vec<_>>()
            .join(" / ");
        let row = Row {
            title: meta.title,
            keys,
        };
        match out.iter_mut().find(|(c, _)| *c == meta.category) {
            Some((_, rows)) => rows.push(row),
            None => out.push((meta.category, vec![row])),
        }
    }
    out.sort_by_key(|(c, _)| *c);
    out
}

pub fn view<'a>(keymap: &Keymap, mode: ModeMask) -> Element<'a, Message> {
    let mut body = column![
        text("Keybindings").size(20),
        text("Esc / F1 to close").size(12)
    ]
    .spacing(4);
    for (category, rows) in sections(keymap, mode) {
        body = body.push(space().height(10));
        body = body.push(text(category.label()).size(15).style(text::primary));
        for row in rows {
            let keys = if row.keys.is_empty() {
                "unbound".to_owned()
            } else {
                row.keys
            };
            body = body.push(
                iced::widget::row![text(row.title).size(13).width(Fill), text(keys).size(13),]
                    .spacing(24),
            );
        }
    }
    let panel = container(scrollable(body.padding(4)))
        .padding(16)
        .width(560)
        .max_height(560)
        .style(|theme: &Theme| {
            let mut s = container::bordered_box(theme);
            s.background = Some(theme.extended_palette().background.base.color.into());
            s
        });
    container(panel)
        .center(Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_titles(s: &[(Category, Vec<Row>)]) -> Vec<&'static str> {
        s.iter()
            .flat_map(|(_, r)| r.iter().map(|r| r.title))
            .collect()
    }

    #[test]
    fn every_normal_mode_action_appears_exactly_once() {
        let km = Keymap::defaults();
        let sheet = sections(&km, ModeMask::NORMAL);
        let titles = all_titles(&sheet);
        for a in Action::ALL {
            let meta = a.meta();
            let expected = usize::from(meta.available_in.contains(ModeMask::NORMAL));
            assert_eq!(
                titles.iter().filter(|t| **t == meta.title).count(),
                expected,
                "{}",
                meta.id
            );
        }
    }

    #[test]
    fn grouped_by_category_in_order() {
        let km = Keymap::defaults();
        let cats: Vec<_> = sections(&km, ModeMask::NORMAL)
            .iter()
            .map(|(c, _)| *c)
            .collect();
        let mut sorted = cats.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(cats, sorted);
    }

    #[test]
    fn filtered_by_mode() {
        let km = Keymap::defaults();
        let insert = all_titles(&sections(&km, ModeMask::INSERT));
        // List navigation is Normal-only; global chords remain.
        assert!(!insert.contains(&"Move selection down"));
        assert!(insert.contains(&"Lock vault"));
    }

    #[test]
    fn shows_effective_bindings_including_overrides() {
        let km = Keymap::build(
            None,
            Some(r#"{"vault.lock": ["ctrl+alt+l"], "list.down": []}"#),
        );
        let sheet = sections(&km, ModeMask::NORMAL);
        let find = |t: &str| {
            sheet
                .iter()
                .flat_map(|(_, r)| r.iter())
                .find(|r| r.title == t)
                .map(|r| r.keys.clone())
        };
        assert_eq!(find("Lock vault").as_deref(), Some("Ctrl+Alt+L"));
        assert_eq!(find("Move selection down").as_deref(), Some(""));
        assert_eq!(
            find("Open command palette").as_deref(),
            Some("Ctrl+K / Ctrl+Shift+P")
        );
    }
}
