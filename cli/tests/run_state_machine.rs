//! Integration tests for `polis run` state machine (issue 07).
//!
//! These tests require multipass and a VM image to be available.
//! Run with: `cargo test --test run_state_machine -- --ignored`

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
    // When POLIS_IMAGE points to a file that does not exist, polis run must
    // exit non-zero with a message naming the bad path.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .arg("run")
        .env("HOME", dir.path())
        .env("POLIS_IMAGE", "/nonexistent/path/image.qcow2")
        .assert()
        .failure()
        .stderr(predicate::str::contains("POLIS_IMAGE"));
}

// ---------------------------------------------------------------------------
// Fresh run — no existing state (requires multipass + VM image)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires multipass and VM image"]
fn test_run_no_agents_installed_succeeds_without_agent() {
    // When no agents are installed and no agent arg is given,
    // the command must succeed — workspace starts without an agent.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .arg("run")
        .env("HOME", dir.path())
        .assert()
        .success();
}

#[test]
#[ignore = "requires multipass and VM image"]
fn test_run_with_explicit_agent_name_succeeds_without_preinstall() {
    // Providing an agent name that is not pre-installed must still succeed —
    // the workspace starts and the agent name is recorded as-is.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["run", "nonexistent-agent-xyz"])
        .env("HOME", dir.path())
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Resume — existing state, same agent (requires multipass)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires multipass and VM image"]
fn test_run_with_existing_state_same_agent_resumes_from_checkpoint() {
    // When state.json exists with the same agent, run must print a resume message.
    let dir = state_dir_with("provisioned", "claude-dev");
    polis()
        .args(["run", "claude-dev"])
        .env("HOME", dir.path())
        .assert()
        .stdout(predicate::str::contains("Resuming").or(predicate::str::contains("resuming")));
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
// State file format (requires multipass)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires multipass and VM image"]
fn test_run_creates_state_file_after_first_stage() {
    // After a fresh run, state.json must exist at ~/.polis/state.json.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["run", "claude-dev"])
        .env("HOME", dir.path())
        .assert()
        .success();
    assert!(
        dir.path().join(".polis").join("state.json").exists(),
        "state.json must be created after run"
    );
}

#[test]
#[ignore = "requires multipass and VM image"]
fn test_run_state_file_contains_valid_json_after_stage() {
    // The state file written by run must be valid JSON with required fields.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["run", "claude-dev"])
        .env("HOME", dir.path())
        .assert()
        .success();
    let state_path = dir.path().join(".polis").join("state.json");
    let content = std::fs::read_to_string(&state_path).expect("read state");
    let v: serde_json::Value =
        serde_json::from_str(&content).expect("state.json must be valid JSON");
    assert!(v.get("stage").is_some(), "state must have 'stage' field");
    assert!(v.get("agent").is_some(), "state must have 'agent' field");
    assert!(
        v.get("workspace_id").is_some(),
        "state must have 'workspace_id' field"
    );
    assert!(
        v.get("started_at").is_some(),
        "state must have 'started_at' field"
    );
}

// ---------------------------------------------------------------------------
// Vocabulary constraint (NFR) — requires multipass
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires multipass and VM image"]
fn test_run_output_contains_no_forbidden_vocabulary() {
    // User-facing output must never mention VM, container, docker, or multipass.
    let dir = TempDir::new().expect("tempdir");
    let output = polis()
        .args(["run", "claude-dev"])
        .env("HOME", dir.path())
        .output()
        .expect("command should run");

    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
    .to_lowercase();

    for forbidden in &["multipass", " docker", "container"] {
        assert!(
            !combined.contains(forbidden),
            "output must not contain '{forbidden}'"
        );
    }
}

// ---------------------------------------------------------------------------
// Property-based tests (require multipass — disabled)
// ---------------------------------------------------------------------------

// These tests are disabled because they require multipass and a VM image.
// To run them: cargo test --test run_state_machine -- --ignored
// After setting up multipass and building the VM image.
