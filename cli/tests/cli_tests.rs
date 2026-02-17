//! Integration tests for polis CLI skeleton
//!
//! These tests verify the CLI structure and argument parsing per spec 02-cli-crate-skeleton.md

#![allow(clippy::expect_used, deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;

fn polis() -> Command {
    Command::cargo_bin("polis").expect("polis binary should exist")
}

// --- Help and version tests ---

#[test]
fn test_cli_no_args_shows_help_and_exits_zero() {
    // clap with arg_required_else_help shows help on stderr and exits 2
    polis()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Secure workspaces for AI coding agents"));
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
        .stdout(predicate::str::contains(r#"{"version":"0.1.0"}"#));
}

// --- Command hierarchy tests ---

#[test]
fn test_help_shows_run_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("run"));
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
fn test_help_shows_logs_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("logs"));
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
fn test_help_shows_agents_command() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("agents"));
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
    polis()
        .args(["_ssh-proxy", "--help"])
        .assert()
        .success();
}

// --- Global flags tests ---

#[test]
fn test_global_json_flag_accepted() {
    polis()
        .args(["--json", "version"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"{"version":"#));
}

#[test]
fn test_global_quiet_flag_accepted() {
    polis()
        .args(["--quiet", "version"])
        .assert()
        .success();
}

#[test]
fn test_global_no_color_flag_accepted() {
    polis()
        .args(["--no-color", "version"])
        .assert()
        .success();
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

#[test]
fn test_unimplemented_command_exits_with_error() {
    polis()
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

// --- Subcommand argument tests ---

#[test]
fn test_run_accepts_agent_argument() {
    // Should fail with "not implemented" but parse successfully
    polis()
        .args(["run", "claude-dev"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_logs_accepts_follow_flag() {
    polis()
        .args(["logs", "--follow"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_logs_accepts_security_flag() {
    polis()
        .args(["logs", "--security"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_delete_accepts_all_flag() {
    polis()
        .args(["delete", "--all"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_connect_accepts_ide_option() {
    polis()
        .args(["connect", "--ide", "vscode"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_agents_list_subcommand() {
    polis()
        .args(["agents", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_agents_info_subcommand() {
    polis()
        .args(["agents", "info", "claude-dev"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_config_show_subcommand() {
    polis()
        .args(["config", "show"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_config_set_subcommand() {
    polis()
        .args(["config", "set", "key", "value"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}


// ============================================================================
// Property-Based Tests
// ============================================================================

#[cfg(test)]
mod proptests {
    use assert_cmd::Command;
    use predicates::prelude::*;
    use proptest::prelude::*;

    fn polis() -> Command {
        Command::cargo_bin("polis").expect("polis binary should exist")
    }

    proptest! {
        /// Any unknown command should fail with error
        #[test]
        fn prop_unknown_command_fails(cmd in "[a-z]{3,10}") {
            // Skip known commands
            let known = ["run", "start", "stop", "delete", "status", "logs", 
                        "shell", "connect", "agents", "config", "doctor", 
                        "update", "version", "help"];
            if known.contains(&cmd.as_str()) {
                return Ok(());
            }
            
            polis()
                .arg(&cmd)
                .assert()
                .failure();
        }

        /// Version command with --json always produces valid JSON structure
        #[test]
        fn prop_version_json_valid_structure(_seed in 0u32..1000) {
            let output = polis()
                .args(["version", "--json"])
                .output()
                .expect("command should run");
            
            let stdout = String::from_utf8_lossy(&output.stdout);
            prop_assert!(stdout.contains(r#""version":"#), "should contain version key");
            prop_assert!(stdout.trim().ends_with('}'), "should end with brace");
        }

        /// Global flags can be placed before any command
        #[test]
        fn prop_global_flags_before_version(
            json in proptest::bool::ANY,
            quiet in proptest::bool::ANY,
            no_color in proptest::bool::ANY,
        ) {
            let mut cmd = polis();
            if json { cmd.arg("--json"); }
            if quiet { cmd.arg("--quiet"); }
            if no_color { cmd.arg("--no-color"); }
            cmd.arg("version");
            
            cmd.assert().success();
        }

        /// Run command accepts any agent name string
        #[test]
        fn prop_run_accepts_agent_name(agent in "[a-z][a-z0-9-]{0,20}") {
            polis()
                .args(["run", &agent])
                .assert()
                .failure()
                .stderr(predicate::str::contains("not yet implemented"));
        }

        /// Agents info accepts any agent name
        #[test]
        fn prop_agents_info_accepts_name(name in "[a-z][a-z0-9-]{0,20}") {
            polis()
                .args(["agents", "info", &name])
                .assert()
                .failure()
                .stderr(predicate::str::contains("not yet implemented"));
        }

        /// Config set accepts any key-value pair
        #[test]
        fn prop_config_set_accepts_kv(
            key in "[a-z][a-z0-9_.]{0,20}",
            value in "[a-zA-Z0-9_.]{1,50}",  // No leading dash to avoid flag parsing
        ) {
            polis()
                .args(["config", "set", &key, &value])
                .assert()
                .failure()
                .stderr(predicate::str::contains("not yet implemented"));
        }
    }
}
