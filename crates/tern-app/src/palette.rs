//! Command palette (S2-T04): fuzzy search over actions and hosts.
//!
//! The model ([`Palette`]) is pure; `view` renders it. Running an entry goes through
//! `State::dispatch`, same as the keymap.

use iced::widget::{column, container, row, space, text, text_input};
use iced::{Color, Element, Fill, Theme};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::action::{Action, ModeMask};
use crate::app::Message;
use crate::keymap::Keymap;

/// Settings key holding the JSON array of recently used entries.
pub const RECENT_SETTING: &str = "palette_recent";
pub const PALETTE_ID: &str = "palette-input";
/// Rows shown at once.
const MAX_RESULTS: usize = 10;
/// How many recently used entries are remembered and boosted.
pub const MAX_RECENT: usize = 20;
/// Largest score bonus (for the most recent entry); roughly one strong match bonus.
const RECENT_BONUS: u32 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Action(Action),
    /// "Connect: <host label>"; carries the host id.
    Connect(String),
    /// Connect to a typed `user@host[:port]` without saving it (S4-T07).
    Quick(QuickTarget),
    /// Save a typed `user@host[:port]` as a new host.
    SaveQuick(QuickTarget),
}

/// A parsed `user@host[:port]` connect string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickTarget {
    pub user: String,
    pub host: String,
    pub port: u16,
}

impl QuickTarget {
    /// Parse `user@host`, `user@host:port` or `user@[v6addr]:port`. `None` when it is not one.
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        let (user, rest) = input.split_once('@')?;
        let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
            let (addr, tail) = v6.split_once(']')?;
            let port = match tail.strip_prefix(':') {
                Some(p) => p.parse().ok()?,
                None if tail.is_empty() => 22,
                None => return None,
            };
            (addr, port)
        } else {
            match rest.rsplit_once(':') {
                Some((h, p)) => (h, p.parse().ok()?),
                None => (rest, 22),
            }
        };
        tern_core::validate_username(user).ok()?;
        tern_core::validate_hostname(host).ok()?;
        tern_core::validate_port(port).ok()?;
        Some(Self {
            user: user.to_owned(),
            host: host.to_owned(),
            port,
        })
    }

    pub fn display(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.user, self.host)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub title: String,
    pub target: Target,
    /// Right-hand column: the action's keybinding.
    pub keys: String,
    /// Stable key for recents persistence: action id or `host:<label>`.
    pub recent_key: String,
}

#[derive(Debug, Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
    entries: Vec<Entry>,
    /// Indexes into `entries`, best first, capped at [`MAX_RESULTS`].
    results: Vec<usize>,
    /// Most recent first.
    recent: Vec<String>,
    /// Host picker (`Ctrl+Shift+O`): typed `user@host:port` strings offer quick connect.
    hosts_only: bool,
    hosts: Vec<HostRef>,
}

/// A saved host as offered by the palette's connect entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRef {
    pub id: String,
    pub label: String,
    /// Extra searchable text (address, tags, group), shown dimmed after the label.
    pub detail: String,
}

impl Palette {
    /// Open as a host-only picker (`Ctrl+Shift+O`).
    pub fn open_hosts(&mut self, hosts: &[HostRef]) {
        self.open(&Keymap::defaults(), hosts);
        self.entries
            .retain(|e| matches!(e.target, Target::Connect(_)));
        self.hosts_only = true;
        self.hosts = hosts.to_vec();
        self.refresh();
    }

    pub fn set_recent(&mut self, recent: Vec<String>) {
        self.recent = recent;
        self.recent.truncate(MAX_RECENT);
    }

    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Rebuild the entry list and reset the query. Called each time the palette opens.
    pub fn open(&mut self, keymap: &Keymap, hosts: &[HostRef]) {
        self.hosts_only = false;
        self.query.clear();
        self.selected = 0;
        self.entries = Action::ALL
            .iter()
            .copied()
            .filter(|a| {
                !matches!(a, Action::Back | Action::OpenPalette)
                    && a.meta().available_in.contains(ModeMask::NORMAL)
            })
            .map(|a| {
                let meta = a.meta();
                Entry {
                    title: meta.title.to_owned(),
                    target: Target::Action(a),
                    keys: keymap
                        .keys_for(a)
                        .first()
                        .map(|k| pretty_keys(k))
                        .unwrap_or_default(),
                    recent_key: meta.id.to_owned(),
                }
            })
            .chain(hosts.iter().map(|h| Entry {
                title: if h.detail.is_empty() {
                    format!("Connect: {}", h.label)
                } else {
                    format!("Connect: {}  {}", h.label, h.detail)
                },
                target: Target::Connect(h.id.clone()),
                keys: String::new(),
                recent_key: format!("host:{}", h.label),
            }))
            .collect();
        self.refresh();
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;
        self.entries
            .retain(|e| !matches!(e.target, Target::Quick(_) | Target::SaveQuick(_)));
        if self.hosts_only
            && let Some(q) = QuickTarget::parse(&self.query)
            && !self.hosts.iter().any(|h| h.label == q.host)
        {
            let shown = q.display();
            self.entries.push(Entry {
                title: format!("Connect to {shown}"),
                target: Target::Quick(q.clone()),
                keys: String::new(),
                recent_key: format!("quick:{shown}"),
            });
            self.entries.push(Entry {
                title: format!("Save as new host: {shown}"),
                target: Target::SaveQuick(q),
                keys: String::new(),
                recent_key: format!("save:{shown}"),
            });
        }
        self.refresh();
        // The typed address is the most likely intent: show its entries first.
        let n = self.entries.len();
        if self.hosts_only && n >= 2 && matches!(self.entries[n - 2].target, Target::Quick(_)) {
            self.results.retain(|&i| i < n - 2);
            self.results.insert(0, n - 1);
            self.results.insert(0, n - 2);
            self.results.truncate(MAX_RESULTS);
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let last = self.results.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.results.get(self.selected).map(|&i| &self.entries[i])
    }

    /// Mark an entry as just used (moves it to the front of the recents).
    pub fn record_use(&mut self, recent_key: &str) {
        self.recent.retain(|k| k != recent_key);
        self.recent.insert(0, recent_key.to_owned());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn results(&self) -> impl Iterator<Item = &Entry> {
        self.results.iter().map(|&i| &self.entries[i])
    }

    fn recent_bonus(&self, key: &str) -> u32 {
        match self.recent.iter().position(|k| k == key) {
            Some(pos) => RECENT_BONUS * (MAX_RECENT - pos) as u32 / MAX_RECENT as u32,
            None => 0,
        }
    }

    fn refresh(&mut self) {
        let mut scored: Vec<(usize, u32)> = if self.query.trim().is_empty() {
            // No query: recents first, then registry order.
            (0..self.entries.len())
                .map(|i| (i, self.recent_bonus(&self.entries[i].recent_key)))
                .collect()
        } else {
            let mut matcher = Matcher::new(Config::DEFAULT);
            let pattern = Pattern::parse(&self.query, CaseMatching::Ignore, Normalization::Smart);
            let mut buf = Vec::new();
            (0..self.entries.len())
                .filter_map(|i| {
                    let e = &self.entries[i];
                    let score = pattern.score(Utf32Str::new(&e.title, &mut buf), &mut matcher)?;
                    Some((i, score + self.recent_bonus(&e.recent_key)))
                })
                .collect()
        };
        // Highest score first; ties keep registry order.
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        scored.truncate(MAX_RESULTS);
        self.results = scored.into_iter().map(|(i, _)| i).collect();
    }
}

/// `"ctrl+shift+l"` -> `"Ctrl+Shift+L"`, `"g g"` -> `"G G"`.
pub fn pretty_keys(keys: &str) -> String {
    keys.split(' ')
        .map(|chord| {
            chord
                .split('+')
                .map(|part| {
                    let mut c = part.chars();
                    match c.next() {
                        Some(f) => f.to_uppercase().chain(c).collect::<String>(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn view(p: &Palette) -> Element<'_, Message> {
    let input = text_input("Type a command or host…", &p.query)
        .id(PALETTE_ID)
        .on_input(Message::PaletteQueryChanged)
        .padding(8);

    let mut list = column![].spacing(2);
    for (i, entry) in p.results().enumerate() {
        let selected = i == p.selected;
        list = list.push(
            container(
                row![
                    text(entry.title.as_str()).size(14),
                    space().width(Fill),
                    text(entry.keys.as_str()).size(12),
                ]
                .padding([4, 8]),
            )
            .width(Fill)
            .style(move |theme: &Theme| {
                if selected {
                    let pal = theme.extended_palette();
                    container::Style {
                        background: Some(pal.primary.weak.color.into()),
                        text_color: Some(pal.primary.weak.text),
                        ..container::Style::default()
                    }
                } else {
                    container::Style::default()
                }
            }),
        );
    }
    if p.results().next().is_none() {
        list = list.push(text("No matches").size(14));
    }

    let panel = container(column![input, list].spacing(8))
        .padding(12)
        .width(560)
        .style(|theme: &Theme| {
            let mut s = container::bordered_box(theme);
            s.background = Some(theme.extended_palette().background.base.color.into());
            s
        });
    container(panel)
        .center_x(Fill)
        .padding(iced::Padding::ZERO.top(80))
        .height(Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn open(hosts: &[HostRef]) -> Palette {
        let mut p = Palette::default();
        p.open(&Keymap::defaults(), hosts);
        p
    }

    fn top(p: &Palette) -> Option<&str> {
        p.selected_entry().map(|e| e.title.as_str())
    }

    #[test]
    fn typing_lock_finds_lock_vault_first() {
        let mut p = open(&[]);
        p.set_query("lock".into());
        assert_eq!(top(&p), Some("Lock vault"));
        assert_eq!(
            p.selected_entry().map(|e| e.target.clone()),
            Some(Target::Action(Action::LockVault))
        );
        assert_eq!(
            p.selected_entry().map(|e| e.keys.as_str()),
            Some("Ctrl+Shift+L")
        );
    }

    #[test]
    fn fuzzy_matches_hosts() {
        let host = |l: &str| HostRef {
            id: l.to_owned(),
            label: l.to_owned(),
            detail: String::new(),
        };
        let hosts = vec![host("prod-db"), host("staging-web")];
        let mut p = open(&hosts);
        p.set_query("conn prod".into());
        assert_eq!(top(&p), Some("Connect: prod-db"));
        p.set_query("pdb".into());
        assert_eq!(top(&p), Some("Connect: prod-db"));
    }

    #[test]
    fn parses_quick_connect_strings() {
        let q = |s| QuickTarget::parse(s);
        assert_eq!(
            q("root@10.0.0.5"),
            Some(QuickTarget {
                user: "root".into(),
                host: "10.0.0.5".into(),
                port: 22
            })
        );
        assert_eq!(q("me@example.com:2200").map(|t| t.port), Some(2200));
        assert_eq!(
            q("me@[::1]:2200").map(|t| (t.host, t.port)),
            Some(("::1".into(), 2200))
        );
        assert_eq!(q("example.com"), None);
        assert_eq!(q("me@"), None);
        assert_eq!(q("me@host:notaport"), None);
        assert_eq!(q("me@host:0"), None);
        assert_eq!(q("@host"), None);
    }

    #[test]
    fn host_picker_offers_quick_connect_and_save() {
        let mut p = Palette::default();
        p.open_hosts(&[]);
        p.set_query("alice@db.example.com:2222".into());
        let titles: Vec<_> = p.results().map(|e| e.title.clone()).collect();
        assert_eq!(titles[0], "Connect to alice@db.example.com:2222");
        assert_eq!(titles[1], "Save as new host: alice@db.example.com:2222");
        // Typing something else removes them again.
        p.set_query("alice".into());
        assert!(p.results().next().is_none());
        // The full palette never offers them.
        let mut full = open(&[]);
        full.set_query("alice@db.example.com".into());
        assert!(!full.results().any(|e| matches!(e.target, Target::Quick(_))));
    }

    #[test]
    fn no_match_gives_no_selection() {
        let mut p = open(&[]);
        p.set_query("qqqzzzxxx".into());
        assert!(p.selected_entry().is_none());
        p.move_selection(1); // must not panic
    }

    #[test]
    fn selection_moves_and_clamps() {
        let mut p = open(&[]);
        p.move_selection(-1);
        assert_eq!(p.selected, 0);
        p.move_selection(3);
        assert_eq!(p.selected, 3);
        p.move_selection(1000);
        assert_eq!(p.selected, MAX_RESULTS - 1);
    }

    #[test]
    fn recently_used_ranks_higher() {
        let mut p = open(&[]);
        p.set_query("go to".into());
        let before = top(&p).map(str::to_owned);
        assert_eq!(before.as_deref(), Some("Go to Hosts"));

        p.record_use("nav.tunnels");
        p.set_query(String::new());
        p.set_query("go to".into());
        assert_eq!(top(&p), Some("Go to Tunnels"));

        // With an empty query the most recent entry comes first too.
        p.set_query(String::new());
        assert_eq!(top(&p), Some("Go to Tunnels"));
    }

    #[test]
    fn recents_are_deduplicated_and_bounded() {
        let mut p = Palette::default();
        for i in 0..50 {
            p.record_use(&format!("k{i}"));
        }
        p.record_use("k10");
        assert_eq!(p.recent().len(), MAX_RECENT);
        assert_eq!(p.recent()[0], "k10");
        assert_eq!(p.recent().iter().filter(|k| *k == "k10").count(), 1);
    }

    #[test]
    fn modal_only_and_self_actions_are_not_listed() {
        let p = open(&[]);
        assert!(!p.entries.iter().any(|e| matches!(
            e.target,
            Target::Action(Action::Confirm | Action::Cancel | Action::Back | Action::OpenPalette)
        )));
    }

    #[test]
    fn five_hundred_hosts_open_and_search_quickly() {
        let hosts: Vec<HostRef> = (0..500)
            .map(|i| HostRef {
                id: i.to_string(),
                label: format!("server-{i:03}.example.com"),
                detail: String::new(),
            })
            .collect();
        let start = Instant::now();
        let mut p = open(&hosts);
        p.set_query("srv 42".into());
        let elapsed = start.elapsed();
        assert!(p.selected_entry().is_some());
        // Budget is 50 ms; unoptimised test builds get generous slack.
        assert!(elapsed.as_millis() < 250, "took {elapsed:?}");
    }

    #[test]
    fn pretty_keys_formats() {
        assert_eq!(pretty_keys("ctrl+shift+l"), "Ctrl+Shift+L");
        assert_eq!(pretty_keys("g g"), "G G");
        assert_eq!(pretty_keys("f1"), "F1");
    }
}
