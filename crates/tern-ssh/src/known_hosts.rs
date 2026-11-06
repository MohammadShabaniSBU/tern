//! Host-key verification policy (S4-T03). Pure logic plus a storage trait the app implements.

use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The key a server presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyInfo {
    /// e.g. `ssh-ed25519`
    pub key_type: String,
    /// `SHA256:…`
    pub fingerprint: String,
    /// `authorized_keys`-style line.
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyVerdict {
    /// Matches what we stored.
    Known,
    /// Never seen for this host/port/key type.
    Unknown,
    /// A different key than the stored one: possible MITM.
    Mismatch { old_fingerprint: String },
}

/// Compare a stored fingerprint (if any) with the presented one.
pub fn verdict(stored: Option<&str>, presented: &str) -> HostKeyVerdict {
    match stored {
        None => HostKeyVerdict::Unknown,
        Some(s) if s == presented => HostKeyVerdict::Known,
        Some(s) => HostKeyVerdict::Mismatch {
            old_fingerprint: s.to_owned(),
        },
    }
}

/// Persistent known-hosts storage, implemented by the app on top of `tern-store`.
pub trait KnownHostsStore: Send + Sync + 'static {
    /// Stored fingerprint for this host/port/key type.
    fn lookup<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        key_type: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, String>>;

    /// Save an accepted key. `replace` overwrites an existing entry (mismatch case).
    fn save<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        key: &'a HostKeyInfo,
        replace: bool,
    ) -> BoxFuture<'a, Result<(), String>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_known_mismatch() {
        assert_eq!(verdict(None, "SHA256:a"), HostKeyVerdict::Unknown);
        assert_eq!(verdict(Some("SHA256:a"), "SHA256:a"), HostKeyVerdict::Known);
        assert_eq!(
            verdict(Some("SHA256:a"), "SHA256:b"),
            HostKeyVerdict::Mismatch {
                old_fingerprint: "SHA256:a".into()
            }
        );
    }
}
