//! Keys section data -> list rows and detail text (S3-T04).

use std::collections::HashMap;

use tern_core::{KeyId, SshKeyMeta};

use crate::list::Item;

#[derive(Debug, Clone, Default)]
pub struct KeysData {
    pub keys: Vec<SshKeyMeta>,
    /// RFC 3339 creation timestamps.
    pub created: HashMap<KeyId, String>,
}

fn date(ts: Option<&String>) -> String {
    ts.map(|t| t.chars().take(10).collect())
        .unwrap_or_else(|| "unknown".into())
}

fn short_type(t: &str) -> &str {
    t.strip_prefix("ssh-")
        .or_else(|| t.strip_prefix("ecdsa-sha2-"))
        .unwrap_or(t)
}

impl KeysData {
    pub fn key(&self, id: &str) -> Option<&SshKeyMeta> {
        self.keys.iter().find(|k| k.id.to_string() == id)
    }

    pub fn items(&self) -> Vec<Item> {
        self.keys
            .iter()
            .map(|k| {
                Item::leaf(k.label.clone(), 0)
                    .with_id(k.id.to_string())
                    .with_meta(format!(
                        "{}  {}  {}",
                        short_type(&k.key_type),
                        k.fingerprint,
                        date(self.created.get(&k.id))
                    ))
            })
            .collect()
    }

    /// Public information only; the private key never reaches the UI.
    pub fn detail(&self, id: &str) -> Option<String> {
        let k = self.key(id)?;
        Some(format!(
            "{}\n\nType         {}\nFingerprint  {}\nCreated      {}\n\n{}",
            k.label,
            k.key_type,
            k.fingerprint,
            date(self.created.get(&k.id)),
            k.public_key
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> KeysData {
        let k = SshKeyMeta {
            id: KeyId::new(),
            label: "laptop".into(),
            key_type: "ssh-ed25519".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            fingerprint: "SHA256:abc".into(),
        };
        let mut created = HashMap::new();
        created.insert(k.id, "2026-11-01T10:00:00Z".to_owned());
        KeysData {
            keys: vec![k],
            created,
        }
    }

    #[test]
    fn rows_show_type_fingerprint_and_date() {
        let items = data().items();
        assert_eq!(items[0].meta, "ed25519  SHA256:abc  2026-11-01");
    }

    #[test]
    fn detail_has_public_key_only() {
        let d = data();
        let id = d.keys[0].id.to_string();
        let t = d.detail(&id).unwrap();
        assert!(t.contains("ssh-ed25519 AAAA") && t.contains("2026-11-01"));
        assert!(d.detail("x").is_none());
    }
}
