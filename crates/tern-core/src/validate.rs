use crate::ValidationError;

/// Maximum length (in characters) of a label.
pub const MAX_LABEL_LEN: usize = 100;
const MAX_HOSTNAME_LEN: usize = 253;
const MAX_USERNAME_LEN: usize = 256;

/// Labels must be non-blank and at most [`MAX_LABEL_LEN`] characters.
pub fn validate_label(label: &str) -> Result<(), ValidationError> {
    let len = label.chars().count();
    if label.trim().is_empty() {
        return Err(ValidationError::Empty { field: "label" });
    }
    if len > MAX_LABEL_LEN {
        return Err(ValidationError::TooLong {
            field: "label",
            len,
            max: MAX_LABEL_LEN,
        });
    }
    Ok(())
}

/// Hostnames (or IP literals) must be non-empty, free of whitespace and control characters,
/// and at most 253 characters.
pub fn validate_hostname(hostname: &str) -> Result<(), ValidationError> {
    if hostname.trim().is_empty() {
        return Err(ValidationError::Empty { field: "hostname" });
    }
    if hostname.len() > MAX_HOSTNAME_LEN {
        return Err(ValidationError::TooLong {
            field: "hostname",
            len: hostname.len(),
            max: MAX_HOSTNAME_LEN,
        });
    }
    if hostname
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(ValidationError::InvalidChars { field: "hostname" });
    }
    Ok(())
}

/// Usernames must be non-empty, free of whitespace and control characters.
pub fn validate_username(username: &str) -> Result<(), ValidationError> {
    if username.is_empty() {
        return Err(ValidationError::Empty { field: "username" });
    }
    if username.len() > MAX_USERNAME_LEN {
        return Err(ValidationError::TooLong {
            field: "username",
            len: username.len(),
            max: MAX_USERNAME_LEN,
        });
    }
    if username
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(ValidationError::InvalidChars { field: "username" });
    }
    Ok(())
}

/// TCP ports are 1..=65535.
pub fn validate_port(port: u16) -> Result<(), ValidationError> {
    if port == 0 {
        Err(ValidationError::InvalidPort)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_rules() {
        assert!(validate_label("prod db").is_ok());
        assert!(validate_label(&"x".repeat(MAX_LABEL_LEN)).is_ok());
        assert_eq!(
            validate_label("   "),
            Err(ValidationError::Empty { field: "label" })
        );
        assert!(matches!(
            validate_label(&"x".repeat(MAX_LABEL_LEN + 1)),
            Err(ValidationError::TooLong { .. })
        ));
    }

    #[test]
    fn label_length_counts_chars_not_bytes() {
        assert!(validate_label(&"é".repeat(MAX_LABEL_LEN)).is_ok());
    }

    #[test]
    fn hostname_rules() {
        assert!(validate_hostname("example.com").is_ok());
        assert!(validate_hostname("192.168.1.10").is_ok());
        assert!(validate_hostname("::1").is_ok());
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname("  ").is_err());
        assert!(validate_hostname("bad host").is_err());
        assert!(validate_hostname("bad\nhost").is_err());
        assert!(validate_hostname(&"a".repeat(254)).is_err());
    }

    #[test]
    fn username_rules() {
        assert!(validate_username("root").is_ok());
        assert!(validate_username("").is_err());
        assert!(validate_username("a b").is_err());
    }

    #[test]
    fn port_rules() {
        assert_eq!(validate_port(0), Err(ValidationError::InvalidPort));
        assert!(validate_port(1).is_ok());
        assert!(validate_port(22).is_ok());
        assert!(validate_port(65535).is_ok());
    }
}
