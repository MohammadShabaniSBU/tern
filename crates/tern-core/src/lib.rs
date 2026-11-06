//! Domain types for tern. Pure data and validation: no I/O, no GUI, no secrets.

mod error;
mod ids;
mod model;
mod validate;

pub use error::ValidationError;
pub use ids::{GroupId, HostId, IdentityId, KeyId, TunnelId};
pub use model::{Group, Host, Identity, SshKeyMeta, Tunnel, TunnelKind};
pub use validate::{
    MAX_LABEL_LEN, validate_hostname, validate_label, validate_port, validate_username,
};
