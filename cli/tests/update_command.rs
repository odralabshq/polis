//! Integration tests for `polis update` (issue 16).
//!
//! RED phase: tests 2 and 3 fail until the engineer implements `update::run()`.
//! Test 1 passes today (the subcommand is already registered in cli.rs).
//!
//! Testability requirement: `run()` must accept `impl UpdateChecker` so the
//! unit tests in update.rs can inject a fake. The Senior Rust Engineer must
//! extract the `UpdateChecker` trait before the unit tests compile.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

#[test]
fn test_update_command_help_shows_description() {
    polis()
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Update Polis"));
}

#[test]
fn test_update_command_does_not_say_not_yet_implemented() {
    // Currently exits with "Command not yet implemented" — must be gone after implementation.
    polis()
        .arg("update")
        .assert()
        .stderr(predicate::str::contains("not yet implemented").not());
}

#[test]
fn test_update_command_network_failure_shows_actionable_error() {
    // In CI the GitHub API is unreachable or rate-limited.
    // The error must mention something meaningful (not a bare panic or placeholder).
    polis().arg("update").assert().failure().stderr(
        predicate::str::contains("update")
            .or(predicate::str::contains("network"))
            .or(predicate::str::contains("GitHub"))
            .or(predicate::str::contains("rate"))
            .or(predicate::str::contains("connect"))
            .or(predicate::str::contains("check")),
    );
}
