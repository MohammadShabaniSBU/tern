//! Encrypted backup bundle (`tern-backup`, version 1). See `docs/EXPORT_FORMAT.md`.

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tern_core::{Group, Host, Identity, KnownHost, SshKeyMeta, Tunnel};
use tern_vault::{KdfParams, derive_kek, generate_salt, open, seal};

use crate::{Error, Result};

pub const FORMAT: &str = "tern-backup";
pub const VERSION: u32 = 1;
const CIPHER: &str = "xchacha20poly1305";
const AAD: &[u8] = b"tern-backup:v1";
/// Upper bounds accepted from a file, so a hostile bundle cannot demand absurd work.
const MAX_M_COST: u32 = 1024 * 1024;
const MAX_T_COST: u32 = 64;
const MAX_P_COST: u32 = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfInfo {
    pub alg: String,
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    pub salt: String,
}

/// Outer, unencrypted envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub version: u32,
    pub created_at: String,
    pub kdf: KdfInfo,
    pub cipher: String,
    /// base64 of `tern_vault::seal` output (`version || nonce || ciphertext+tag`).
    pub payload: String,
}

/// An identity plus its optional saved password.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityEntry {
    #[serde(flatten)]
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

/// An SSH key's metadata plus its secrets.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyEntry {
    #[serde(flatten)]
    pub meta: SshKeyMeta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
}

impl std::fmt::Debug for IdentityEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityEntry")
            .field("identity", &self.identity)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl std::fmt::Debug for KeyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyEntry")
            .field("meta", &self.meta)
            .field("secrets", &"<redacted>")
            .finish()
    }
}

/// Decrypted contents. Contains secrets in plain strings: keep short-lived, never log.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Payload {
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub hosts: Vec<Host>,
    #[serde(default)]
    pub identities: Vec<IdentityEntry>,
    #[serde(default)]
    pub ssh_keys: Vec<KeyEntry>,
    #[serde(default)]
    pub tunnels: Vec<Tunnel>,
    #[serde(default)]
    pub known_hosts: Vec<KnownHost>,
    /// `(key, json-text value)` pairs.
    #[serde(default)]
    pub settings: Vec<(String, String)>,
}

/// KDF cost for new bundles (64 MiB, 3 passes).
fn export_params() -> KdfParams {
    KdfParams {
        m_cost: 65536,
        t_cost: 3,
        p_cost: 1,
    }
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339(secs)
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (civil-from-days algorithm).
pub fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn map_vault(e: tern_vault::Error) -> Error {
    Error::Other(e.to_string())
}

/// Encrypt `payload` under `password` and return the bundle file bytes (pretty JSON).
pub fn encrypt(payload: &Payload, password: &SecretString) -> Result<Vec<u8>> {
    let params = export_params();
    let salt = generate_salt().map_err(map_vault)?;
    let key = derive_kek(password, &salt, &params).map_err(map_vault)?;
    let mut plain =
        serde_json::to_vec(payload).map_err(|e| Error::Other(format!("serialize: {e}")))?;
    let sealed = seal(&key, &plain, AAD);
    zeroize::Zeroize::zeroize(&mut plain);
    let sealed = sealed.map_err(map_vault)?;
    let bundle = Bundle {
        format: FORMAT.into(),
        version: VERSION,
        created_at: now_rfc3339(),
        kdf: KdfInfo {
            alg: "argon2id".into(),
            m_cost: params.m_cost,
            t_cost: params.t_cost,
            p_cost: params.p_cost,
            salt: B64.encode(salt),
        },
        cipher: CIPHER.into(),
        payload: B64.encode(sealed),
    };
    serde_json::to_vec_pretty(&bundle).map_err(|e| Error::Other(format!("serialize: {e}")))
}

/// Parse the envelope only (no password needed). Validates format/version/cipher.
pub fn parse(bytes: &[u8]) -> Result<Bundle> {
    let bundle: Bundle = serde_json::from_slice(bytes).map_err(|_| Error::NotABundle)?;
    if bundle.format != FORMAT {
        return Err(Error::NotABundle);
    }
    if bundle.version != VERSION {
        return Err(Error::UnsupportedVersion(bundle.version));
    }
    if bundle.cipher != CIPHER || bundle.kdf.alg != "argon2id" {
        return Err(Error::Invalid("unsupported cipher or KDF".into()));
    }
    let k = &bundle.kdf;
    if k.m_cost > MAX_M_COST || k.t_cost > MAX_T_COST || k.p_cost > MAX_P_COST {
        return Err(Error::Invalid("KDF parameters out of range".into()));
    }
    Ok(bundle)
}

/// Decrypt a bundle. A wrong password and a tampered file are indistinguishable by design.
pub fn decrypt(bytes: &[u8], password: &SecretString) -> Result<Payload> {
    let bundle = parse(bytes)?;
    let salt = B64
        .decode(&bundle.kdf.salt)
        .map_err(|_| Error::Invalid("bad salt encoding".into()))?;
    let sealed = B64
        .decode(&bundle.payload)
        .map_err(|_| Error::Invalid("bad payload encoding".into()))?;
    let params = KdfParams::new(bundle.kdf.m_cost, bundle.kdf.t_cost, bundle.kdf.p_cost)
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let key = derive_kek(password, &salt, &params).map_err(|e| Error::Invalid(e.to_string()))?;
    let plain = open(&key, &sealed, AAD).map_err(|_| Error::WrongPasswordOrCorrupt)?;
    serde_json::from_slice(plain.expose_secret())
        .map_err(|e| Error::Invalid(format!("payload: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tern_core::{Host, KeyId};

    #[test]
    fn secret_bearing_entries_never_debug_print_secrets() {
        let id = IdentityEntry {
            identity: tern_core::Identity {
                id: tern_core::IdentityId::new(),
                label: "l".into(),
                username: "u".into(),
                ssh_key_id: None,
            },
            password: Some("hunter2-secret".into()),
        };
        let key = KeyEntry {
            meta: SshKeyMeta {
                id: KeyId::new(),
                label: "k".into(),
                key_type: "ssh-ed25519".into(),
                public_key: "pub".into(),
                fingerprint: "fp".into(),
            },
            private_key: Some("PRIVATE-KEY-BODY".into()),
            passphrase: Some("pass-phrase-secret".into()),
        };
        let payload = Payload {
            identities: vec![id.clone()],
            ssh_keys: vec![key.clone()],
            ..Payload::default()
        };
        let dbg = format!("{id:?} {key:?} {payload:?}");
        for needle in ["hunter2", "PRIVATE-KEY-BODY", "pass-phrase-secret"] {
            assert!(!dbg.contains(needle), "{needle} leaked: {dbg}");
        }
    }

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn sample() -> Payload {
        let mut p = Payload::default();
        p.hosts.push(Host::new("web", "example.com"));
        p.ssh_keys.push(KeyEntry {
            meta: SshKeyMeta {
                id: KeyId::new(),
                label: "k".into(),
                key_type: "ssh-ed25519".into(),
                public_key: "ssh-ed25519 AAAA".into(),
                fingerprint: "SHA256:x".into(),
            },
            private_key: Some("PRIVATE".into()),
            passphrase: None,
        });
        p.settings.push(("theme".into(), "\"dark\"".into()));
        p
    }

    #[test]
    fn round_trip() {
        let p = sample();
        let bytes = encrypt(&p, &pw("hunter2")).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE"));
        assert_eq!(decrypt(&bytes, &pw("hunter2")).unwrap(), p);
    }

    #[test]
    fn wrong_password() {
        let bytes = encrypt(&sample(), &pw("a")).unwrap();
        assert!(matches!(
            decrypt(&bytes, &pw("b")),
            Err(Error::WrongPasswordOrCorrupt)
        ));
    }

    #[test]
    fn tampered_payload() {
        let bytes = encrypt(&sample(), &pw("a")).unwrap();
        let mut b: Bundle = serde_json::from_slice(&bytes).unwrap();
        let mut raw = B64.decode(&b.payload).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        b.payload = B64.encode(raw);
        let bytes = serde_json::to_vec(&b).unwrap();
        assert!(matches!(
            decrypt(&bytes, &pw("a")),
            Err(Error::WrongPasswordOrCorrupt)
        ));
    }

    #[test]
    fn rejects_garbage_and_versions() {
        assert!(matches!(decrypt(b"nope", &pw("a")), Err(Error::NotABundle)));
        assert!(matches!(decrypt(b"{}", &pw("a")), Err(Error::NotABundle)));
        let bytes = encrypt(&sample(), &pw("a")).unwrap();
        let mut b: Bundle = serde_json::from_slice(&bytes).unwrap();
        b.version = 99;
        let bytes = serde_json::to_vec(&b).unwrap();
        assert!(matches!(
            decrypt(&bytes, &pw("a")),
            Err(Error::UnsupportedVersion(99))
        ));
        b.version = 1;
        b.kdf.m_cost = u32::MAX;
        let bytes = serde_json::to_vec(&b).unwrap();
        assert!(matches!(decrypt(&bytes, &pw("a")), Err(Error::Invalid(_))));
    }

    #[test]
    fn rfc3339_known_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_709_210_096), "2024-02-29T12:34:56Z");
    }
}
