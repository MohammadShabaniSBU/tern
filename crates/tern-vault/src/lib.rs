//! Key derivation, authenticated encryption and secret handling for tern.
//!
//! Nothing in this crate ever logs or `Debug`-prints key material: keys and decrypted plaintexts
//! are wrapped in [`secrecy::SecretBox`] (zeroized on drop, redacted `Debug`).

mod crypto;
mod error;

pub use crypto::{
    KEY_LEN, KdfParams, Key, SALT_LEN, VERSION, derive_kek, generate_salt, open, seal,
};
pub use error::{Error, Result};
pub use secrecy::{ExposeSecret, SecretBox, SecretString};
