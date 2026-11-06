//! Plaintext JSON export (opt-in; secrets excluded unless explicitly requested).

use crate::bundle::Payload;
use crate::merge::strip_secrets;
use crate::{Error, Result};

/// Serialize `payload` as pretty JSON. With `include_secrets == false` all passwords, private
/// keys and passphrases are removed first.
pub fn to_json(mut payload: Payload, include_secrets: bool) -> Result<Vec<u8>> {
    if !include_secrets {
        strip_secrets(&mut payload);
    }
    serde_json::to_vec_pretty(&payload).map_err(|e| Error::Other(format!("serialize: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::KeyEntry;
    use tern_core::{KeyId, SshKeyMeta};

    #[test]
    fn secrets_excluded_by_default() {
        let mut p = Payload::default();
        p.ssh_keys.push(KeyEntry {
            meta: SshKeyMeta {
                id: KeyId::new(),
                label: "k".into(),
                key_type: "t".into(),
                public_key: "p".into(),
                fingerprint: "f".into(),
            },
            private_key: Some("SECRETPEM".into()),
            passphrase: Some("SECRETPASS".into()),
        });
        let without = String::from_utf8(to_json(p.clone(), false).unwrap()).unwrap();
        assert!(!without.contains("SECRET"));
        let with = String::from_utf8(to_json(p, true).unwrap()).unwrap();
        assert!(with.contains("SECRETPEM") && with.contains("SECRETPASS"));
    }
}
