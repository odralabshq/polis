//! Integration tests for issue 09: doctor image health checks.
//!
//! Covers JSON schema for `checks.workspace.image` and display output
//! for image cached/missing and `POLIS_IMAGE` override (V-011, F-006).

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

fn doctor_json() -> serde_json::Value {
    let output = polis()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("valid JSON")
}

// ── JSON schema ───────────────────────────────────────────────────────────────

#[test]
fn test_doctor_json_workspace_image_is_object() {
    let v = doctor_json();
    assert!(
        v["checks"]["workspace"]["image"].is_object(),
        "checks.workspace.image must be a JSON object"
    );
}

#[test]
fn test_doctor_json_image_cached_is_boolean() {
    let v = doctor_json();
    assert!(
        v["checks"]["workspace"]["image"]["cached"].is_boolean(),
        "image.cached must be a boolean"
    );
}

#[test]
fn test_doctor_json_image_version_is_null_or_string() {
    let v = doctor_json();
    let field = &v["checks"]["workspace"]["image"]["version"];
    assert!(
        field.is_null() || field.is_string(),
        "image.version must be null or string, got: {field}"
    );
}

#[test]
fn test_doctor_json_image_sha256_preview_is_null_or_string() {
    let v = doctor_json();
    let field = &v["checks"]["workspace"]["image"]["sha256_preview"];
    assert!(
        field.is_null() || field.is_string(),
        "image.sha256_preview must be null or string, got: {field}"
    );
}

#[test]
fn test_doctor_json_image_polis_image_override_is_null_or_string() {
    let v = doctor_json();
    let field = &v["checks"]["workspace"]["image"]["polis_image_override"];
    assert!(
        field.is_null() || field.is_string(),
        "image.polis_image_override must be null or string, got: {field}"
    );
}

#[test]
fn test_doctor_json_image_version_drift_is_null_or_object() {
    let v = doctor_json();
    let field = &v["checks"]["workspace"]["image"]["version_drift"];
    assert!(
        field.is_null() || field.is_object(),
        "image.version_drift must be null or object, got: {field}"
    );
}

// ── Display output ────────────────────────────────────────────────────────────

#[test]
fn test_doctor_text_output_contains_image_status_line() {
    polis()
        .arg("doctor")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Image cached")
                .or(predicate::str::contains("No workspace image cached")),
        );
}

// ── POLIS_IMAGE override (V-011, F-006) ───────────────────────────────────────

#[test]
fn test_doctor_polis_image_override_set_to_existing_file_shows_warning() {
    let file = tempfile::NamedTempFile::new().expect("tempfile");
    polis()
        .arg("doctor")
        .env("POLIS_IMAGE", file.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("POLIS_IMAGE override active"));
}

#[test]
fn test_doctor_polis_image_override_set_to_missing_file_shows_file_not_found() {
    polis()
        .arg("doctor")
        .env("POLIS_IMAGE", "/nonexistent/custom.qcow2")
        .assert()
        .success()
        .stdout(predicate::str::contains("file not found"));
}
