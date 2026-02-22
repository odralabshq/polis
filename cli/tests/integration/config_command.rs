//! Integration tests for `polis config` command.
//!
//! All filesystem-touching tests set `POLIS_CONFIG` to a temp path so they
//! never read or write `~/.polis/config.yaml`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

/// Returns a `TempDir` and the path string for a config file inside it.
fn temp_config_path() -> (TempDir, String) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir
        .path()
        .join("config.yaml")
        .to_string_lossy()
        .into_owned();
    (dir, path)
}

// ---------------------------------------------------------------------------
// Subcommand registration
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
// `polis config show`
// ---------------------------------------------------------------------------

#[test]
fn test_config_show_no_config_file_uses_balanced_default() {
    let (_dir, path) = temp_config_path();
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
fn test_config_show_displays_polis_config_env_var_label() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "show"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success()
        .stdout(predicate::str::contains("POLIS_CONFIG"));
}

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
// `polis config set` happy paths
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
// Validation errors
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_unknown_key_returns_error_with_valid_keys() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "unknown.key", "value"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("security.level"));
}

#[test]
fn test_config_set_invalid_value_returns_error_with_valid_values() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "security.level", "permissive"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("balanced").or(predicate::str::contains("strict")));
}

// ---------------------------------------------------------------------------
// POLIS_CONFIG env var
// ---------------------------------------------------------------------------

#[test]
fn test_config_set_uses_polis_config_env_var() {
    let (_dir, path) = temp_config_path();
    polis()
        .args(["config", "set", "security.level", "strict"])
        .env("POLIS_CONFIG", &path)
        .assert()
        .success();
    assert!(
        std::path::Path::new(&path).exists(),
        "config file should be created at POLIS_CONFIG path"
    );
}

// ---------------------------------------------------------------------------
// Round-trip: set then show
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
// File permissions
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
    let mode = std::fs::metadata(&path)
        .expect("file should exist")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "expected 0o600, got {mode:o}");
}

// ---------------------------------------------------------------------------
// Corrupt YAML handling
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
