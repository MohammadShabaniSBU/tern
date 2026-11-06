//! OpenSSH client config: parser (import), mapper to tern entities, and a writer (export).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tern_core::{Host, HostId, Tunnel, TunnelId, TunnelKind};

const MAX_INCLUDE_DEPTH: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Forward {
    pub bind_addr: Option<String>,
    pub bind_port: u16,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
}

/// One concrete (non-wildcard) `Host` alias with the options that apply to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_files: Vec<String>,
    pub proxy_jump: Option<String>,
    pub local_forwards: Vec<Forward>,
    pub remote_forwards: Vec<Forward>,
    pub dynamic_forwards: Vec<Forward>,
}

/// Split a config line into `(keyword, rest)`; accepts `Key value` and `Key=value`.
fn split_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let idx = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let key = line[..idx].to_ascii_lowercase();
    let rest = line[idx..].trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    Some((key, rest.trim().to_string()))
}

/// Shell-ish word splitting with double quotes.
fn words(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '#' if !quoted && !any && cur.is_empty() => break,
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_wildcard(p: &str) -> bool {
    p.contains(['*', '?', '!'])
}

/// `[bind:]port`
fn parse_listen(s: &str) -> Option<(Option<String>, u16)> {
    match s.rsplit_once(':') {
        Some((b, p)) => Some((
            Some(b.trim_matches(['[', ']']).to_string()),
            p.parse().ok()?,
        )),
        None => Some((None, s.parse().ok()?)),
    }
}

/// `host:port` (host may be `[v6]`).
fn parse_target(s: &str) -> Option<(String, u16)> {
    let (h, p) = s.rsplit_once(':')?;
    Some((h.trim_matches(['[', ']']).to_string(), p.parse().ok()?))
}

fn parse_forward(args: &str, dynamic: bool) -> Option<Forward> {
    let w = words(args);
    let (bind_addr, bind_port) = parse_listen(w.first()?)?;
    if dynamic {
        return Some(Forward {
            bind_addr,
            bind_port,
            ..Forward::default()
        });
    }
    let (th, tp) = parse_target(w.get(1)?)?;
    Some(Forward {
        bind_addr,
        bind_port,
        target_host: Some(th),
        target_port: Some(tp),
    })
}

/// Expand a leading `~` and resolve relative paths against `base`.
fn resolve(path: &str, home: &Path, base: &Path) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else if path == "~" {
        home.to_path_buf()
    } else {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        }
    }
}

fn glob_match(pat: &str, s: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), s.chars().collect());
    fn go(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => (0..=t.len()).any(|i| go(&p[1..], &t[i..])),
            Some('?') => !t.is_empty() && go(&p[1..], &t[1..]),
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    go(&p, &t)
}

fn expand_include(spec: &str, home: &Path, base: &Path) -> Vec<PathBuf> {
    let full = resolve(spec, home, base);
    let (Some(dir), Some(name)) = (full.parent(), full.file_name().and_then(|n| n.to_str())) else {
        return Vec::new();
    };
    if !name.contains(['*', '?']) {
        return vec![full];
    }
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| glob_match(name, &e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Parse a config file (following `Include`s). `home` is used for `~`; relative includes resolve
/// against `<home>/.ssh` like OpenSSH does for user configs.
pub fn parse_file(path: &Path, home: &Path) -> Vec<ParsedHost> {
    let mut hosts = Vec::new();
    let mut seen = Vec::new();
    parse_into(path, home, 0, &mut seen, &mut hosts);
    hosts
}

fn parse_into(
    path: &Path,
    home: &Path,
    depth: usize,
    seen: &mut Vec<PathBuf>,
    out: &mut Vec<ParsedHost>,
) {
    if depth > MAX_INCLUDE_DEPTH {
        return;
    }
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.contains(&canon) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    seen.push(canon);
    parse_text(&text, home, depth, seen, out);
    seen.pop();
}

/// Parse config text. `Include` is only honoured via the file entry points; here it recurses
/// through the filesystem too (so tests can use temp dirs).
pub fn parse_text(
    text: &str,
    home: &Path,
    depth: usize,
    seen: &mut Vec<PathBuf>,
    out: &mut Vec<ParsedHost>,
) {
    let ssh_dir = home.join(".ssh");
    // Indices into `out` for the current Host block; `None` for Match/wildcard-only blocks.
    let mut current: Vec<usize> = Vec::new();
    for line in text.lines() {
        let Some((key, rest)) = split_line(line) else {
            continue;
        };
        match key.as_str() {
            "include" => {
                // An Include before any Host applies globally; inside a block it is still read
                // here, which is a superset of what OpenSSH does for our purposes.
                for spec in words(&rest) {
                    for f in expand_include(&spec, home, &ssh_dir) {
                        parse_into(&f, home, depth + 1, seen, out);
                    }
                }
                // Hosts from includes are separate blocks; do not attach later options to them.
                current.clear();
            }
            "host" => {
                current.clear();
                for pat in words(&rest) {
                    if is_wildcard(&pat) {
                        continue;
                    }
                    out.push(ParsedHost {
                        alias: pat,
                        ..ParsedHost::default()
                    });
                    current.push(out.len() - 1);
                }
            }
            "match" => current.clear(),
            _ => {
                for &i in &current {
                    let h = &mut out[i];
                    let first = |v: &str| words(v).into_iter().next();
                    match key.as_str() {
                        "hostname" => h.hostname = h.hostname.take().or_else(|| first(&rest)),
                        "user" => h.user = h.user.take().or_else(|| first(&rest)),
                        "port" => h.port = h.port.or_else(|| first(&rest)?.parse().ok()),
                        "identityfile" => {
                            if let Some(f) = first(&rest) {
                                h.identity_files.push(f)
                            }
                        }
                        "proxyjump" => {
                            h.proxy_jump = h.proxy_jump.take().or_else(|| first(&rest));
                        }
                        "localforward" => h.local_forwards.extend(parse_forward(&rest, false)),
                        "remoteforward" => h.remote_forwards.extend(parse_forward(&rest, false)),
                        "dynamicforward" => h.dynamic_forwards.extend(parse_forward(&rest, true)),
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Result of mapping parsed entries onto tern entities.
#[derive(Debug, Default)]
pub struct Built {
    pub hosts: Vec<Host>,
    pub tunnels: Vec<Tunnel>,
    /// `(host id, resolved key file path)` for the caller to import and attach as an identity.
    pub key_files: Vec<(HostId, PathBuf)>,
    pub warnings: Vec<String>,
}

/// Map selected entries to hosts/tunnels. `existing` maps labels of already-stored hosts to ids so
/// `ProxyJump` can point at them. Only the first hop of a `ProxyJump` chain is used.
pub fn build(parsed: &[ParsedHost], existing: &HashMap<String, HostId>, home: &Path) -> Built {
    let mut b = Built::default();
    let mut ids: HashMap<&str, HostId> = HashMap::new();
    for p in parsed {
        let mut h = Host::new(
            &p.alias,
            p.hostname.clone().unwrap_or_else(|| p.alias.clone()),
        );
        h.port = p.port.unwrap_or(22);
        h.username = p.user.clone();
        ids.insert(&p.alias, h.id);
        b.hosts.push(h);
    }
    for (p, h) in parsed.iter().zip(b.hosts.iter_mut()) {
        if let Some(pj) = &p.proxy_jump {
            let first = pj.split(',').next().unwrap_or("");
            // user@host:port -> host
            let name = first.rsplit_once('@').map_or(first, |x| x.1);
            let name = name.split(':').next().unwrap_or(name);
            if pj.contains(',') {
                b.warnings.push(format!(
                    "{}: only the first ProxyJump hop was imported",
                    p.alias
                ));
            }
            match ids.get(name).or_else(|| existing.get(name)) {
                Some(id) => h.jump_host_id = Some(*id),
                None => b
                    .warnings
                    .push(format!("{}: jump host '{name}' not found", p.alias)),
            }
        }
        if let Some(f) = p.identity_files.first() {
            b.key_files
                .push((h.id, resolve(f, home, &home.join(".ssh"))));
        }
        let mut add = |kind: TunnelKind, f: &Forward| {
            b.tunnels.push(Tunnel {
                id: TunnelId::new(),
                label: format!("{} :{}", p.alias, f.bind_port),
                host_id: h.id,
                kind,
                bind_addr: f.bind_addr.clone().unwrap_or_else(|| "127.0.0.1".into()),
                bind_port: f.bind_port,
                target_host: f.target_host.clone(),
                target_port: f.target_port,
                auto_start: false,
            })
        };
        p.local_forwards
            .iter()
            .for_each(|f| add(TunnelKind::Local, f));
        p.remote_forwards
            .iter()
            .for_each(|f| add(TunnelKind::Remote, f));
        p.dynamic_forwards
            .iter()
            .for_each(|f| add(TunnelKind::Dynamic, f));
    }
    b
}

/// Generate an OpenSSH config snippet for `selected` (no secrets). `all` resolves jump hosts.
pub fn export(selected: &[Host], all: &[Host], tunnels: &[Tunnel]) -> String {
    fn alias(h: &Host) -> String {
        let a: String = h
            .label
            .chars()
            .map(|c| {
                if c.is_whitespace() || c == '*' || c == '?' || c == '!' {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        if a.is_empty() { h.hostname.clone() } else { a }
    }
    let mut s = String::from("# Generated by tern. No secrets are included.\n");
    for h in selected {
        s.push_str(&format!(
            "\nHost {}\n    HostName {}\n",
            alias(h),
            h.hostname
        ));
        if let Some(u) = &h.username {
            s.push_str(&format!("    User {u}\n"));
        }
        if h.port != 22 {
            s.push_str(&format!("    Port {}\n", h.port));
        }
        if let Some(j) = h
            .jump_host_id
            .and_then(|id| all.iter().find(|x| x.id == id))
        {
            s.push_str(&format!("    ProxyJump {}\n", alias(j)));
        }
        for t in tunnels.iter().filter(|t| t.host_id == h.id) {
            let listen = format!("{}:{}", t.bind_addr, t.bind_port);
            let target = format!(
                "{}:{}",
                t.target_host.clone().unwrap_or_default(),
                t.target_port.unwrap_or(0)
            );
            match t.kind {
                TunnelKind::Local => s.push_str(&format!("    LocalForward {listen} {target}\n")),
                TunnelKind::Remote => s.push_str(&format!("    RemoteForward {listen} {target}\n")),
                TunnelKind::Dynamic => s.push_str(&format!("    DynamicForward {listen}\n")),
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        std::fs::create_dir_all(ssh.join("conf.d")).unwrap();
        std::fs::write(
            ssh.join("config"),
            r#"
# global comment
Include conf.d/*.conf

Host web staging *.wild
    HostName 10.0.0.5   # trailing comment
    User deploy
    Port=2222
    IdentityFile ~/.ssh/id_web
    ProxyJump bastion
    LocalForward 8080 localhost:80
    DynamicForward 1080

Host *
    User ignored

Match host foo
    User nope

Host "quoted"
    HostName q.example.com
    RemoteForward 127.0.0.1:9000 localhost:22
"#,
        )
        .unwrap();
        std::fs::write(
            ssh.join("conf.d/a.conf"),
            "Host bastion\n  HostName bastion.example.com\n  User jump\n",
        )
        .unwrap();
        // Include cycle must not hang.
        std::fs::write(
            ssh.join("conf.d/b.conf"),
            "Include config\nHost loop\n  HostName l\n",
        )
        .unwrap();
        let cfg = ssh.join("config");
        (home, cfg)
    }

    #[test]
    fn parses_includes_comments_and_wildcards() {
        let (home, cfg) = fixture();
        let hosts = parse_file(&cfg, home.path());
        let names: Vec<_> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, ["bastion", "loop", "web", "staging", "quoted"]);
        let web = &hosts[2];
        assert_eq!(web.hostname.as_deref(), Some("10.0.0.5"));
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.port, Some(2222));
        assert_eq!(web.proxy_jump.as_deref(), Some("bastion"));
        assert_eq!(web.local_forwards.len(), 1);
        assert_eq!(web.dynamic_forwards[0].bind_port, 1080);
        assert_eq!(hosts[1].hostname.as_deref(), Some("l"));
        assert_eq!(hosts[4].remote_forwards[0].target_port, Some(22));
        // The Match block's User must not leak onto anything.
        assert!(hosts.iter().all(|h| h.user.as_deref() != Some("nope")));
    }

    #[test]
    fn builds_hosts_tunnels_and_jump() {
        let (home, cfg) = fixture();
        let parsed = parse_file(&cfg, home.path());
        let built = build(&parsed, &HashMap::new(), home.path());
        let web = built.hosts.iter().find(|h| h.label == "web").unwrap();
        let bastion = built.hosts.iter().find(|h| h.label == "bastion").unwrap();
        assert_eq!(web.jump_host_id, Some(bastion.id));
        assert_eq!(web.port, 2222);
        assert_eq!(
            built.tunnels.iter().filter(|t| t.host_id == web.id).count(),
            2
        );
        assert_eq!(built.key_files.len(), 2); // web + staging share the IdentityFile
        assert!(built.key_files[0].1.ends_with(".ssh/id_web"));
    }

    #[test]
    fn export_round_trips() {
        let (home, cfg) = fixture();
        let parsed = parse_file(&cfg, home.path());
        let built = build(&parsed, &HashMap::new(), home.path());
        let text = export(&built.hosts, &built.hosts, &built.tunnels);
        let again = {
            let mut out = Vec::new();
            parse_text(&text, home.path(), 0, &mut Vec::new(), &mut out);
            out
        };
        assert_eq!(again.len(), built.hosts.len());
        let web = again.iter().find(|h| h.alias == "web").unwrap();
        assert_eq!(web.proxy_jump.as_deref(), Some("bastion"));
        assert_eq!(web.port, Some(2222));
        assert!(!text.contains("IdentityFile"));
    }
}
