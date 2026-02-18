//! Integration tests for `polis config` (issue 19).
//!
//! RED phase: tests 2–15 fail until the engineer implements `config::run()`,
//! `PolisConfig`, and wires `Command::Config` in `cli.rs`.
//! Test 1 passes today (clap already registers the subcommand).
//!
//! All filesystem-touching tests set `POLIS_CONFIG` to a temp path so they
//! never read or write `~/.polis/config.yaml`.

#![allow(clippy::expect_used, clippy::unwrap_used, deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn polis() -> Command {
    Command::cargo_bin("polis").expect("polis binary should exist")
}

/// Returns a `TempDir` and the path string for a config file inside it.
/// The file does NOT exist yet — callers that need an empty baseline use this.
fn temp_config_path() -> (TempDir, String) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("config.yaml").to_string_lossy().into_owned();
    (dir, path)
}

// ---------------------------------------------------------------------------
// 1. Subcommand registration (PASSES today — clap handles --help)
// ---------------------------------------------------------------------------

#[test]
fn test_config_help_shows_show_and_set_subcommands() {
    polis()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("set"));
}

// ---------------------------------------------------------------------------
// 2–6. `polis config show` (RED until run() is wired)
// ---------------------------------------------------------------------------

#[test]
fn test_config_show_does_not_say_not_yet_implemented() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .stderr(predicate::str::contains("not yet implemented").not());
}

#[test]
fn test_config_show_no_config_file_uses_balanced_default() {
    let (_dir, path) = temp_config_path();
    // path does not exist → must fall back to default "balanced"
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("balanced"));
}

#[test]
fn test_config_show_displays_security_level_key() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("security.level"));
}

#[test]
fn test_config_show_displays_defaults_agent_key() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("defaults.agent"));
}

#[test]
fn test_config_show_displays_polis_config_env_var_label() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("POLIS_CONFIG"));
}

// ---------------------------------------------------------------------------
// 7–8. `polis config set` happy paths (RED)
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_security_level_balanced_succeeds() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "security.level", "balanced"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("security.level"));
}

#[test]
fn test_config_set_security_level_strict_succeeds() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "security.level", "strict"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("security.level"));
}

// ---------------------------------------------------------------------------
// 9. V-003: `relaxed` is banned (RED)
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_security_level_relaxed_returns_error() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "security.level", "relaxed"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// 10–11. `defaults.agent` (RED)
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_defaults_agent_sets_value() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "defaults.agent", "claude-dev"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("defaults.agent"));
}

#[test]
fn test_config_set_defaults_agent_null_unsets() {
    let (_dir, path) = temp_config_path();
    // First set a value, then unset with "null"
    polis()
        .args(["config", "set", "defaults.agent", "claude-dev"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();

    polis()
        .args(["config", "set", "defaults.agent", "null"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();

    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("(not set)"));
}

// ---------------------------------------------------------------------------
// 12–13. Validation errors (RED)
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_unknown_key_returns_error_with_valid_keys() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "unknown.key", "value"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .failure()
        // Error must list at least one valid key so the user knows what to use
        .stderr(
            predicate::str::contains("security.level")
                .or(predicate::str::contains("defaults.agent")),
        );
}

#[test]
fn test_config_set_invalid_value_returns_error_with_valid_values() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "security.level", "permissive"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .failure()
        // Error must list the valid values
        .stderr(
            predicate::str::contains("balanced").or(predicate::str::contains("strict")),
        );
}

// ---------------------------------------------------------------------------
// 14. POLIS_CONFIG env var (RED)
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_uses_polis_config_env_var() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "security.level", "strict"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();

    // The file must now exist at the custom path
    assert!(
        std::path::Path::new(&path).exists(),
        "config file should be created at POLIS_CONFIG path"
    );
}

// ---------------------------------------------------------------------------
// 15. Round-trip: set then show (RED)
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_persists_value_readable_by_show() {
    let (_dir, path) = temp_config_path();

    polis()
        .args(["config", "set", "security.level", "strict"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();

    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("strict"));
}

// ---------------------------------------------------------------------------
// NFR: show must NOT create the config file (spec §5 negative constraints)
// ---------------------------------------------------------------------------

#[test]
fn test_config_show_does_not_create_file() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();
    assert!(
        !std::path::Path::new(&path).exists(),
        "show must not create the config file"
    );
}

// ---------------------------------------------------------------------------
// NFR: config file must have 0o600 permissions after set (spec §5 security)
// ---------------------------------------------------------------------------

#[test]
#[cfg(unix)]
fn test_config_set_creates_file_with_0o600_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "security.level", "strict"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();
    let mode = std::fs::metadata(&path).expect("file should exist").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "expected 0o600, got {mode:o}");
}

// ---------------------------------------------------------------------------
// NFR: corrupt YAML must return an error (spec §3 error table)
// ---------------------------------------------------------------------------

#[test]
fn test_config_show_corrupt_yaml_returns_error() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, b"{ not: valid: yaml: [[[").expect("write");
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", path.to_str().expect("path"))
        .assert()
        .failure();
}

#[test]
fn test_config_set_corrupt_yaml_returns_error() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, b"{ not: valid: yaml: [[[").expect("write");
    polis()
        .args(["config", "set", "security.level", "strict"])
        .env("POLIS_CONFIG", path.to_str().expect("path"))
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Orthogonality: setting one key does not clobber the other
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_security_level_preserves_defaults_agent() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "defaults.agent", "my-agent"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();
    polis()
        .args(["config", "set", "security.level", "strict"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("my-agent"))
        .stdout(predicate::str::contains("strict"));
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod config_proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Any printable non-empty string is a valid defaults.agent value.
        #[test]
        fn prop_defaults_agent_accepts_any_printable_string(
            agent in "[a-zA-Z][a-zA-Z0-9._-]{0,49}"
        ) {
            let (_dir, path) = temp_config_path();
            polis()
                .args(["config", "set", "defaults.agent", &agent])
                .env("POLIS_CONFIG", &path)
                .assert()
                .success();
        }

        /// Only "balanced" and "strict" are valid security levels.
        #[test]
        fn prop_security_level_rejects_non_balanced_strict(
            level in "[a-z]{3,15}"
        ) {
            prop_assume!(level != "balanced" && level != "strict");
            let (_dir, path) = temp_config_path();
            polis()
                .args(["config", "set", "security.level", &level])
                .env("POLIS_CONFIG", &path)
                .assert()
                .failure()
                .stderr(predicate::str::contains("balanced").or(predicate::str::contains("strict")));
        }

        /// set then show always reflects the last written value.
        #[test]
        fn prop_set_then_show_reflects_value(
            level in prop::sample::select(vec!["balanced", "strict"])
        ) {
            let (_dir, path) = temp_config_path();
            polis()
                .args(["config", "set", "security.level", level])
                .env("POLIS_CONFIG", &path)
                .assert()
                .success();
            polis()
                .args(["config", "show"])
                .env("POLIS_CONFIG", &path)
                .assert()
                .success()
                .stdout(predicate::str::contains(level));
        }
    }
}
