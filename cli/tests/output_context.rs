//! Integration tests for `OutputContext` helper methods (issue 03).
//!
//! These tests run the binary and verify that styled output reaches the
//! terminal correctly. `polis doctor` is the best existing command that
//! exercises the styling system (header + success/error markers).

#![allow(clippy::expect_used, deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::cargo_bin("polis").expect("polis binary should exist")
}

// ---------------------------------------------------------------------------
// Human-readable output contains expected markers
// ---------------------------------------------------------------------------

#[test]
fn test_doctor_human_output_contains_success_marker() {
    // doctor prints "✓" for passing checks via ctx.styles.success
    polis()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains('✓'));
}

#[test]
fn test_doctor_human_output_contains_header() {
    // doctor prints "Polis Health Check" via ctx.styles.header
    polis()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("Polis Health Check"));
}

#[test]
fn test_doctor_human_output_contains_section_labels() {
    polis()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("Workspace:"))
        .stdout(predicate::str::contains("Network:"))
        .stdout(predicate::str::contains("Security:"));
}

// ---------------------------------------------------------------------------
// JSON mode suppresses styled markers
// ---------------------------------------------------------------------------

#[test]
fn test_doctor_json_output_does_not_contain_check_mark() {
    // JSON mode bypasses all styled helpers — no ✓ or ✗ in output
    polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains('✓').not())
        .stdout(predicate::str::contains('✗').not());
}

#[test]
fn test_doctor_json_output_does_not_contain_header_text_as_plain_line() {
    // "Polis Health Check" only appears in human mode, not in JSON
    polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Polis Health Check").not());
}

// ---------------------------------------------------------------------------
// --no-color strips ANSI escape codes from output
// ---------------------------------------------------------------------------

#[test]
fn test_doctor_no_color_flag_strips_ansi_codes() {
    let output = polis()
        .args(["--no-color", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    assert!(
        !text.contains("\x1b["),
        "no-color flag must strip ANSI escape codes from output"
    );
}

#[test]
fn test_doctor_no_color_env_strips_ansi_codes() {
    let output = polis()
        .env("NO_COLOR", "1")
        .arg("doctor")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("utf8");
    assert!(
        !text.contains("\x1b["),
        "NO_COLOR env must strip ANSI escape codes from output"
    );
}

// ---------------------------------------------------------------------------
// Property-based integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptests {
    use assert_cmd::Command;
    use proptest::prelude::*;

    fn polis() -> Command {
        Command::cargo_bin("polis").expect("polis binary should exist")
    }

    proptest! {
        /// doctor --json always produces valid JSON regardless of color flags
        #[test]
        fn prop_doctor_json_always_valid(
            no_color in proptest::bool::ANY,
        ) {
            let mut cmd = polis();
            if no_color {
                cmd.arg("--no-color");
            }
            cmd.arg("doctor").arg("--json");

            let output = cmd.output().expect("command should run");
            let text = String::from_utf8_lossy(&output.stdout);
            let parsed = serde_json::from_str::<serde_json::Value>(&text);
            prop_assert!(parsed.is_ok(), "doctor --json must always produce valid JSON");
        }

        /// doctor human output always contains ✓ (at least one check passes)
        #[test]
        fn prop_doctor_human_always_has_check_mark(no_color in proptest::bool::ANY) {
            let mut cmd = polis();
            if no_color {
                cmd.arg("--no-color");
            }
            cmd.arg("doctor");

            let output = cmd.output().expect("command should run");
            let text = String::from_utf8_lossy(&output.stdout);
            prop_assert!(
                text.contains('✓'),
                "doctor human output must contain at least one ✓"
            );
        }

        /// --no-color never produces ANSI escape codes in doctor output
        #[test]
        fn prop_no_color_never_produces_ansi_in_doctor(_seed in 0u32..20) {
            let output = polis()
                .args(["--no-color", "doctor"])
                .output()
                .expect("command should run");
            let text = String::from_utf8_lossy(&output.stdout);
            prop_assert!(
                !text.contains("\x1b["),
                "--no-color must strip all ANSI codes"
            );
        }
    }
}
