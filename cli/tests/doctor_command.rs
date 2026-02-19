//! Integration tests for `polis doctor` (issue 17).
//!
//! RED phase: tests 2–6 fail until the engineer implements `doctor::run()`.
//! Test 1 passes today (the subcommand is already registered in cli.rs).
//!
//! Testability requirement: `run()` must accept `impl HealthProbe` so unit
//! tests can inject a fake. The Senior Rust Engineer must extract the
//! `HealthProbe` trait before the unit tests in doctor.rs compile.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

#[test]
fn test_doctor_help_shows_description() {
    polis()
        .args(["doctor", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Diagnose").or(predicate::str::contains("diagnose")));
}

#[test]
fn test_doctor_command_does_not_say_not_yet_implemented() {
    polis()
        .arg("doctor")
        .assert()
        .stderr(predicate::str::contains("not yet implemented").not());
}

#[test]
fn test_doctor_json_flag_outputs_valid_json() {
    let output = polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    serde_json::from_str::<serde_json::Value>(&text).expect("output must be valid JSON");
}

#[test]
fn test_doctor_json_has_status_field() {
    let output = polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    let status = v["status"].as_str().expect("status must be a string");
    assert!(
        status == "healthy" || status == "unhealthy",
        "status must be 'healthy' or 'unhealthy', got: {status}"
    );
}

#[test]
fn test_doctor_json_has_required_check_sections() {
    let output = polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert!(
        v["checks"]["workspace"].is_object(),
        "checks.workspace must be an object"
    );
    assert!(
        v["checks"]["network"].is_object(),
        "checks.network must be an object"
    );
    assert!(
        v["checks"]["security"].is_object(),
        "checks.security must be an object"
    );
}

#[test]
fn test_doctor_json_issues_is_array() {
    let output = polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert!(v["issues"].is_array(), "issues must be a JSON array");
}
