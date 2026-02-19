//! Integration tests for `polis _extract-host-key` (issue 12).
//!
//! RED phase: all tests define expected behavior that does not yet exist.
//! They will fail until `extract_host_key()` is implemented and wired in `Cli::run()`.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// `_extract-host-key` must be wired in `Cli::run()` — not fall through to the
/// "not yet implemented" catch-all.
#[test]
fn test_extract_host_key_does_not_say_not_yet_implemented() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    polis()
        .arg("_extract-host-key")
        .env("HOME", dir.path())
        .assert()
        .stderr(predicate::str::contains("not yet implemented").not());
}

/// `_extract-host-key --help` must be accessible (hidden command, but help still works).
#[test]
fn test_extract_host_key_help_is_accessible() {
    polis()
        .args(["_extract-host-key", "--help"])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Backend detection / error handling
// ---------------------------------------------------------------------------

/// When neither multipass nor docker is reachable, `_extract-host-key` must
/// exit non-zero with a human-readable error — not a panic, not "not yet implemented".
#[test]
fn test_extract_host_key_no_backend_exits_with_error() {
    let empty = tempfile::TempDir::new().expect("tempdir");
    polis()
        .arg("_extract-host-key")
        .env("PATH", empty.path())
        .env("HOME", empty.path())
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not())
        .stderr(predicate::str::contains("panic").not())
        .stderr(predicate::str::contains("not yet implemented").not());
}

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

/// When a backend is available, output must be a single `known_hosts` line
/// starting with "workspace ssh-ed25519 ".
///
/// This test is skipped in CI (no backend) via the `POLIS_BACKEND_AVAILABLE`
/// env gate — it will be RED until the command is implemented.
#[test]
fn test_extract_host_key_output_format_is_known_hosts_line() {
    if std::env::var("POLIS_BACKEND_AVAILABLE").is_err() {
        // No backend in this environment — test the format contract only when
        // a real backend is present.  Still counts as RED until implemented.
        return;
    }
    let dir = tempfile::TempDir::new().expect("tempdir");
    polis()
        .arg("_extract-host-key")
        .env("HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::starts_with("workspace ssh-ed25519 "));
}
