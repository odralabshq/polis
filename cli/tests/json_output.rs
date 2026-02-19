//! Integration tests for `--json` output across commands (issue 18).
//!
//! # RED / GREEN map
//!
//! | Test | State | Reason |
//! |------|-------|--------|
//! | `test_status_json_*` (6 tests) | 🔴 RED | `Command::Status` not wired in `cli.rs` |
//! | `test_version_json_is_pretty_printed` | 🔴 RED | `version::run` uses compact JSON |
//! | `test_agents_list_json_*` (2 tests) | 🟢 GREEN | already implemented |
//! | `test_version_json_outputs_valid_json` | 🟢 GREEN | already implemented |
//! | `test_json_quiet_json_takes_precedence` | 🟢 GREEN | `version::run` ignores `--quiet` |
//!
//! Spec: docs/linear-issues/polis-oss/ux-improvements/18-json-output.md
//! Depends on: 06-status-command, 15-agents-commands, 17-doctor-command

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

// ── polis status --json ───────────────────────────────────────────────────────
// All six tests below are RED: `Command::Status` is not yet wired in cli.rs.

/// EARS: WHEN `polis status --json` is run THEN it exits successfully.
#[test]
fn test_status_json_exits_successfully() {
    polis().args(["status", "--json"]).assert().success();
}

/// EARS: WHEN `polis status --json` is run THEN stdout is valid JSON.
#[test]
fn test_status_json_outputs_valid_json() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    serde_json::from_str::<serde_json::Value>(&text).expect("stdout must be valid JSON");
}

/// EARS: WHEN `polis status --json` is run THEN output contains `workspace` object.
#[test]
fn test_status_json_schema_has_workspace() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert!(
        v["workspace"].is_object(),
        "status JSON must have a 'workspace' object"
    );
    assert!(
        v["workspace"]["status"].is_string(),
        "workspace.status must be a string"
    );
}

/// EARS: WHEN `polis status --json` is run THEN output contains `security` object.
#[test]
fn test_status_json_schema_has_security() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert!(
        v["security"].is_object(),
        "status JSON must have a 'security' object"
    );
    assert!(
        v["security"]["traffic_inspection"].is_boolean(),
        "security.traffic_inspection must be a boolean"
    );
}

/// EARS: WHEN `polis status --json` is run THEN output contains `events` object.
#[test]
fn test_status_json_schema_has_events() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert!(
        v["events"].is_object(),
        "status JSON must have an 'events' object"
    );
    assert!(
        v["events"]["count"].is_number(),
        "events.count must be a number"
    );
}

/// NFR: `polis status --json` output must be pretty-printed (multi-line).
#[test]
fn test_status_json_is_pretty_printed() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    assert!(
        text.trim().contains('\n'),
        "status --json must be pretty-printed (internal newlines required)"
    );
}

// ── polis version --json ──────────────────────────────────────────────────────

/// EARS: WHEN `polis version --json` is run THEN stdout is valid JSON.
/// State: 🟢 GREEN — already implemented.
#[test]
fn test_version_json_outputs_valid_json() {
    let output = polis()
        .args(["version", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    let v: serde_json::Value = serde_json::from_str(&text).expect("stdout must be valid JSON");
    assert!(
        v["version"].is_string(),
        "version JSON must have a 'version' string field"
    );
}

/// NFR: `polis version --json` output must be pretty-printed (multi-line).
/// State: 🔴 RED — `version::run` currently uses `println!(r#"{{"version":...}}"#)` (compact).
#[test]
fn test_version_json_is_pretty_printed() {
    let output = polis()
        .args(["version", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    assert!(
        text.trim().contains('\n'),
        "version --json must be pretty-printed (internal newlines required); got: {text:?}"
    );
}

// ── polis agents list --json ──────────────────────────────────────────────────
// Both tests are 🟢 GREEN — already implemented in agents.rs.

/// EARS: WHEN `polis agents list --json` is run THEN stdout is a valid JSON array.
#[test]
fn test_agents_list_json_outputs_valid_json_array() {
    let output = polis()
        .args(["agents", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    let v: serde_json::Value = serde_json::from_str(&text).expect("stdout must be valid JSON");
    assert!(v.is_array(), "agents list --json must output a JSON array");
}

/// EARS: WHEN agents are present THEN each entry has name/provider/version/capabilities.
/// With no agents installed the array is empty — schema check is vacuously satisfied.
#[test]
fn test_agents_list_json_schema_fields() {
    let output = polis()
        .args(["agents", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let arr: Vec<serde_json::Value> = serde_json::from_slice(&output).expect("valid JSON array");

    for entry in &arr {
        assert!(entry["name"].is_string(), "agent entry must have 'name'");
        assert!(
            entry["provider"].is_string(),
            "agent entry must have 'provider'"
        );
        assert!(
            entry["version"].is_string(),
            "agent entry must have 'version'"
        );
        assert!(
            entry["capabilities"].is_array(),
            "agent entry must have 'capabilities' array"
        );
    }
}

// ── --json + --quiet precedence ───────────────────────────────────────────────
// 🟢 GREEN — `version::run` ignores `--quiet`; JSON is always emitted.

/// EARS: WHEN `--json` and `--quiet` are both passed THEN `--json` takes precedence.
#[test]
fn test_json_quiet_json_takes_precedence() {
    let output = polis()
        .args(["--json", "--quiet", "version"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    let v: serde_json::Value =
        serde_json::from_str(&text).expect("--json must take precedence over --quiet");
    assert!(v["version"].is_string());
}

// ── no non-JSON output in JSON mode ──────────────────────────────────────────
// 🔴 RED — `polis status --json` is not yet implemented.

/// NFR: `polis status --json` stdout must be parseable as JSON with no surrounding text.
#[test]
fn test_status_json_stdout_is_pure_json() {
    let output = polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    // The entire trimmed stdout must parse as a single JSON value.
    serde_json::from_str::<serde_json::Value>(text.trim())
        .expect("status --json stdout must be pure JSON with no surrounding text");
}

/// NFR: `polis status --json` must not emit ANSI escape codes.
#[test]
fn test_status_json_no_ansi_codes() {
    polis()
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b[").not());
}

// ── polis doctor --json NFRs ──────────────────────────────────────────────────

/// NFR: `polis doctor --json` output must be pretty-printed (multi-line).
#[test]
fn test_doctor_json_is_pretty_printed() {
    let output = polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    assert!(
        text.trim().contains('\n'),
        "doctor --json must be pretty-printed; got: {text:?}"
    );
}

/// NFR: `polis doctor --json` stdout must be parseable as a single JSON value.
#[test]
fn test_doctor_json_stdout_is_pure_json() {
    let output = polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    serde_json::from_str::<serde_json::Value>(text.trim())
        .expect("doctor --json stdout must be pure JSON");
}

/// NFR: `polis doctor --json` must not emit ANSI escape codes.
#[test]
fn test_doctor_json_no_ansi_codes() {
    polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b[").not());
}

// ── polis agents list --json NFRs ─────────────────────────────────────────────

/// NFR: `polis agents list --json` must not emit ANSI escape codes.
#[test]
fn test_agents_list_json_no_ansi_codes() {
    polis()
        .args(["agents", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b[").not());
}

// ── polis version --json NFRs ─────────────────────────────────────────────────

/// NFR: `polis version --json` stdout must be parseable as a single JSON value.
#[test]
fn test_version_json_stdout_is_pure_json() {
    let output = polis()
        .args(["version", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    serde_json::from_str::<serde_json::Value>(text.trim())
        .expect("version --json stdout must be pure JSON");
}

/// NFR: `polis version --json` must not emit ANSI escape codes.
#[test]
fn test_version_json_no_ansi_codes() {
    polis()
        .args(["version", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b[").not());
}

/// Schema: `polis version --json` `version` field must be a non-empty string.
#[test]
fn test_version_json_version_field_is_non_empty_string() {
    let output = polis()
        .args(["version", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    let version = v["version"].as_str().expect("version must be a string");
    assert!(!version.is_empty(), "version must not be empty");
}
