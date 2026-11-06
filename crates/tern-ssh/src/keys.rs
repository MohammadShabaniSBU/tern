//! SSH key generation, parsing and fingerprints (S3-T04). Pure functions, no I/O.

use russh::keys::{Algorithm, EcdsaCurve, HashAlg, PrivateKey, ssh_key::LineEnding};
use secrecy::{ExposeSecret, SecretString};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Ed25519,
    Rsa4096,
    EcdsaP256,
}

impl KeyKind {
    pub const ALL: [KeyKind; 3] = [KeyKind::Ed25519, KeyKind::Rsa4096, KeyKind::EcdsaP256];

    pub fn label(self) -> &'static str {
        match self {
            KeyKind::Ed25519 => "Ed25519",
            KeyKind::Rsa4096 => "RSA 4096",
            KeyKind::EcdsaP256 => "ECDSA P-256",
        }
    }

    fn algorithm(self) -> Algorithm {
        match self {
            KeyKind::Ed25519 => Algorithm::Ed25519,
            KeyKind::Rsa4096 => Algorithm::Rsa { hash: None },
            KeyKind::EcdsaP256 => Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP256,
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("the key is encrypted; a passphrase is required")]
    PassphraseRequired,
    #[error("the passphrase is wrong")]
    WrongPassphrase,
    #[error("not a valid OpenSSH private key: {0}")]
    Invalid(String),
    #[error("key generation failed: {0}")]
    Generate(String),
}

/// A key ready to be stored. The private part is OpenSSH PEM (encrypted if a passphrase was
/// given); the metadata fields are public.
pub struct KeyMaterial {
    pub private_pem: SecretString,
    pub passphrase: Option<SecretString>,
    /// `authorized_keys` line, e.g. `ssh-ed25519 AAAA… comment`.
    pub public_key: String,
    pub key_type: String,
    /// `SHA256:…`
    pub fingerprint: String,
}

impl std::fmt::Debug for KeyMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyMaterial")
            .field("key_type", &self.key_type)
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

fn describe(
    key: &PrivateKey,
    passphrase: Option<&SecretString>,
    comment: &str,
) -> Result<KeyMaterial, KeyError> {
    let mut public = key.public_key().clone();
    if !comment.is_empty() {
        public.set_comment(comment);
    }
    let stored = match passphrase {
        Some(p) => key
            .encrypt(&mut rand::rng(), p.expose_secret())
            .map_err(|e| KeyError::Generate(e.to_string()))?,
        None => key.clone(),
    };
    let pem = stored
        .to_openssh(LineEnding::LF)
        .map_err(|e| KeyError::Invalid(e.to_string()))?;
    Ok(KeyMaterial {
        private_pem: SecretString::from(pem.as_str().to_owned()),
        passphrase: passphrase.cloned(),
        public_key: public
            .to_openssh()
            .map_err(|e| KeyError::Invalid(e.to_string()))?,
        key_type: key.algorithm().as_str().to_owned(),
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
    })
}

/// Generate a new key. `passphrase` encrypts the stored private key.
pub fn generate(
    kind: KeyKind,
    comment: &str,
    passphrase: Option<&SecretString>,
) -> Result<KeyMaterial, KeyError> {
    let key = PrivateKey::random(&mut rand::rng(), kind.algorithm())
        .map_err(|e| KeyError::Generate(e.to_string()))?;
    describe(&key, passphrase, comment)
}

/// Parse an OpenSSH private key (file contents or pasted text). Encrypted keys need their
/// `passphrase`; the key is stored as given (still encrypted) together with it.
pub fn import(
    text: &str,
    comment: &str,
    passphrase: Option<&SecretString>,
) -> Result<KeyMaterial, KeyError> {
    let parsed =
        PrivateKey::from_openssh(text.trim()).map_err(|e| KeyError::Invalid(e.to_string()))?;
    if !parsed.is_encrypted() {
        return describe(&parsed, None, comment);
    }
    let Some(pass) = passphrase else {
        return Err(KeyError::PassphraseRequired);
    };
    let plain = parsed
        .decrypt(pass.expose_secret())
        .map_err(|_| KeyError::WrongPassphrase)?;
    // Keep the original encrypted PEM; only the public metadata comes from the decrypted key.
    let mut m = describe(&plain, None, comment)?;
    m.private_pem = SecretString::from(text.trim().to_owned() + "\n");
    m.passphrase = Some(pass.clone());
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn ed25519_roundtrip_and_metadata() {
        let m = generate(KeyKind::Ed25519, "me@tern", None).unwrap();
        assert_eq!(m.key_type, "ssh-ed25519");
        assert!(m.fingerprint.starts_with("SHA256:"));
        assert!(m.public_key.starts_with("ssh-ed25519 "));
        assert!(m.public_key.ends_with(" me@tern"));
        let again = import(m.private_pem.expose_secret(), "", None).unwrap();
        assert_eq!(again.fingerprint, m.fingerprint);
    }

    #[test]
    fn ecdsa_generates() {
        let m = generate(KeyKind::EcdsaP256, "", None).unwrap();
        assert_eq!(m.key_type, "ecdsa-sha2-nistp256");
    }

    #[test]
    fn encrypted_key_needs_the_right_passphrase() {
        let m = generate(KeyKind::Ed25519, "", Some(&pass("hunter2"))).unwrap();
        let pem = m.private_pem.expose_secret().to_owned();
        assert!(matches!(
            import(&pem, "", None),
            Err(KeyError::PassphraseRequired)
        ));
        assert!(matches!(
            import(&pem, "", Some(&pass("nope"))),
            Err(KeyError::WrongPassphrase)
        ));
        let ok = import(&pem, "", Some(&pass("hunter2"))).unwrap();
        assert_eq!(ok.fingerprint, m.fingerprint);
        assert!(ok.passphrase.is_some());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(matches!(
            import("not a key", "", None),
            Err(KeyError::Invalid(_))
        ));
    }

    /// Acceptance: our public key equals `ssh-keygen -y` of the exported private key.
    #[test]
    fn public_key_matches_ssh_keygen() {
        let Ok(dir) = std::env::temp_dir()
            .join(format!("tern-key-test-{}", std::process::id()))
            .canonicalize()
            .or_else(|_| {
                let d = std::env::temp_dir().join(format!("tern-key-test-{}", std::process::id()));
                std::fs::create_dir_all(&d).map(|()| d)
            })
        else {
            return;
        };
        for kind in [KeyKind::Ed25519, KeyKind::EcdsaP256] {
            let m = generate(kind, "", None).unwrap();
            let path = dir.join(format!("{kind:?}"));
            std::fs::write(&path, m.private_pem.expose_secret()).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            let Ok(out) = std::process::Command::new("ssh-keygen")
                .args(["-y", "-f"])
                .arg(&path)
                .output()
            else {
                return; // ssh-keygen not installed
            };
            assert!(out.status.success());
            let theirs = String::from_utf8(out.stdout).unwrap();
            assert_eq!(theirs.trim(), m.public_key.trim());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rsa_4096_generates() {
        let m = generate(KeyKind::Rsa4096, "", None).unwrap();
        assert_eq!(m.key_type, "ssh-rsa");
    }
}
