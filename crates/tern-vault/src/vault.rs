//! Envelope key management (ADR-1): a random data key (DK) encrypts secrets; the DK itself is
//! wrapped with a key-encryption key (KEK) derived from the master password.

use secrecy::{ExposeSecret, SecretBox, SecretString};

use crate::{Error, KdfParams, Key, Result, derive_kek, generate_salt, open, seal};

/// AAD binding the wrapped DK to its slot, so it cannot be swapped with another ciphertext.
const WRAP_AAD: &[u8] = b"vault_meta:wrapped_data_key:1";

/// What must be persisted to unlock the vault later (the single `vault_meta` row).
///
/// Contains no secret in the clear: `wrapped_data_key` is the DK sealed under the KEK
/// (`version || nonce || ciphertext`, so the nonce is inside the blob).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultMeta {
    pub kdf_params: KdfParams,
    pub kdf_salt: Vec<u8>,
    pub wrapped_data_key: Vec<u8>,
}

/// An unlocked vault: holds the data key in memory.
///
/// The key lives in a [`SecretBox`], which zeroizes it on drop; [`Vault::lock`] simply consumes
/// the vault. `Debug` never prints key material.
#[derive(Debug)]
pub struct Vault {
    data_key: Key,
}

fn require_password(password: &SecretString) -> Result<()> {
    if password.expose_secret().is_empty() {
        Err(Error::EmptyPassword)
    } else {
        Ok(())
    }
}

fn wrap(data_key: &Key, password: &SecretString, params: KdfParams) -> Result<VaultMeta> {
    let salt = generate_salt()?;
    let kek = derive_kek(password, &salt, &params)?;
    let wrapped_data_key = seal(&kek, data_key.expose(), WRAP_AAD)?;
    Ok(VaultMeta {
        kdf_params: params,
        kdf_salt: salt.to_vec(),
        wrapped_data_key,
    })
}

fn unwrap_data_key(password: &SecretString, meta: &VaultMeta) -> Result<Key> {
    let kek = derive_kek(password, &meta.kdf_salt, &meta.kdf_params)?;
    let plain = open(&kek, &meta.wrapped_data_key, WRAP_AAD).map_err(|e| match e {
        // Authentication failure on the wrapped key means the password is wrong.
        Error::Decrypt => Error::WrongPassword,
        other => other,
    })?;
    let bytes: &[u8; crate::KEY_LEN] = plain
        .expose_secret()
        .as_slice()
        .try_into()
        .map_err(|_| Error::Other("wrapped data key has the wrong length".into()))?;
    Ok(Key::from_bytes(bytes))
}

impl Vault {
    /// Create a new vault with the default KDF parameters.
    pub fn create(password: &SecretString) -> Result<(Self, VaultMeta)> {
        Self::create_with_params(password, KdfParams::default())
    }

    /// Create a new vault with explicit KDF parameters (tests use cheap ones).
    pub fn create_with_params(
        password: &SecretString,
        params: KdfParams,
    ) -> Result<(Self, VaultMeta)> {
        require_password(password)?;
        let data_key = Key::generate()?;
        let meta = wrap(&data_key, password, params)?;
        Ok((Self { data_key }, meta))
    }

    /// Unlock an existing vault. Returns [`Error::WrongPassword`] on a bad password.
    pub fn unlock(password: &SecretString, meta: &VaultMeta) -> Result<Self> {
        Ok(Self {
            data_key: unwrap_data_key(password, meta)?,
        })
    }

    /// Re-wrap the data key under a new password (fresh salt). No stored secrets need
    /// re-encryption. `old` is verified against `meta`; persist the returned meta to commit.
    pub fn change_password(
        &self,
        old: &SecretString,
        new: &SecretString,
        meta: &VaultMeta,
    ) -> Result<VaultMeta> {
        self.rewrap(old, new, meta.kdf_params, meta)
    }

    /// Re-wrap the data key under `new` with the given KDF cost (fresh salt). Used to change the
    /// password, the Argon2 cost, or both; `old` is verified against `meta`.
    pub fn rewrap(
        &self,
        old: &SecretString,
        new: &SecretString,
        params: KdfParams,
        meta: &VaultMeta,
    ) -> Result<VaultMeta> {
        require_password(new)?;
        unwrap_data_key(old, meta)?;
        wrap(&self.data_key, new, params)
    }

    /// Lock the vault, dropping (and zeroizing) the data key.
    pub fn lock(self) {}

    /// Encrypt a secret field. `aad` should be `"<table>:<column>:<row id>"`.
    pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        seal(&self.data_key, plaintext, aad)
    }

    /// Decrypt a secret field produced by [`Vault::seal`].
    pub fn open(&self, blob: &[u8], aad: &[u8]) -> Result<SecretBox<Vec<u8>>> {
        open(&self.data_key, blob, aad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s)
    }

    fn fast() -> KdfParams {
        KdfParams::new(8, 1, 1).unwrap()
    }

    fn create(password: &str) -> (Vault, VaultMeta) {
        Vault::create_with_params(&pw(password), fast()).unwrap()
    }

    #[test]
    fn create_then_unlock() {
        let (vault, meta) = create("master");
        let blob = vault.seal(b"secret", b"t:c:1").unwrap();

        let again = Vault::unlock(&pw("master"), &meta).unwrap();
        assert_eq!(
            again
                .open(&blob, b"t:c:1")
                .unwrap()
                .expose_secret()
                .as_slice(),
            b"secret"
        );
    }

    #[test]
    fn wrong_password_is_rejected() {
        let (_vault, meta) = create("master");
        assert!(matches!(
            Vault::unlock(&pw("Master"), &meta).unwrap_err(),
            Error::WrongPassword
        ));
    }

    #[test]
    fn tampered_meta_is_rejected() {
        let (_vault, mut meta) = create("master");
        let last = meta.wrapped_data_key.len() - 1;
        meta.wrapped_data_key[last] ^= 1;
        assert!(Vault::unlock(&pw("master"), &meta).is_err());

        let (_vault, mut meta) = create("master");
        meta.kdf_salt[0] ^= 1;
        assert!(Vault::unlock(&pw("master"), &meta).is_err());
    }

    #[test]
    fn empty_password_is_rejected() {
        assert!(matches!(
            Vault::create_with_params(&pw(""), fast()).unwrap_err(),
            Error::EmptyPassword
        ));
        let (vault, meta) = create("master");
        assert!(matches!(
            vault
                .change_password(&pw("master"), &pw(""), &meta)
                .unwrap_err(),
            Error::EmptyPassword
        ));
    }

    #[test]
    fn same_password_gives_distinct_vaults() {
        let (_a, meta_a) = create("master");
        let (_b, meta_b) = create("master");
        assert_ne!(meta_a.kdf_salt, meta_b.kdf_salt);
        assert_ne!(meta_a.wrapped_data_key, meta_b.wrapped_data_key);
    }

    #[test]
    fn change_password_rewraps_without_reencrypting() {
        let (vault, meta) = create("old");
        let blob = vault.seal(b"secret", b"t:c:1").unwrap();

        let new_meta = vault
            .change_password(&pw("old"), &pw("new"), &meta)
            .unwrap();
        assert_ne!(new_meta.kdf_salt, meta.kdf_salt);
        assert_eq!(new_meta.kdf_params, meta.kdf_params);

        // Old password no longer works on the new meta; new one does.
        assert!(matches!(
            Vault::unlock(&pw("old"), &new_meta).unwrap_err(),
            Error::WrongPassword
        ));
        let unlocked = Vault::unlock(&pw("new"), &new_meta).unwrap();
        // Existing ciphertext still opens: the data key did not change.
        assert_eq!(
            unlocked
                .open(&blob, b"t:c:1")
                .unwrap()
                .expose_secret()
                .as_slice(),
            b"secret"
        );
    }

    #[test]
    fn rewrap_changes_the_kdf_cost_and_keeps_the_data_key() {
        let (vault, meta) = create("pw");
        let blob = vault.seal(b"s", b"a").unwrap();
        let costly = KdfParams::new(16, 2, 1).unwrap();
        let new_meta = vault.rewrap(&pw("pw"), &pw("pw"), costly, &meta).unwrap();
        assert_eq!(new_meta.kdf_params, costly);
        assert_ne!(new_meta.kdf_salt, meta.kdf_salt);
        let again = Vault::unlock(&pw("pw"), &new_meta).unwrap();
        assert_eq!(
            again.open(&blob, b"a").unwrap().expose_secret().as_slice(),
            b"s"
        );
        assert!(vault.rewrap(&pw("bad"), &pw("pw"), costly, &meta).is_err());
    }

    #[test]
    fn change_password_requires_correct_old_password() {
        let (vault, meta) = create("old");
        assert!(matches!(
            vault
                .change_password(&pw("nope"), &pw("new"), &meta)
                .unwrap_err(),
            Error::WrongPassword
        ));
    }

    #[test]
    fn lock_consumes_the_vault() {
        let (vault, meta) = create("master");
        vault.lock();
        // Can be unlocked again from the persisted meta.
        assert!(Vault::unlock(&pw("master"), &meta).is_ok());
    }

    #[test]
    fn debug_never_shows_the_data_key() {
        let (vault, _meta) = create("master");
        let dbg = format!("{vault:?}");
        assert!(dbg.contains("REDACTED"), "{dbg}");

        let hex: String = vault
            .data_key
            .expose()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let decimal = format!("{:?}", vault.data_key.expose());
        assert!(!dbg.to_lowercase().contains(&hex));
        assert!(!dbg.contains(&decimal));
    }
}
