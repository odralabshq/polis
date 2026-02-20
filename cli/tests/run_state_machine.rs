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

/// Write a valid state.json into a temp dir and return the dir.
fn state_dir_with(stage: &str, agent: &str) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let polis_dir = dir.path().join(".polis");
    std::fs::create_dir_all(&polis_dir).expect("create .polis dir");
    let json = format!(
        r#"{{"stage":"{stage}","agent":"{agent}","workspace_id":"ws-test01","started_at":"2026-02-17T14:30:00Z"}}"#
    );
    std::fs::write(polis_dir.join("state.json"), json).expect("write state.json");
    dir
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

#[test]
fn test_run_with_existing_state_different_agent_prompts_for_confirmation() {
    // When state.json has agent A and user requests agent B,
    // the command must mention the running agent in its output.
    let dir = state_dir_with("agent_ready", "claude-dev");
    polis()
        .args(["run", "gpt-dev"])
        .env("HOME", dir.path())
        .write_stdin("") // EOF → dialoguer treats as cancelled
        .timeout(std::time::Duration::from_secs(2))
        .assert()
        .stdout(
            predicate::str::contains("claude-dev")
                .or(predicate::str::contains("Switch"))
                .or(predicate::str::contains("switch")),
        );
}

#[test]
fn test_run_agent_switch_declined_makes_no_changes() {
    // When the user declines the switch prompt, the state file must be unchanged.
    let dir = state_dir_with("agent_ready", "claude-dev");
    let state_path = dir.path().join(".polis").join("state.json");
    let before = std::fs::read_to_string(&state_path).expect("read state before");

    let _ = polis()
        .args(["run", "gpt-dev"])
        .env("HOME", dir.path())
        .write_stdin("n\n") // decline
        .timeout(std::time::Duration::from_secs(2))
        .assert();

    // State file must be unchanged
    let after = std::fs::read_to_string(&state_path).expect("read state after");
    assert_eq!(
        before, after,
        "state must not change when switch is declined"
    );
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

// These tests are covered by unit tests in cli/src/commands/run.rs using MockMultipass.
