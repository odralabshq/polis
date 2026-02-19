//! Integration tests for `polis _ssh-proxy` (issue 11).
//!
//! RED phase: all tests define expected behavior that does not yet exist.
//! They will fail until `ssh_proxy()` is implemented and wired in `Cli::run()`.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// `_ssh-proxy` must be wired in `Cli::run()` — not fall through to the
/// "not yet implemented" catch-all.
#[test]
fn test_ssh_proxy_does_not_say_not_yet_implemented() {
    polis()
        .arg("_ssh-proxy")
        .assert()
        .stderr(predicate::str::contains("not yet implemented").not());
}

// ---------------------------------------------------------------------------
// Backend detection
// ---------------------------------------------------------------------------

/// When neither multipass nor docker is reachable, `_ssh-proxy` must exit
/// non-zero with a human-readable error — not a panic or empty stderr.
#[test]
fn test_ssh_proxy_no_backend_exits_with_error() {
    // Force PATH to an empty dir so multipass and docker are not found.
    let empty = tempfile::TempDir::new().expect("tempdir");
    polis()
        .arg("_ssh-proxy")
        .env("PATH", empty.path())
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not());
}

// ---------------------------------------------------------------------------
// STDIO bridging
// ---------------------------------------------------------------------------

/// When stdin closes immediately (SSH client disconnects before handshake),
/// `_ssh-proxy` must exit cleanly — no panic, no "unwrap" in stderr.
#[test]
fn test_ssh_proxy_stdin_eof_exits_without_panic() {
    let empty = tempfile::TempDir::new().expect("tempdir");
    polis()
        .arg("_ssh-proxy")
        .env("PATH", empty.path())
        .write_stdin(b"" as &[u8])
        .assert()
        .failure()
        .stderr(predicate::str::contains("panic").not())
        .stderr(predicate::str::contains("unwrap").not());
}
