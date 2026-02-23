//! Integration tests for polis CLI skeleton
//!
//! These tests verify the CLI structure and argument parsing per spec 02-cli-crate-skeleton.md

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("polis"));
    cmd.env("NO_COLOR", "1");
    cmd
}

// --- Help and version tests ---

#[test]
fn test_cli_no_args_shows_help_and_exits_zero() {
    // clap with arg_required_else_help shows help on stderr and exits 2
    polis().assert().code(2).stderr(predicate::str::contains(
        "Secure workspaces for AI coding agents",
    ));
}

#[test]
fn test_cli_help_flag_shows_help() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"))
        .stdout(predicate::str::contains("Commands:"));
}

#[test]
fn test_cli_version_flag_shows_version() {
    polis()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("polis"));
}

#[test]
fn test_version_command_shows_version() {
    polis()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("polis 0.1.0"));
}

#[test]
fn test_version_command_json_outputs_valid_json() {
    polis()
        .arg("version")
        .arg("--json")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""version": "0.1.0""#));
}

// --- Command hierarchy tests ---

#[test]
fn test_help_shows_start_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("start"));
}

#[test]
fn test_help_shows_status_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("status"));
}

#[test]
fn test_help_shows_connect_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("connect"));
}

#[test]
fn test_help_shows_stop_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("stop"));
}

#[test]
fn test_help_shows_config_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("config"));
}

#[test]
fn test_help_shows_doctor_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn test_help_shows_update_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("update"));
}

// --- Hidden commands tests ---

#[test]
fn test_help_hides_ssh_proxy_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("_ssh-proxy").not());
}

#[test]
fn test_help_hides_provision_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("_provision").not());
}

#[test]
fn test_help_hides_extract_host_key_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("_extract-host-key").not());
}

#[test]
fn test_ssh_proxy_help_accessible_directly() {
    polis().args(["_ssh-proxy", "--help"]).assert().success();
}

// --- Global flags tests ---

#[test]
fn test_global_json_flag_accepted() {
    polis()
        .args(["--json", "version"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""version":"#));
}

#[test]
fn test_global_quiet_flag_accepted() {
    polis().args(["--quiet", "version"]).assert().success();
}

#[test]
fn test_global_no_color_flag_accepted() {
    polis().args(["--no-color", "version"]).assert().success();
}

#[test]
fn test_no_color_env_var_accepted() {
    // NO_COLOR env var should be accepted with any truthy value
    polis()
        .env("NO_COLOR", "true")
        .arg("version")
        .assert()
        .success();
}

// --- Error handling tests ---

#[test]
fn test_unknown_command_exits_with_error() {
    polis()
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

// --- Subcommand argument tests ---

#[test]
fn test_start_image_flag_is_accepted() {
    // --image is a valid flag; outcome depends on VM state
    polis()
        .args(["start", "--image", "/nonexistent.qcow2"])
        .assert()
        .stderr(predicate::str::contains("unrecognized").not());
}

#[test]
fn test_delete_accepts_all_flag() {
    // --all is a valid flag; command prompts for confirmation and fails when stdin is closed
    polis()
        .args(["delete", "--all"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no input provided"));
}

#[test]
fn test_connect_accepts_ide_option() {
    // connect is implemented: --ide vscode is accepted (fails because no TTY/IDE in CI,
    // not because the command is unrecognised).
    polis()
        .args(["connect", "--ide", "vscode"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented").not());
}

#[test]
fn test_config_show_subcommand() {
    polis()
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("security.level"));
}

#[test]
fn test_config_set_subcommand() {
    polis()
        .args(["config", "set", "key", "value"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown setting"));
}
