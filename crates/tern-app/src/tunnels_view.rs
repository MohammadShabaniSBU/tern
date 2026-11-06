//! Tunnels section data -> list rows and detail text (S6-T01).

use tern_core::{Host, Tunnel, TunnelKind};

use crate::list::Item;

#[derive(Debug, Clone, Default)]
pub struct TunnelsData {
    pub tunnels: Vec<Tunnel>,
    pub hosts: Vec<Host>,
}

pub fn kind_label(kind: TunnelKind) -> &'static str {
    match kind {
        TunnelKind::Local => "local",
        TunnelKind::Remote => "remote",
        TunnelKind::Dynamic => "dynamic",
    }
}

/// `bind -> target` as shown in the list (`-> SOCKS5` for dynamic tunnels).
pub fn route(t: &Tunnel) -> String {
    let bind = format!("{}:{}", t.bind_addr, t.bind_port);
    match (&t.target_host, t.target_port) {
        (Some(h), Some(p)) => format!("{bind} -> {h}:{p}"),
        _ => format!("{bind} -> SOCKS5"),
    }
}

impl TunnelsData {
    pub fn tunnel(&self, id: &str) -> Option<&Tunnel> {
        self.tunnels.iter().find(|t| t.id.to_string() == id)
    }

    pub fn host_label(&self, t: &Tunnel) -> &str {
        self.hosts
            .iter()
            .find(|h| h.id == t.host_id)
            .map_or("(missing host)", |h| h.label.as_str())
    }

    pub fn items(&self) -> Vec<Item> {
        self.tunnels
            .iter()
            .map(|t| {
                Item::leaf(t.label.clone(), 0)
                    .with_id(t.id.to_string())
                    .with_meta(format!(
                        "{}  {}  via {}  stopped",
                        kind_label(t.kind),
                        route(t),
                        self.host_label(t)
                    ))
            })
            .collect()
    }

    pub fn detail(&self, id: &str) -> Option<String> {
        let t = self.tunnel(id)?;
        let mut s = format!(
            "{}\n\nKind        {}\nRoute       {}\nHost        {}\nAuto-start  {}",
            t.label,
            kind_label(t.kind),
            route(t),
            self.host_label(t),
            if t.auto_start { "yes" } else { "no" }
        );
        if let Some(w) = t.bind_warning() {
            s.push_str(&format!("\n\nWarning: {w}"));
        }
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tern_core::TunnelId;

    fn data() -> TunnelsData {
        let host = Host::new("bastion", "b.example.com");
        let t = Tunnel {
            id: TunnelId::new(),
            label: "db".into(),
            host_id: host.id,
            kind: TunnelKind::Local,
            bind_addr: "127.0.0.1".into(),
            bind_port: 5433,
            target_host: Some("db.internal".into()),
            target_port: Some(5432),
            auto_start: true,
        };
        TunnelsData {
            tunnels: vec![t],
            hosts: vec![host],
        }
    }

    #[test]
    fn rows_show_route_and_host() {
        let items = data().items();
        assert_eq!(
            items[0].meta,
            "local  127.0.0.1:5433 -> db.internal:5432  via bastion  stopped"
        );
    }

    #[test]
    fn dynamic_route_and_detail_warning() {
        let mut d = data();
        d.tunnels[0].kind = TunnelKind::Dynamic;
        d.tunnels[0].target_host = None;
        d.tunnels[0].target_port = None;
        d.tunnels[0].bind_addr = "0.0.0.0".into();
        let id = d.tunnels[0].id.to_string();
        let t = d.detail(&id).unwrap();
        assert!(t.contains("0.0.0.0:5433 -> SOCKS5") && t.contains("Warning"));
        assert!(d.detail("x").is_none());
    }
}
