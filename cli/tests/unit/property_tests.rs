//! Property-based tests for critical validation and generation logic.
//!
//! Uses `proptest` to verify invariants across many random inputs.

#![allow(clippy::expect_used)]

use proptest::prelude::*;

use polis_cli::commands::config::{validate_config_key, validate_config_value};
use polis_cli::commands::start::generate_workspace_id;
use polis_cli::workspace::vm::generate_certs_and_secrets;

use crate::mocks::MultipassExecRecorder;

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

// ============================================================================
// generate_certs_and_secrets() property tests
// ============================================================================

proptest! {
    /// **Validates: Requirements 3.2, 3.3**
    ///
    /// Property: `generate_certs_and_secrets()` always calls all 5 scripts in
    /// the correct order, regardless of any pre-existing cert file state.
    ///
    /// The Rust function is unconditional — idempotency is handled inside the
    /// scripts themselves. The proptest inputs simulate arbitrary combinations
    /// of pre-existing cert files (present/absent) but the function's behaviour
    /// must be invariant across all of them.
    #[test]
    fn prop_generate_certs_always_calls_5_scripts_in_order(
        // Simulate arbitrary combinations of pre-existing cert files
        // (the Rust function doesn't check these — scripts do)
        _ca_exists in proptest::bool::ANY,
        _valkey_certs_exist in proptest::bool::ANY,
        _valkey_secrets_exist in proptest::bool::ANY,
        _toolbox_certs_exist in proptest::bool::ANY,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let mp = MultipassExecRecorder::new();
            generate_certs_and_secrets(&mp).await.expect("should succeed");
            let calls = mp.recorded_calls();

            // Always exactly 6 calls (5 scripts + 1 logger)
            prop_assert_eq!(calls.len(), 6);

            // Script calls must be in correct order
            let script_names = [
                "generate-ca.sh",
                "generate-certs.sh",
                "generate-secrets.sh",
                "generate-certs.sh",  // toolbox
                "fix-cert-ownership.sh",
            ];
            for (i, name) in script_names.iter().enumerate() {
                let cmd = calls[i].get(3).map_or("", String::as_str);
                prop_assert!(cmd.contains(name), "call {} should contain {}, got: {}", i, name, cmd);
            }

            Ok(())
        })?;
    }

    /// **Validates: Requirements 3.2, 3.3**
    ///
    /// Property: all script paths are rooted at `/opt/polis/` and use
    /// `sudo bash -c` as the invocation pattern.
    #[test]
    fn prop_generate_certs_all_script_paths_rooted_at_opt_polis(
        _any_bool in proptest::bool::ANY,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let mp = MultipassExecRecorder::new();
            generate_certs_and_secrets(&mp).await.expect("should succeed");
            let calls = mp.recorded_calls();

            // All 5 script calls must use sudo bash -c and paths rooted at /opt/polis/
            for (i, call) in calls.iter().take(5).enumerate() {
                prop_assert_eq!(call.first().map(String::as_str), Some("sudo"),
                    "call {} should start with sudo", i);
                prop_assert_eq!(call.get(1).map(String::as_str), Some("bash"),
                    "call {} second arg should be bash", i);
                prop_assert_eq!(call.get(2).map(String::as_str), Some("-c"),
                    "call {} third arg should be -c", i);
                let cmd = call.get(3).map_or("", String::as_str);
                prop_assert!(cmd.starts_with("/opt/polis/"),
                    "call {} path should start with /opt/polis/, got: {}", i, cmd);
            }

            Ok(())
        })?;
    }
}
