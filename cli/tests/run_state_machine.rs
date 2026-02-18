//! Integration tests for `polis run` state machine (issue 07).
//!
//! RED phase: all tests below define expected behavior that does not yet exist.
//! They will fail until the run command state machine is fully implemented.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn polis() -> Command {
    Command::cargo_bin("polis").expect("polis binary should exist")
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
// Fresh run — no existing state
// ---------------------------------------------------------------------------

#[test]
fn test_run_no_agents_installed_exits_with_error_and_hint() {
    // When no agents are installed and no agent arg is given,
    // the command must exit non-zero and print the install hint.
    polis()
        .arg("run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("polis agents add"));
}

#[test]
fn test_run_with_unknown_agent_exits_with_error_listing_available() {
    // Requesting an agent that is not installed must fail with a useful message.
    // Use a clean HOME with one known agent so the "not found" path is exercised.
    let dir = TempDir::new().expect("tempdir");
    let agents_dir = dir.path().join(".polis").join("agents").join("known-agent");
    std::fs::create_dir_all(agents_dir.join("agent.yaml").parent().expect("parent"))
        .expect("create agents dir");
    std::fs::write(agents_dir.join("agent.yaml"), b"name: known-agent").expect("write agent.yaml");
    polis()
        .args(["run", "nonexistent-agent-xyz"])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("nonexistent-agent-xyz").or(
            predicate::str::contains("not found").or(predicate::str::contains("not installed")),
        ));
}

// ---------------------------------------------------------------------------
// Resume — existing state, same agent
// ---------------------------------------------------------------------------

#[test]
fn test_run_with_existing_state_same_agent_resumes_from_checkpoint() {
    // When state.json exists with the same agent, run must print a resume message.
    // RED: currently prints "not yet implemented" — must print "Resuming" once wired.
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
    // RED: currently prints "not yet implemented" — must mention "claude-dev" once wired.
    let dir = state_dir_with("agent_ready", "claude-dev");
    polis()
        .args(["run", "gpt-dev"])
        .env("HOME", dir.path())
        .write_stdin("") // EOF → dialoguer treats as cancelled
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

    polis()
        .args(["run", "gpt-dev"])
        .env("HOME", dir.path())
        .write_stdin("n\n") // decline
        .assert()
        .failure(); // RED: not yet implemented

    // State file must be unchanged (or not exist if command errored before reading it)
    if state_path.exists() {
        let after = std::fs::read_to_string(&state_path).expect("read state after");
        assert_eq!(before, after, "state must not change when switch is declined");
    }
}

// ---------------------------------------------------------------------------
// State file format
// ---------------------------------------------------------------------------

#[test]
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
    assert!(v.get("workspace_id").is_some(), "state must have 'workspace_id' field");
    assert!(v.get("started_at").is_some(), "state must have 'started_at' field");
}

// ---------------------------------------------------------------------------
// Vocabulary constraint (NFR)
// ---------------------------------------------------------------------------

#[test]
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
// Property-based tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn polis() -> Command {
        Command::cargo_bin("polis").expect("polis binary should exist")
    }

    proptest! {
        /// Fresh run with any valid agent name always succeeds and creates state.json
        #[test]
        fn prop_fresh_run_any_agent_creates_state_file(agent in "[a-z][a-z0-9-]{1,20}") {
            let dir = TempDir::new().expect("tempdir");
            polis()
                .args(["run", &agent])
                .env("HOME", dir.path())
                .assert()
                .success();
            prop_assert!(
                dir.path().join(".polis").join("state.json").exists(),
                "state.json must exist after fresh run"
            );
        }

        /// State file after fresh run always contains agent_ready stage
        #[test]
        fn prop_fresh_run_state_file_ends_at_agent_ready(agent in "[a-z][a-z0-9-]{1,20}") {
            let dir = TempDir::new().expect("tempdir");
            polis()
                .args(["run", &agent])
                .env("HOME", dir.path())
                .assert()
                .success();
            let content = std::fs::read_to_string(
                dir.path().join(".polis").join("state.json")
            ).expect("read state");
            let v: serde_json::Value = serde_json::from_str(&content).expect("valid json");
            prop_assert_eq!(v["stage"].as_str(), Some("agent_ready"));
        }

        /// State file after fresh run always records the requested agent name
        #[test]
        fn prop_fresh_run_state_file_records_agent_name(agent in "[a-z][a-z0-9-]{1,20}") {
            let dir = TempDir::new().expect("tempdir");
            polis()
                .args(["run", &agent])
                .env("HOME", dir.path())
                .assert()
                .success();
            let content = std::fs::read_to_string(
                dir.path().join(".polis").join("state.json")
            ).expect("read state");
            let v: serde_json::Value = serde_json::from_str(&content).expect("valid json");
            prop_assert_eq!(v["agent"].as_str(), Some(agent.as_str()));
        }

        /// Resume run with same agent always succeeds and stdout contains "Resuming"
        #[test]
        fn prop_resume_run_prints_resuming(
            agent in "[a-z][a-z0-9-]{1,20}",
            stage in prop_oneof![
                Just("image_ready"),
                Just("workspace_created"),
                Just("credentials_set"),
                Just("provisioned"),
            ],
        ) {
            let dir = state_dir_with(stage, &agent);
            polis()
                .args(["run", &agent])
                .env("HOME", dir.path())
                .assert()
                .success()
                .stdout(predicate::str::contains("Resuming"));
        }

        /// Fresh run output never contains forbidden vocabulary
        #[test]
        fn prop_fresh_run_output_no_forbidden_vocabulary(agent in "[a-z][a-z0-9-]{1,20}") {
            let dir = TempDir::new().expect("tempdir");
            let out = polis()
                .args(["run", &agent])
                .env("HOME", dir.path())
                .output()
                .expect("run");
            let combined = format!(
                "{} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            ).to_lowercase();
            for forbidden in &["multipass", " docker", "container"] {
                prop_assert!(
                    !combined.contains(forbidden),
                    "output must not contain '{forbidden}'"
                );
            }
        }
    }
}
