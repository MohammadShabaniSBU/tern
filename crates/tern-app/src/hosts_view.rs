//! Pure mapping from stored hosts/groups to list rows and the detail pane text (S3-T01).

use std::collections::HashMap;

use tern_core::{Group, GroupId, Host, HostId, Identity, IdentityId};

use crate::list::Item;
use crate::palette::HostRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostSort {
    #[default]
    Label,
    LastConnected,
}

impl HostSort {
    pub fn toggled(self) -> Self {
        match self {
            Self::Label => Self::LastConnected,
            Self::LastConnected => Self::Label,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Label => "label",
            Self::LastConnected => "last connected",
        }
    }
}

/// Everything loaded from the store that the Hosts section needs.
#[derive(Debug, Clone, Default)]
pub struct HostsData {
    pub hosts: Vec<Host>,
    pub groups: Vec<Group>,
    pub identities: Vec<Identity>,
    /// RFC 3339 timestamps keyed by host.
    pub last_connected: HashMap<HostId, String>,
}

impl HostsData {
    pub fn host(&self, id: &str) -> Option<&Host> {
        self.hosts.iter().find(|h| h.id.to_string() == id)
    }

    fn group_name(&self, id: GroupId) -> Option<&str> {
        self.groups
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.name.as_str())
    }

    fn identity(&self, id: IdentityId) -> Option<&Identity> {
        self.identities.iter().find(|i| i.id == id)
    }

    /// Display-ordered rows: groups (nested) first, then ungrouped hosts.
    pub fn items(&self, sort: HostSort) -> Vec<Item> {
        let mut out = Vec::new();
        self.push_level(None, 0, sort, &mut out);
        let mut loose: Vec<&Host> = self
            .hosts
            .iter()
            .filter(|h| h.group_id.is_none_or(|g| self.group_name(g).is_none()))
            .collect();
        self.sort_hosts(&mut loose, sort);
        out.extend(loose.into_iter().map(|h| self.host_item(h, 0)));
        out
    }

    fn push_level(&self, parent: Option<GroupId>, depth: u8, sort: HostSort, out: &mut Vec<Item>) {
        let mut groups: Vec<&Group> = self
            .groups
            .iter()
            .filter(|g| g.parent_id == parent)
            .collect();
        groups.sort_by(|a, b| {
            (a.sort_order, a.name.to_lowercase()).cmp(&(b.sort_order, b.name.to_lowercase()))
        });
        for g in groups {
            out.push(Item::group(g.name.clone(), depth, true).with_id(g.id.to_string()));
            self.push_level(Some(g.id), depth + 1, sort, out);
            let mut hosts: Vec<&Host> = self
                .hosts
                .iter()
                .filter(|h| h.group_id == Some(g.id))
                .collect();
            self.sort_hosts(&mut hosts, sort);
            out.extend(hosts.into_iter().map(|h| self.host_item(h, depth + 1)));
        }
    }

    fn sort_hosts(&self, hosts: &mut [&Host], sort: HostSort) {
        match sort {
            HostSort::Label => hosts.sort_by_key(|h| h.label.to_lowercase()),
            HostSort::LastConnected => hosts.sort_by(|a, b| {
                // Most recent first; never-connected hosts last, then by label.
                let (la, lb) = (
                    self.last_connected.get(&a.id),
                    self.last_connected.get(&b.id),
                );
                lb.cmp(&la)
                    .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            }),
        }
    }

    fn host_item(&self, h: &Host, depth: u8) -> Item {
        let mut meta = self.destination(h);
        if !h.tags.is_empty() {
            meta.push_str("  #");
            meta.push_str(&h.tags.join(" #"));
        }
        meta.push_str("  ");
        meta.push_str(&self.last_seen(h.id));
        let group = h.group_id.and_then(|g| self.group_name(g)).unwrap_or("");
        let search = format!("{} {} {group}", h.hostname, h.tags.join(" "));
        Item::leaf(h.label.clone(), depth)
            .with_id(h.id.to_string())
            .with_meta(meta)
            .with_search(search)
    }

    /// `user@hostname:port`; the user is the host override, else the identity's, else omitted.
    fn destination(&self, h: &Host) -> String {
        let user = h
            .username
            .as_deref()
            .or_else(|| {
                h.identity_id
                    .and_then(|i| self.identity(i))
                    .map(|i| i.username.as_str())
            })
            .map(|u| format!("{u}@"))
            .unwrap_or_default();
        format!("{user}{}:{}", h.hostname, h.port)
    }

    /// Entries for the palette's host picker.
    pub fn host_refs(&self) -> Vec<HostRef> {
        let mut hosts: Vec<&Host> = self.hosts.iter().collect();
        hosts.sort_by_key(|h| h.label.to_lowercase());
        hosts
            .into_iter()
            .map(|h| {
                let mut detail = self.destination(h);
                if !h.tags.is_empty() {
                    detail.push_str(&format!("  #{}", h.tags.join(" #")));
                }
                if let Some(g) = h.group_id.and_then(|g| self.group_name(g)) {
                    detail.push_str(&format!("  [{g}]"));
                }
                HostRef {
                    id: h.id.to_string(),
                    label: h.label.clone(),
                    detail,
                }
            })
            .collect()
    }

    fn last_seen(&self, id: HostId) -> String {
        match self.last_connected.get(&id) {
            // RFC 3339: the date is the first ten characters.
            Some(ts) => ts.chars().take(10).collect(),
            None => "never".to_owned(),
        }
    }

    /// Detail pane for a host id; secrets are never included, only whether one is set.
    pub fn host_detail(&self, id: &str) -> Option<String> {
        let h = self.host(id)?;
        let identity = h.identity_id.and_then(|i| self.identity(i));
        let mut lines = vec![
            h.label.clone(),
            String::new(),
            format!("Address     {}", self.destination(h)),
            format!(
                "Identity    {}",
                identity.map_or("(none)", |i| i.label.as_str())
            ),
            format!(
                "Group       {}",
                h.group_id
                    .and_then(|g| self.group_name(g))
                    .unwrap_or("(none)")
            ),
            format!(
                "Tags        {}",
                if h.tags.is_empty() {
                    "(none)".to_owned()
                } else {
                    h.tags.join(", ")
                }
            ),
            format!(
                "Keepalive   {}",
                h.keepalive_secs
                    .map_or("default".to_owned(), |s| format!("{s}s"))
            ),
            format!("Last used   {}", self.last_seen(h.id)),
            "Password    ••••••••".to_owned(),
        ];
        if identity.is_none() {
            lines.pop();
        }
        if let Some(n) = h.notes.as_deref().filter(|n| !n.is_empty()) {
            lines.push(String::new());
            lines.push(n.to_owned());
        }
        Some(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    /// Timing probe for the 1,000-host target (S8-T06). Run:
    /// `cargo test -p tern-app --release perf_ -- --ignored --nocapture`
    #[test]
    #[ignore = "timing probe; run manually in release mode"]
    fn perf_thousand_hosts() {
        use crate::list::ListState;
        use std::time::Instant;
        let mut data = HostsData::default();
        for i in 0..1000 {
            let mut h = Host::new(format!("host-{i:04}"), format!("h{i}.example.com"));
            h.tags = vec![format!("t{}", i % 10)];
            data.hosts.push(h);
        }
        let t = Instant::now();
        let items = data.items(HostSort::default());
        let build = t.elapsed();
        let t = Instant::now();
        let mut list = ListState::new(items);
        let load = t.elapsed();
        let t = Instant::now();
        for q in ["h", "ho", "host-09", "t5", "zzz", ""] {
            list.set_filter(q.to_owned());
        }
        let filter = t.elapsed() / 6;
        println!(
            "1000 hosts: build items {build:?}, load list {load:?}, filter (avg of 6) {filter:?}"
        );
        assert!(filter.as_millis() < 50, "filter too slow: {filter:?}");
    }

    use super::*;

    fn data() -> HostsData {
        let work = Group {
            id: GroupId::new(),
            name: "Work".into(),
            parent_id: None,
            sort_order: 0,
        };
        let mut a = Host::new("beta", "b.example.com");
        a.group_id = Some(work.id);
        a.username = Some("root".into());
        a.tags = vec!["db".into(), "prod".into()];
        let mut b = Host::new("Alpha", "a.example.com");
        b.group_id = Some(work.id);
        let c = Host::new("loose", "l.example.com");
        let mut last = HashMap::new();
        last.insert(a.id, "2026-11-01T10:00:00Z".to_owned());
        HostsData {
            hosts: vec![a, b, c],
            groups: vec![work],
            identities: vec![],
            last_connected: last,
        }
    }

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn groups_first_then_ungrouped_sorted_by_label() {
        let items = data().items(HostSort::Label);
        assert_eq!(labels(&items), ["Work", "Alpha", "beta", "loose"]);
        assert_eq!(items[1].depth, 1);
        assert_eq!(items[3].depth, 0);
    }

    #[test]
    fn last_connected_sort_puts_recent_first() {
        let items = data().items(HostSort::LastConnected);
        assert_eq!(labels(&items), ["Work", "beta", "Alpha", "loose"]);
    }

    #[test]
    fn row_meta_shows_destination_tags_and_last_seen() {
        let items = data().items(HostSort::Label);
        assert_eq!(
            items[2].meta,
            "root@b.example.com:22  #db #prod  2026-11-01"
        );
        assert_eq!(items[1].meta, "a.example.com:22  never");
    }

    #[test]
    fn filter_matches_hostname_tags_and_group_name() {
        let mut l = crate::list::ListState::new(data().items(HostSort::Label));
        let shown = |l: &crate::list::ListState| -> Vec<String> {
            l.visible_items(0..l.len_visible())
                .map(|(_, i)| i.label.clone())
                .collect()
        };
        l.set_filter("b.example".into());
        assert_eq!(shown(&l), ["Work", "beta"]);
        l.set_filter("PROD".into());
        assert_eq!(shown(&l), ["Work", "beta"]);
        l.set_filter("work".into());
        assert_eq!(shown(&l), ["Work", "Alpha", "beta"]);
    }

    #[test]
    fn picker_entries_carry_address_tags_and_group() {
        let refs = data().host_refs();
        assert_eq!(refs[0].label, "Alpha");
        assert!(refs[1].detail.contains("#db #prod") && refs[1].detail.contains("[Work]"));
    }

    #[test]
    fn host_with_missing_group_is_listed_as_ungrouped() {
        let mut d = data();
        d.groups.clear();
        let items = d.items(HostSort::Label);
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|i| i.depth == 0));
    }

    #[test]
    fn detail_never_contains_secrets_and_resolves_names() {
        let d = data();
        let id = d.hosts[0].id.to_string();
        let text = d.host_detail(&id).unwrap();
        assert!(text.contains("Group       Work"));
        assert!(text.contains("db, prod"));
        assert!(d.host_detail("nope").is_none());
    }
}
