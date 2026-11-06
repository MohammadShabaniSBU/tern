use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Generate, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use secrecy::{ExposeSecret, SecretBox, SecretString};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Ciphertext format version written as the first byte of every sealed blob.
pub const VERSION: u8 = 1;
/// Symmetric key length in bytes (256-bit).
pub const KEY_LEN: usize = 32;
/// Recommended salt length in bytes.
pub const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const MIN_SALT_LEN: usize = 8;

/// A 256-bit symmetric key. Zeroized on drop; `Debug` prints `[REDACTED]`.
#[derive(Debug)]
pub struct Key(SecretBox<[u8; KEY_LEN]>);

impl Key {
    /// A fresh random key from the OS RNG.
    pub fn generate() -> Result<Self> {
        let mut bytes = <[u8; KEY_LEN]>::try_generate().map_err(|_| Error::Rng)?;
        let key = Self::from_bytes(&bytes);
        zeroize::Zeroize::zeroize(&mut bytes);
        Ok(key)
    }

    pub fn from_bytes(bytes: &[u8; KEY_LEN]) -> Self {
        Self(SecretBox::init_with(|| *bytes))
    }

    /// Raw key bytes. Only for persisting a *wrapped* key or for tests; never log the result.
    pub fn expose(&self) -> &[u8; KEY_LEN] {
        self.0.expose_secret()
    }
}

/// Argon2id cost parameters. Stored alongside the vault so they can be raised later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub m_cost: u32,
    /// Number of passes.
    pub t_cost: u32,
    /// Degree of parallelism.
    pub p_cost: u32,
}

impl Default for KdfParams {
    /// 128 MiB, 8 passes, 1 lane: ~420 ms on the author's desktop (release build). Re-run
    /// `tuning_probe` in the tests to re-tune; the params are stored with the vault.
    fn default() -> Self {
        Self {
            m_cost: 128 * 1024,
            t_cost: 8,
            p_cost: 1,
        }
    }
}

impl KdfParams {
    pub fn new(m_cost: u32, t_cost: u32, p_cost: u32) -> Result<Self> {
        let params = Self {
            m_cost,
            t_cost,
            p_cost,
        };
        params.to_argon2()?;
        Ok(params)
    }

    fn to_argon2(self) -> Result<Argon2<'static>> {
        let params = Params::new(self.m_cost, self.t_cost, self.p_cost, Some(KEY_LEN))
            .map_err(|e| Error::InvalidKdfParams(e.to_string()))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }
}

/// A random salt for [`derive_kek`].
pub fn generate_salt() -> Result<[u8; SALT_LEN]> {
    <[u8; SALT_LEN]>::try_generate().map_err(|_| Error::Rng)
}

/// Derive the key-encryption key from the master password with Argon2id.
pub fn derive_kek(password: &SecretString, salt: &[u8], params: &KdfParams) -> Result<Key> {
    if salt.len() < MIN_SALT_LEN {
        return Err(Error::InvalidKdfParams(format!(
            "salt must be at least {MIN_SALT_LEN} bytes"
        )));
    }
    let argon2 = params.to_argon2()?;
    let mut failure = None;
    let secret = SecretBox::<[u8; KEY_LEN]>::init_with_mut(|out| {
        if let Err(e) = argon2.hash_password_into(password.expose_secret().as_bytes(), salt, out) {
            failure = Some(Error::Kdf(e.to_string()));
        }
    });
    match failure {
        Some(e) => Err(e),
        None => Ok(Key(secret)),
    }
}

/// Encrypt `plaintext` with XChaCha20-Poly1305. Output: `version(1) || nonce(24) || ciphertext`.
///
/// `aad` is authenticated but not stored; the same value must be passed to [`open`].
pub fn seal(key: &Key, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).map_err(|_| Error::Encrypt)?;
    let nonce = XNonce::try_generate().map_err(|_| Error::Rng)?;
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::Encrypt)?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    out.push(VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`seal`]. Fails on wrong key, wrong `aad`, tampering or an
/// unknown version byte.
pub fn open(key: &Key, blob: &[u8], aad: &[u8]) -> Result<SecretBox<Vec<u8>>> {
    let (&version, rest) = blob.split_first().ok_or(Error::Truncated)?;
    if version != VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    if rest.len() < NONCE_LEN {
        return Err(Error::Truncated);
    }
    let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).map_err(|_| Error::Decrypt)?;
    let nonce = XNonce::try_from(nonce).map_err(|_| Error::Truncated)?;
    let plaintext = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| Error::Decrypt)?;
    Ok(SecretBox::new(Box::new(plaintext)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap parameters so tests stay fast.
    fn fast() -> KdfParams {
        KdfParams::new(8, 1, 1).unwrap()
    }

    fn pw(s: &str) -> SecretString {
        SecretString::from(s)
    }

    #[test]
    fn round_trip() {
        let key = Key::generate().unwrap();
        let blob = seal(&key, b"hunter2", b"identities:password_enc:1").unwrap();
        let plain = open(&key, &blob, b"identities:password_enc:1").unwrap();
        assert_eq!(plain.expose_secret().as_slice(), b"hunter2");
    }

    #[test]
    fn empty_plaintext_round_trips() {
        let key = Key::generate().unwrap();
        let blob = seal(&key, b"", b"").unwrap();
        assert!(open(&key, &blob, b"").unwrap().expose_secret().is_empty());
    }

    #[test]
    fn blob_layout() {
        let key = Key::generate().unwrap();
        let blob = seal(&key, b"abc", b"").unwrap();
        assert_eq!(blob[0], VERSION);
        // version + nonce + plaintext + 16-byte Poly1305 tag
        assert_eq!(blob.len(), 1 + NONCE_LEN + 3 + 16);
    }

    #[test]
    fn nonces_are_unique() {
        let key = Key::generate().unwrap();
        let a = seal(&key, b"x", b"").unwrap();
        let b = seal(&key, b"x", b"").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn wrong_key_fails() {
        let blob = seal(&Key::generate().unwrap(), b"secret", b"a").unwrap();
        let err = open(&Key::generate().unwrap(), &blob, b"a").unwrap_err();
        assert!(matches!(err, Error::Decrypt));
    }

    #[test]
    fn wrong_aad_fails() {
        let key = Key::generate().unwrap();
        let blob = seal(&key, b"secret", b"row:1").unwrap();
        assert!(matches!(
            open(&key, &blob, b"row:2").unwrap_err(),
            Error::Decrypt
        ));
    }

    #[test]
    fn tampering_fails_everywhere() {
        let key = Key::generate().unwrap();
        let blob = seal(&key, b"secret", b"a").unwrap();
        // Flip each byte after the version byte (nonce, ciphertext, tag).
        for i in 1..blob.len() {
            let mut bad = blob.clone();
            bad[i] ^= 0x01;
            assert!(
                open(&key, &bad, b"a").is_err(),
                "byte {i} not authenticated"
            );
        }
    }

    #[test]
    fn version_byte_is_checked() {
        let key = Key::generate().unwrap();
        let mut blob = seal(&key, b"secret", b"a").unwrap();
        blob[0] = VERSION + 1;
        assert!(matches!(
            open(&key, &blob, b"a").unwrap_err(),
            Error::UnsupportedVersion(v) if v == VERSION + 1
        ));
    }

    #[test]
    fn truncated_blobs_fail() {
        let key = Key::generate().unwrap();
        assert!(matches!(
            open(&key, &[], b"").unwrap_err(),
            Error::Truncated
        ));
        assert!(matches!(
            open(&key, &[VERSION, 1, 2, 3], b"").unwrap_err(),
            Error::Truncated
        ));
        // Right length for a nonce but no tag.
        assert!(open(&key, &[VERSION; 1 + NONCE_LEN], b"").is_err());
    }

    #[test]
    fn kdf_is_deterministic_and_input_sensitive() {
        let salt = [7u8; SALT_LEN];
        let a = derive_kek(&pw("correct horse"), &salt, &fast()).unwrap();
        let b = derive_kek(&pw("correct horse"), &salt, &fast()).unwrap();
        assert_eq!(a.expose(), b.expose());

        let other_pw = derive_kek(&pw("correct horsf"), &salt, &fast()).unwrap();
        assert_ne!(a.expose(), other_pw.expose());

        let other_salt = derive_kek(&pw("correct horse"), &[8u8; SALT_LEN], &fast()).unwrap();
        assert_ne!(a.expose(), other_salt.expose());

        let other_params = derive_kek(
            &pw("correct horse"),
            &salt,
            &KdfParams::new(16, 1, 1).unwrap(),
        )
        .unwrap();
        assert_ne!(a.expose(), other_params.expose());
    }

    #[test]
    fn kdf_rejects_bad_inputs() {
        assert!(derive_kek(&pw("x"), &[1, 2, 3], &fast()).is_err());
        assert!(KdfParams::new(1, 1, 1).is_err()); // memory below Argon2 minimum
        assert!(KdfParams::new(8, 0, 1).is_err()); // zero passes
    }

    #[test]
    fn kdf_params_serialize() {
        let json = serde_json::to_string(&KdfParams::default()).unwrap();
        assert_eq!(json, r#"{"m_cost":131072,"t_cost":8,"p_cost":1}"#);
    }

    #[test]
    fn salts_are_random() {
        assert_ne!(generate_salt().unwrap(), generate_salt().unwrap());
    }

    #[test]
    fn key_debug_is_redacted() {
        let key = Key::from_bytes(&[0xAB; KEY_LEN]);
        let dbg = format!("{key:?}");
        assert!(!dbg.to_lowercase().contains("ab"));
        assert!(dbg.contains("REDACTED"));
    }

    /// Run with `cargo test -p tern-vault --release -- --ignored --nocapture` to time the default
    /// parameters on this machine.
    #[test]
    #[ignore]
    fn tuning_probe() {
        let salt = [1u8; SALT_LEN];
        for params in [
            KdfParams::default(),
            KdfParams::new(64 * 1024, 3, 1).unwrap(),
            KdfParams::new(256 * 1024, 3, 1).unwrap(),
        ] {
            let start = std::time::Instant::now();
            derive_kek(&pw("benchmark"), &salt, &params).unwrap();
            println!("{params:?}: {:?}", start.elapsed());
        }
    }
}
