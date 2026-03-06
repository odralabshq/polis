//! Security domain types and validation.
//!
//! Re-exports `SecurityLevel` from polis-common for single source of truth.
//! Defines `AllowAction` for domain rule operations and request ID validation.

pub use polis_common::types::SecurityLevel;

use clap::ValueEnum;
use std::sync::LazyLock;
use regex::Regex;
use anyhow::Result;

/// Action to take for domain rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum AllowAction {
    /// Auto-approve matching requests
    #[default]
    Allow,
    /// Prompt user for matching requests
    Prompt,
    /// Block matching requests
    Block,
}

impl std::fmt::Display for AllowAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allow => write!(f, "allow"),
            Self::Prompt => write!(f, "prompt"),
            Self::Block => write!(f, "block"),
        }
    }
}

static REQUEST_ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| {
        Regex::new(r"^req-[a-f0-9]{8}$")
            .unwrap_or_else(|e| panic!("REQUEST_ID_REGEX is invalid: {e}"))
    });

/// Validates a request ID matches the expected format.
///
/// # Errors
/// Returns an error if the ID does not match pattern `req-[a-f0-9]{8}`.
pub fn validate_request_id(id: &str) -> Result<()> {
    if !REQUEST_ID_REGEX.is_match(id) {
        anyhow::bail!(
            "Invalid request ID '{id}': expected format req-[a-f0-9]{{8}}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_request_ids_accepted() {
        assert!(validate_request_id("req-12345678").is_ok());
        assert!(validate_request_id("req-abcdef01").is_ok());
        assert!(validate_request_id("req-00000000").is_ok());
        assert!(validate_request_id("req-ffffffff").is_ok());
    }

    #[test]
    fn invalid_request_id_too_short() {
        let err = validate_request_id("req-1234567").expect_err("should reject too-short ID");
        assert!(err.to_string().contains("Invalid request ID"));
    }

    #[test]
    fn invalid_request_id_too_long() {
        let err = validate_request_id("req-123456789").expect_err("should reject too-long ID");
        assert!(err.to_string().contains("Invalid request ID"));
    }

    #[test]
    fn invalid_request_id_uppercase() {
        let err = validate_request_id("req-ABCDEF01").expect_err("should reject uppercase ID");
        assert!(err.to_string().contains("Invalid request ID"));
    }

    #[test]
    fn invalid_request_id_empty() {
        let err = validate_request_id("").expect_err("should reject empty string");
        assert!(err.to_string().contains("Invalid request ID"));
    }

    #[test]
    fn invalid_request_id_wrong_prefix() {
        let err = validate_request_id("id-12345678").expect_err("should reject wrong prefix");
        assert!(err.to_string().contains("Invalid request ID"));
    }

    #[test]
    fn allow_action_display_lowercase() {
        assert_eq!(AllowAction::Allow.to_string(), "allow");
        assert_eq!(AllowAction::Prompt.to_string(), "prompt");
        assert_eq!(AllowAction::Block.to_string(), "block");
    }
}
