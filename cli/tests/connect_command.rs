//! Integration tests for `polis connect` (issue 13).
//!
//! RED phase: all tests define expected behavior that does not yet exist.
//! They will fail until `SshConfigManager`, `connect::run()`, and
//! `resolve_ide()` are implemented and wired in `Cli::run()`.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

#[test]
fn test_connect_help_is_accessible() {
    polis().args(["connect", "--help"]).assert().success();
}

#[test]
fn test_connect_does_not_say_not_yet_implemented() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // Pipe empty stdin so dialoguer doesn't block waiting for a TTY.
    polis()
        .arg("connect")
        .env("HOME", dir.path())
        .write_stdin(b"n\n" as &[u8])
        .assert()
        .stderr(predicate::str::contains("not yet implemented").not());
}

// ---------------------------------------------------------------------------
// IDE flag — error paths
// ---------------------------------------------------------------------------

#[test]
fn test_connect_unknown_ide_exits_with_error_listing_supported_ides() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    polis()
        .args(["connect", "--ide", "emacs"])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("emacs").or(predicate::str::contains("Unknown IDE")))
        .stderr(
            predicate::str::contains("vscode")
                .or(predicate::str::contains("cursor"))
                .or(predicate::str::contains("Supported")),
        );
}

#[test]
fn test_connect_ide_not_installed_exits_with_error() {
    // Force PATH to an empty dir so neither `code` nor `cursor` is found.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let empty_path = tempfile::TempDir::new().expect("tempdir");
    polis()
        .args(["connect", "--ide", "vscode"])
        .env("HOME", dir.path())
        .env("PATH", empty_path.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("panic").not())
        .stderr(predicate::str::is_empty().not());
}
