use thiserror::Error;

/// A domain value failed validation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{field} is too long ({len} > {max} characters)")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    #[error("{field} contains invalid characters")]
    InvalidChars { field: &'static str },
    #[error("port must be between 1 and 65535")]
    InvalidPort,
    #[error("{0} tunnels require a target host and port")]
    MissingTarget(&'static str),
    #[error("dynamic tunnels must not have a target host or port")]
    UnexpectedTarget,
}
