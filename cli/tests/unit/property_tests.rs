//! Property-based tests for critical validation and generation logic.
//!
//! Uses `proptest` to verify invariants across many random inputs.

#![allow(clippy::expect_used)]

use proptest::prelude::*;

use polis_cli::commands::config::{validate_config_key, validate_config_value};
use polis_cli::commands::start::generate_workspace_id;

// ============================================================================
// generate_workspace_id() property tests
// ============================================================================

proptest! {
    /// Generated IDs always have correct format: polis- prefix + 16 hex chars.
    #[test]
    fn prop_workspace_id_has_valid_format(
        major in 0u32..100,
        minor in 0u32..100,
    ) {
        // Use inputs to vary the test run; the real invariant is on the generated ID.
        let _ = (major, minor);
        let id = generate_workspace_id();
        prop_assert!(id.starts_with("polis-"), "missing polis- prefix: {}", id);
        prop_assert!(id.len() == 22, "wrong length: {}", id);
        prop_assert!(id[6..].chars().all(|c| c.is_ascii_hexdigit()), "non-hex chars: {}", id);
    }
}

#[test]
fn test_workspace_id_uniqueness_batch() {
    // Generate 100 IDs and verify all are unique
    let ids: std::collections::HashSet<_> = (0..100).map(|_| generate_workspace_id()).collect();
    assert_eq!(ids.len(), 100, "duplicate IDs generated");
}

// ============================================================================
// validate_config_key() and validate_config_value() property tests
// ============================================================================

proptest! {
    /// Arbitrary keys (not in whitelist) are rejected.
    #[test]
    fn prop_arbitrary_keys_rejected(key in "[a-z]{1,20}\\.[a-z]{1,20}") {
        // Skip the one valid key
        if key != "security.level" {
            prop_assert!(validate_config_key(&key).is_err(), "accepted invalid key: {key}");
        }
    }

    /// Arbitrary values for security.level (not in whitelist) are rejected.
    #[test]
    fn prop_arbitrary_security_values_rejected(value in "[a-z]{1,20}") {
        if value != "balanced" && value != "strict" && value != "relaxed" {
            prop_assert!(
                validate_config_value("security.level", &value).is_err(),
                "accepted invalid value: {value}"
            );
        }
    }
}

#[test]
fn test_config_key_whitelist() {
    assert!(validate_config_key("security.level").is_ok());
    assert!(validate_config_key("unknown.key").is_err());
    assert!(validate_config_key("").is_err());
    assert!(validate_config_key("defaults.agent").is_err());
}

#[test]
fn test_config_value_whitelist() {
    assert!(validate_config_value("security.level", "balanced").is_ok());
    assert!(validate_config_value("security.level", "strict").is_ok());
    assert!(validate_config_value("security.level", "relaxed").is_ok());
    assert!(validate_config_value("security.level", "").is_err());
}
