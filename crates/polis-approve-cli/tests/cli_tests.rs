use assert_cmd::cargo;
use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

// NOTE: These tests expect a running Valkey instance or will fail/skip.
// For CI, we would typically spin up a container. For this environment,
// we will verify the binary logic where possible without a live connection
// or mock the connection if we refactor.
//
// Since we cannot easily mock the redis crate's async connection in a binary integration test,
// we will focus on:
// 1. Argument parsing (help, version)
// 2. Failure modes (missing env vars)

#[test]
fn test_help() {
    let mut cmd = Command::new(cargo::cargo_bin!("polis-approve"));
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("polis HITL approval CLI tool"));
}

#[test]
fn test_version() {
    let mut cmd = Command::new(cargo::cargo_bin!("polis-approve"));
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("polis-approve"));
}

#[test]
fn test_missing_env_var() {
    // Should fail because polis_VALKEY_PASS is missing
    let mut cmd = Command::new(cargo::cargo_bin!("polis-approve"));
    cmd.env_remove("polis_VALKEY_PASS")
        .arg("list-pending")
        .assert()
        .failure()
        .stderr(predicate::str::contains("polis_VALKEY_PASS env var is required"));
}

#[test]
fn test_invalid_subcommand() {
    let mut cmd = Command::new(cargo::cargo_bin!("polis-approve"));
    cmd.env("polis_VALKEY_PASS", "dummy")
        .arg("invalid-cmd")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}
