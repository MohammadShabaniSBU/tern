/// Errors produced by `tern-vault`. Messages never contain key material or plaintext.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid key-derivation parameters: {0}")]
    InvalidKdfParams(String),
    #[error("key derivation failed: {0}")]
    Kdf(String),
    #[error("system random number generator failed")]
    Rng,
    #[error("encryption failed")]
    Encrypt,
    /// Wrong key, wrong associated data, or tampered/corrupt ciphertext. Deliberately not more
    /// specific: callers (and attackers) learn nothing beyond "it did not authenticate".
    #[error("decryption failed")]
    Decrypt,
    #[error("wrong master password")]
    WrongPassword,
    #[error("master password must not be empty")]
    EmptyPassword,
    #[error("ciphertext is too short")]
    Truncated,
    #[error("unsupported ciphertext version {0}")]
    UnsupportedVersion(u8),
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
