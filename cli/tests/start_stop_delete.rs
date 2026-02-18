//! Integration tests for `polis start`, `polis stop`, and `polis delete [--all]` (issue 08).
//!
//! RED phase: all tests define expected behavior that does not yet exist.
//! They will fail until the commands are wired in `Cli::run()` and implemented.
//!
//! ⚠️  Testability note: tests that exercise the "workspace running/stopped" paths
//! (marked with NOTE) will remain RED even after wiring until a `WorkspaceDriver`
//! trait is introduced — see the testability recommendation at the bottom of this file.

#![allow(clippy::expect_used, deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn polis() -> Command {
    Command::cargo_bin("polis").expect("polis binary should exist")
}

/// Write a minimal valid state.json into `<dir>/.polis/state.json`.
fn write_state(dir: &TempDir, workspace_id: &str) {
    let polis_dir = dir.path().join(".polis");
    std::fs::create_dir_all(&polis_dir).expect("create .polis dir");
    let json = format!(
        r#"{{"stage":"agent_ready","agent":"claude-dev","workspace_id":"{workspace_id}","started_at":"2026-02-17T14:30:00Z"}}"#
    );
    std::fs::write(polis_dir.join("state.json"), json).expect("write state.json");
}

/// Write a minimal config.yaml into `<dir>/.polis/config.yaml`.
fn write_config(dir: &TempDir) {
    let polis_dir = dir.path().join(".polis");
    std::fs::create_dir_all(&polis_dir).expect("create .polis dir");
    std::fs::write(polis_dir.join("config.yaml"), b"security_level: balanced\n")
        .expect("write config.yaml");
}

fn state_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join(".polis").join("state.json")
}

fn config_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join(".polis").join("config.yaml")
}

// ============================================================================
// polis start
// ============================================================================

#[test]
fn test_start_no_workspace_exits_with_error_and_run_hint() {
    // WHEN no state.json exists THEN error with "polis run" hint.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .arg("start")
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("polis run"));
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_start_already_running_shows_info_and_exits_zero() {
    // WHEN workspace is already running THEN info message, exit 0, no error.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    polis()
        .arg("start")
        .env("HOME", dir.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("already running")
                .or(predicate::str::contains("already started")),
        );
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_start_stopped_workspace_exits_zero_and_hints_status() {
    // WHEN workspace is stopped THEN starts it, exits 0, hints "polis status".
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    polis()
        .arg("start")
        .env("HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("polis status"));
}

#[test]
fn test_start_output_contains_no_forbidden_vocabulary() {
    // User-facing output must never mention VM, container, docker, or multipass.
    let dir = TempDir::new().expect("tempdir");
    let out = polis()
        .arg("start")
        .env("HOME", dir.path())
        .output()
        .expect("command should run");
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    )
    .to_lowercase();
    for forbidden in &["multipass", " docker", "container", " vm "] {
        assert!(
            !combined.contains(forbidden),
            "start output must not contain '{forbidden}'"
        );
    }
}

// ============================================================================
// polis stop
// ============================================================================

#[test]
fn test_stop_no_workspace_exits_with_error() {
    // WHEN no state.json exists THEN error exit with a "no workspace" message.
    let dir = TempDir::new().expect("tempdir");
    polis()
        .arg("stop")
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("No workspace")
                .or(predicate::str::contains("no workspace"))
                .or(predicate::str::contains("not found")),
        );
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_stop_already_stopped_shows_info_and_exits_zero() {
    // WHEN workspace is already stopped THEN info message, exit 0.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    polis()
        .arg("stop")
        .env("HOME", dir.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("already stopped")
                .or(predicate::str::contains("not running")),
        );
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_stop_running_workspace_exits_zero_and_hints_start() {
    // WHEN workspace is running THEN stops it, exits 0, hints "polis start".
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    polis()
        .arg("stop")
        .env("HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("polis start"));
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_stop_running_workspace_output_states_data_is_preserved() {
    // WHEN workspace is stopped THEN output must reassure user data is preserved.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    polis()
        .arg("stop")
        .env("HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("preserved").or(predicate::str::contains("data")));
}

#[test]
fn test_stop_output_contains_no_forbidden_vocabulary() {
    let dir = TempDir::new().expect("tempdir");
    let out = polis()
        .arg("stop")
        .env("HOME", dir.path())
        .output()
        .expect("command should run");
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    )
    .to_lowercase();
    for forbidden in &["multipass", " docker", "container", " vm "] {
        assert!(
            !combined.contains(forbidden),
            "stop output must not contain '{forbidden}'"
        );
    }
}

// ============================================================================
// polis delete
// ============================================================================

#[test]
fn test_delete_declined_exits_zero_and_preserves_state_file() {
    // WHEN user declines confirmation THEN exit 0, state.json unchanged.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    let before = std::fs::read_to_string(state_path(&dir)).expect("read state before");

    polis()
        .arg("delete")
        .env("HOME", dir.path())
        .write_stdin("n\n")
        .assert()
        .success();

    assert!(
        state_path(&dir).exists(),
        "state.json must still exist after declined delete"
    );
    let after = std::fs::read_to_string(state_path(&dir)).expect("read state after");
    assert_eq!(before, after, "state.json must be unchanged after declined delete");
}

#[test]
fn test_delete_prompt_mentions_configuration_preserved() {
    // The delete prompt must tell the user that configuration is preserved.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    polis()
        .arg("delete")
        .env("HOME", dir.path())
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Configuration")
                .or(predicate::str::contains("configuration"))
                .or(predicate::str::contains("preserved")),
        );
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_delete_confirmed_removes_state_file() {
    // WHEN user confirms delete THEN state.json is removed.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");

    polis()
        .arg("delete")
        .env("HOME", dir.path())
        .write_stdin("y\n")
        .assert()
        .success();

    assert!(
        !state_path(&dir).exists(),
        "state.json must be removed after confirmed delete"
    );
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_delete_confirmed_preserves_config_yaml() {
    // WHEN user confirms delete THEN config.yaml must NOT be removed.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    write_config(&dir);

    polis()
        .arg("delete")
        .env("HOME", dir.path())
        .write_stdin("y\n")
        .assert()
        .success();

    assert!(
        config_path(&dir).exists(),
        "config.yaml must be preserved after delete"
    );
}

#[test]
fn test_delete_output_contains_no_forbidden_vocabulary() {
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    let out = polis()
        .arg("delete")
        .env("HOME", dir.path())
        .write_stdin("n\n")
        .output()
        .expect("command should run");
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    )
    .to_lowercase();
    for forbidden in &["multipass", " docker", "container", " vm "] {
        assert!(
            !combined.contains(forbidden),
            "delete output must not contain '{forbidden}'"
        );
    }
}

// ============================================================================
// polis delete --all
// ============================================================================

#[test]
fn test_delete_all_declined_exits_zero_and_preserves_state_file() {
    // WHEN user declines --all confirmation THEN exit 0, state.json unchanged.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    let before = std::fs::read_to_string(state_path(&dir)).expect("read state before");

    polis()
        .args(["delete", "--all"])
        .env("HOME", dir.path())
        .write_stdin("n\n")
        .assert()
        .success();

    assert!(
        state_path(&dir).exists(),
        "state.json must still exist after declined delete --all"
    );
    let after = std::fs::read_to_string(state_path(&dir)).expect("read state after");
    assert_eq!(before, after, "state.json must be unchanged after declined delete --all");
}

#[test]
fn test_delete_all_prompt_mentions_cached_images() {
    // The --all prompt must warn the user about cached image removal (~3.5 GB).
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    polis()
        .args(["delete", "--all"])
        .env("HOME", dir.path())
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("cached")
                .or(predicate::str::contains("images"))
                .or(predicate::str::contains("3.5")),
        );
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_delete_all_confirmed_removes_state_file() {
    // WHEN user confirms delete --all THEN state.json is removed.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");

    polis()
        .args(["delete", "--all"])
        .env("HOME", dir.path())
        .write_stdin("y\n")
        .assert()
        .success();

    assert!(
        !state_path(&dir).exists(),
        "state.json must be removed after confirmed delete --all"
    );
}

// NOTE: requires WorkspaceDriver trait to control workspace state in tests.
#[test]
fn test_delete_all_confirmed_preserves_config_yaml() {
    // WHEN user confirms delete --all THEN config.yaml must NOT be removed.
    let dir = TempDir::new().expect("tempdir");
    write_state(&dir, "ws-test01");
    write_config(&dir);

    polis()
        .args(["delete", "--all"])
        .env("HOME", dir.path())
        .write_stdin("y\n")
        .assert()
        .success();

    assert!(
        config_path(&dir).exists(),
        "config.yaml must be preserved after delete --all"
    );
}

// ============================================================================
// Property-based tests
// ============================================================================

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn polis() -> Command {
        Command::cargo_bin("polis").expect("polis binary should exist")
    }

    proptest! {
        /// start with no state file always fails regardless of HOME contents
        #[test]
        fn prop_start_no_state_always_fails(_seed in 0u32..100) {
            let dir = TempDir::new().expect("tempdir");
            polis()
                .arg("start")
                .env("HOME", dir.path())
                .assert()
                .failure();
        }

        /// stop with no state file always fails regardless of HOME contents
        #[test]
        fn prop_stop_no_state_always_fails(_seed in 0u32..100) {
            let dir = TempDir::new().expect("tempdir");
            polis()
                .arg("stop")
                .env("HOME", dir.path())
                .assert()
                .failure();
        }

        /// Any non-"y" input to delete always exits 0 and preserves state file
        #[test]
        fn prop_delete_non_confirm_preserves_state(
            input in "[^yY\n][^\n]*\n|[^\n]*\n"
        ) {
            let dir = TempDir::new().expect("tempdir");
            write_state(&dir, "ws-prop01");
            let before = std::fs::read_to_string(state_path(&dir)).expect("read before");

            polis()
                .arg("delete")
                .env("HOME", dir.path())
                .write_stdin(input.as_bytes())
                .assert()
                .success();

            prop_assert!(state_path(&dir).exists(), "state.json must survive declined delete");
            let after = std::fs::read_to_string(state_path(&dir)).expect("read after");
            prop_assert_eq!(before, after);
        }

        /// Any non-"y" input to delete --all always exits 0 and preserves state file
        #[test]
        fn prop_delete_all_non_confirm_preserves_state(
            input in "[^yY\n][^\n]*\n|[^\n]*\n"
        ) {
            let dir = TempDir::new().expect("tempdir");
            write_state(&dir, "ws-prop02");
            let before = std::fs::read_to_string(state_path(&dir)).expect("read before");

            polis()
                .args(["delete", "--all"])
                .env("HOME", dir.path())
                .write_stdin(input.as_bytes())
                .assert()
                .success();

            prop_assert!(state_path(&dir).exists(), "state.json must survive declined delete --all");
            let after = std::fs::read_to_string(state_path(&dir)).expect("read after");
            prop_assert_eq!(before, after);
        }

        /// delete confirmed with any workspace_id removes state file
        #[test]
        fn prop_delete_confirmed_removes_state(ws_id in "[a-z]{2}-[a-z0-9]{4,8}") {
            let dir = TempDir::new().expect("tempdir");
            write_state(&dir, &ws_id);

            polis()
                .arg("delete")
                .env("HOME", dir.path())
                .write_stdin("y\n")
                .assert()
                .success();

            prop_assert!(!state_path(&dir).exists(), "state.json must be removed after confirmed delete");
        }

        /// delete --all confirmed with any workspace_id preserves config.yaml
        #[test]
        fn prop_delete_all_confirmed_preserves_config(ws_id in "[a-z]{2}-[a-z0-9]{4,8}") {
            let dir = TempDir::new().expect("tempdir");
            write_state(&dir, &ws_id);
            write_config(&dir);

            polis()
                .args(["delete", "--all"])
                .env("HOME", dir.path())
                .write_stdin("y\n")
                .assert()
                .success();

            prop_assert!(config_path(&dir).exists(), "config.yaml must survive delete --all");
        }

        /// start/stop/delete output never contains forbidden vocabulary
        #[test]
        fn prop_lifecycle_output_no_forbidden_vocabulary(
            cmd in prop_oneof![Just("start"), Just("stop"), Just("delete")],
        ) {
            let dir = TempDir::new().expect("tempdir");
            let out = polis()
                .arg(cmd)
                .env("HOME", dir.path())
                .write_stdin("n\n")
                .output()
                .expect("command should run");
            let combined = format!(
                "{} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            ).to_lowercase();
            for forbidden in &["multipass", " docker", "container", " vm "] {
                prop_assert!(
                    !combined.contains(forbidden),
                    "{cmd} output must not contain '{forbidden}'"
                );
            }
        }
    }
}
