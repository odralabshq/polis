//! Integration tests for `polis status` command (issue 06).
//!
//! Tests workspace detection, security status, and output formatting.
//!
//! Spec: docs/linear-issues/polis-oss/ux-improvements/06-status-command.md

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

// ── Human-readable output ─────────────────────────────────────────────────────

/// EARS: WHEN `polis status` is run THEN it exits successfully.
#[test]
fn test_status_exits_successfully() {
    polis().arg("status").assert().success();
}

/// EARS: WHEN `polis status` is run THEN output contains workspace status.
#[test]
fn test_status_shows_workspace_line() {
    polis()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Workspace:"));
}

/// EARS: WHEN `polis status` is run THEN output contains security section.
#[test]
fn test_status_shows_security_section() {
    polis()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Security:"));
}

/// EARS: WHEN `polis status` is run THEN output shows traffic inspection status.
#[test]
fn test_status_shows_traffic_inspection() {
    polis()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Traffic inspection"));
}

/// EARS: WHEN `polis status` is run THEN output shows credential protection status.
#[test]
fn test_status_shows_credential_protection() {
    polis()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Credential protection"));
}

/// EARS: WHEN `polis status` is run THEN output shows malware scanning status.
#[test]
fn test_status_shows_malware_scanning() {
    polis()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Malware scanning"));
}

// ── Vocabulary constraints ────────────────────────────────────────────────────

/// EARS: WHEN `polis status` is run THEN output does NOT contain internal terms.
#[test]
fn test_status_no_internal_vocabulary() {
    let output = polis()
        .arg("status")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    let forbidden = ["docker", "container", "multipass", "qemu", "libvirt", "VM"];

    for term in &forbidden {
        assert!(
            !text.to_lowercase().contains(&term.to_lowercase()),
            "output must not contain '{term}'"
        );
    }
}

// ── JSON output ───────────────────────────────────────────────────────────────

/// EARS: WHEN `polis status --json` is run THEN stdout contains no ANSI codes.
#[test]
fn test_status_json_no_ansi_in_output() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    assert!(
        !text.contains("\x1b["),
        "JSON output must not contain ANSI escape codes"
    );
}

/// EARS: WHEN `polis status --json` is run THEN workspace.status is a valid state.
#[test]
fn test_status_json_workspace_status_valid() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    let status = v["workspace"]["status"].as_str().expect("status is string");
    let valid_states = ["running", "stopped", "starting", "stopping", "error"];
    assert!(
        valid_states.contains(&status),
        "workspace.status '{status}' must be one of {valid_states:?}"
    );
}

/// EARS: WHEN `polis status --json` is run THEN events.severity is valid.
#[test]
fn test_status_json_events_severity_valid() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    let severity = v["events"]["severity"]
        .as_str()
        .expect("severity is string");
    let valid_severities = ["none", "info", "warning", "error"];
    assert!(
        valid_severities.contains(&severity),
        "events.severity '{severity}' must be one of {valid_severities:?}"
    );
}

/// EARS: WHEN `polis status --json` is run THEN events.count is a non-negative integer.
#[test]
fn test_status_json_events_count_non_negative() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    let count = v["events"]["count"].as_u64().expect("count is u64");
    assert!(
        count < u64::from(u32::MAX),
        "events.count should be reasonable"
    );
}

// ── Quiet mode ────────────────────────────────────────────────────────────────

/// EARS: WHEN `polis status --quiet` is run THEN it exits successfully with minimal output.
#[test]
fn test_status_quiet_exits_successfully() {
    polis().args(["status", "--quiet"]).assert().success();
}

// ── No-color mode ─────────────────────────────────────────────────────────────

/// EARS: WHEN `polis status --no-color` is run THEN output has no ANSI codes.
#[test]
fn test_status_no_color_no_ansi() {
    let output = polis()
        .args(["status", "--no-color"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    assert!(
        !text.contains("\x1b["),
        "--no-color output must not contain ANSI escape codes"
    );
}

/// EARS: WHEN `NO_COLOR` env is set THEN output has no ANSI codes.
#[test]
fn test_status_no_color_env_no_ansi() {
    let output = polis()
        .arg("status")
        .env("NO_COLOR", "1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    assert!(
        !text.contains("\x1b["),
        "NO_COLOR=1 output must not contain ANSI escape codes"
    );
}

// ── Workspace state values ────────────────────────────────────────────────────

/// EARS: WHEN `polis status --json` is run THEN workspace.status is one of the valid states.
#[test]
fn test_status_json_workspace_status_is_valid_enum() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    let status = v["workspace"]["status"].as_str().expect("status string");

    // Must be one of: running, stopped, starting, stopping, error
    assert!(
        ["running", "stopped", "starting", "stopping", "error"].contains(&status),
        "workspace.status must be a valid WorkspaceState enum value, got: {status}"
    );
}

/// EARS: WHEN workspace is not fully running THEN status shows starting or stopped.
#[test]
fn test_status_workspace_state_reflects_container() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    let status = v["workspace"]["status"].as_str().expect("status string");

    // If security services are all inactive, workspace shouldn't be "running"
    // (unless container is actually running)
    let traffic = v["security"]["traffic_inspection"]
        .as_bool()
        .unwrap_or(false);
    let cred = v["security"]["credential_protection"]
        .as_bool()
        .unwrap_or(false);
    let malware = v["security"]["malware_scanning"].as_bool().unwrap_or(false);

    if !traffic && !cred && !malware {
        // Services not running - workspace is either starting, stopped, or error
        assert!(
            status != "running" || status == "starting" || status == "stopped" || status == "error",
            "if no services running, workspace should not be 'running' unless container is up"
        );
    }
}
