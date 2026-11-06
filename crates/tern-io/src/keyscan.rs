//! Find private-key files in a directory such as `~/.ssh`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCandidate {
    pub path: PathBuf,
    pub name: String,
}

const SKIP: &[&str] = &[
    "known_hosts",
    "known_hosts.old",
    "config",
    "authorized_keys",
];
const MAX_SIZE: u64 = 64 * 1024;

/// List regular files that look like private keys (no `.pub`, PEM/OpenSSH private-key header).
pub fn scan(dir: &Path) -> Vec<KeyCandidate> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<KeyCandidate> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".pub") || SKIP.contains(&name.as_str()) || name.starts_with('.') {
                return None;
            }
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() || meta.len() > MAX_SIZE {
                return None;
            }
            let text = std::fs::read(&path).ok()?;
            let head = String::from_utf8_lossy(&text[..text.len().min(200)]).to_string();
            head.contains("PRIVATE KEY-----")
                .then_some(KeyCandidate { path, name })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_private_keys() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        std::fs::write(
            p.join("id_ed25519"),
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n",
        )
        .unwrap();
        std::fs::write(p.join("id_ed25519.pub"), "ssh-ed25519 AAAA").unwrap();
        std::fs::write(p.join("known_hosts"), "x").unwrap();
        std::fs::write(p.join("notes.txt"), "hello").unwrap();
        let found = scan(p);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "id_ed25519");
    }
}
