use serde::{Deserialize, Serialize};

use crate::validate::{validate_hostname, validate_label, validate_port, validate_username};
use crate::{GroupId, HostId, IdentityId, KeyId, KnownHostId, TunnelId, ValidationError};

/// A folder for organising hosts. Groups may nest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub parent_id: Option<GroupId>,
    pub sort_order: i32,
}

impl Group {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_label(&self.name)
    }
}

/// A saved SSH destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub id: HostId,
    pub label: String,
    pub hostname: String,
    pub port: u16,
    /// Overrides the identity's username when set.
    pub username: Option<String>,
    pub identity_id: Option<IdentityId>,
    pub group_id: Option<GroupId>,
    pub jump_host_id: Option<HostId>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub keepalive_secs: Option<u32>,
}

impl Host {
    /// A new host with a fresh id and the default SSH port.
    pub fn new(label: impl Into<String>, hostname: impl Into<String>) -> Self {
        Self {
            id: HostId::new(),
            label: label.into(),
            hostname: hostname.into(),
            port: 22,
            username: None,
            identity_id: None,
            group_id: None,
            jump_host_id: None,
            tags: Vec::new(),
            notes: None,
            keepalive_secs: None,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_label(&self.label)?;
        validate_hostname(&self.hostname)?;
        validate_port(self.port)?;
        if let Some(username) = &self.username {
            validate_username(username)?;
        }
        Ok(())
    }
}

/// Login details. Secret material (password, key contents) is deliberately **not** part of this
/// type; the storage layer keeps it encrypted separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub id: IdentityId,
    pub label: String,
    pub username: String,
    pub ssh_key_id: Option<KeyId>,
}

impl Identity {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_label(&self.label)?;
        validate_username(&self.username)
    }
}

/// Public metadata about an SSH key. Never contains private material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshKeyMeta {
    pub id: KeyId,
    pub label: String,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint: String,
}

impl SshKeyMeta {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_label(&self.label)
    }
}

/// A server host key we have seen and trusted (trust-on-first-use).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHost {
    pub id: KnownHostId,
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub public_key: String,
}

impl KnownHost {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_hostname(&self.host)?;
        validate_port(self.port)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelKind {
    /// `-L`: listen locally, forward to a target via the remote.
    Local,
    /// `-R`: listen on the remote, forward to a target locally.
    Remote,
    /// `-D`: local SOCKS5 proxy.
    Dynamic,
}

impl TunnelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
            Self::Dynamic => "dynamic",
        }
    }

    /// Inverse of [`TunnelKind::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "local" => Some(Self::Local),
            "remote" => Some(Self::Remote),
            "dynamic" => Some(Self::Dynamic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tunnel {
    pub id: TunnelId,
    pub label: String,
    pub host_id: HostId,
    pub kind: TunnelKind,
    pub bind_addr: String,
    pub bind_port: u16,
    /// `None` for dynamic tunnels.
    pub target_host: Option<String>,
    /// `None` for dynamic tunnels.
    pub target_port: Option<u16>,
    pub auto_start: bool,
}

impl Tunnel {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_label(&self.label)?;
        validate_hostname(&self.bind_addr)?;
        validate_port(self.bind_port)?;
        match (self.kind, &self.target_host, self.target_port) {
            (TunnelKind::Dynamic, None, None) => Ok(()),
            (TunnelKind::Dynamic, _, _) => Err(ValidationError::UnexpectedTarget),
            (_, Some(host), Some(port)) => {
                validate_hostname(host)?;
                validate_port(port)
            }
            (kind, _, _) => Err(ValidationError::MissingTarget(kind.as_str())),
        }
    }

    /// A warning to show (not an error) when the listener is reachable from other machines.
    /// Remote tunnels bind on the server, so this only applies to local and dynamic ones.
    pub fn bind_warning(&self) -> Option<&'static str> {
        if self.kind == TunnelKind::Remote {
            return None;
        }
        let a = self.bind_addr.trim();
        let loopback = a.eq_ignore_ascii_case("localhost")
            || a == "::1"
            || a == "[::1]"
            || a.starts_with("127.");
        (!loopback)
            .then_some("Binding to a non-loopback address exposes this tunnel to the network")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tunnel(kind: TunnelKind, target: Option<(&str, u16)>) -> Tunnel {
        Tunnel {
            id: TunnelId::new(),
            label: "t".into(),
            host_id: HostId::new(),
            kind,
            bind_addr: "127.0.0.1".into(),
            bind_port: 8080,
            target_host: target.map(|(h, _)| h.to_owned()),
            target_port: target.map(|(_, p)| p),
            auto_start: false,
        }
    }

    #[test]
    fn bind_warning_flags_non_loopback_local_binds() {
        let mut t = tunnel(TunnelKind::Local, Some(("db", 5432)));
        assert!(t.bind_warning().is_none());
        t.bind_addr = "0.0.0.0".into();
        assert!(t.bind_warning().is_some());
        t.kind = TunnelKind::Remote;
        assert!(t.bind_warning().is_none());
    }

    #[test]
    fn new_host_is_valid_with_default_port() {
        let host = Host::new("web", "web.example.com");
        assert_eq!(host.port, 22);
        assert!(host.validate().is_ok());
    }

    #[test]
    fn host_validation_rejects_bad_fields() {
        let mut host = Host::new("web", "web.example.com");
        host.port = 0;
        assert_eq!(host.validate(), Err(ValidationError::InvalidPort));

        let host = Host::new("web", "");
        assert!(host.validate().is_err());

        let host = Host::new("", "h");
        assert!(host.validate().is_err());

        let mut host = Host::new("web", "h");
        host.username = Some(String::new());
        assert!(host.validate().is_err());
    }

    #[test]
    fn tunnel_target_rules() {
        assert!(
            tunnel(TunnelKind::Local, Some(("db", 5432)))
                .validate()
                .is_ok()
        );
        assert!(
            tunnel(TunnelKind::Remote, Some(("localhost", 3000)))
                .validate()
                .is_ok()
        );
        assert!(tunnel(TunnelKind::Dynamic, None).validate().is_ok());
        assert_eq!(
            tunnel(TunnelKind::Local, None).validate(),
            Err(ValidationError::MissingTarget("local"))
        );
        assert_eq!(
            tunnel(TunnelKind::Dynamic, Some(("db", 1))).validate(),
            Err(ValidationError::UnexpectedTarget)
        );
        assert!(
            tunnel(TunnelKind::Local, Some(("db", 0)))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn ids_roundtrip_through_serde_and_str() {
        let id = HostId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<HostId>(&json).unwrap(), id);
        assert_eq!(id.to_string().parse::<HostId>().unwrap(), id);
        assert_ne!(HostId::new(), HostId::new());
    }

    #[test]
    fn tunnel_kind_parse_round_trips() {
        for kind in [TunnelKind::Local, TunnelKind::Remote, TunnelKind::Dynamic] {
            assert_eq!(TunnelKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(TunnelKind::parse("bogus"), None);
    }

    #[test]
    fn tunnel_kind_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&TunnelKind::Dynamic).unwrap(),
            "\"dynamic\""
        );
    }

    #[test]
    fn host_roundtrips_through_json() {
        let host = Host::new("web", "web.example.com");
        let json = serde_json::to_string(&host).unwrap();
        assert_eq!(serde_json::from_str::<Host>(&json).unwrap(), host);
    }
}
