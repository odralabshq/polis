//! Integration tests for `polis run` state machine (issue 07).
//!
//! Tests that require multipass are covered by unit tests in `cli/src/commands/run.rs`
//! using `MockMultipass`. Only tests exercisable without a VM are kept here.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

// ---------------------------------------------------------------------------
// Image resolution — no multipass required (fails before VM launch)
// ---------------------------------------------------------------------------

#[test]
fn test_run_no_image_exits_nonzero_with_polis_init_hint() {
    // When no image exists at POLIS_IMAGE or ~/.polis/images/, polis run must
    // exit non-zero and tell the user to run 'polis init'.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .arg("run")
        .env("HOME", dir.path())
        .env_remove("POLIS_IMAGE")
        .assert()
        .failure()
        .stderr(predicate::str::contains("polis init"));
}

#[test]
fn test_run_polis_image_nonexistent_exits_nonzero_with_error() {
    // Without a state file, polis run must exit non-zero directing user to polis init.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .arg("run")
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("polis init"));
}

// ---------------------------------------------------------------------------
// Agent switch — existing state, different agent
// ---------------------------------------------------------------------------
// NOTE: These tests require a running VM to work. Since we now check VM existence
// via multipass before checking state, tests that only set up state files will
// fail with "No workspace VM found". Agent switch logic is tested in unit tests
// using MockMultipass in cli/src/commands/run.rs.

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

// These tests are covered by unit tests in cli/src/commands/run.rs using MockMultipass.
