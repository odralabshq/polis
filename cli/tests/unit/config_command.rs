//! Unit tests for `polis config set` Valkey propagation.
//!
//! Tests use mocked Multipass to verify that `config set security.level`
//! propagates to Valkey on success and warns (without failing) on error.
//!
//! IMPORTANT: These tests mutate `POLIS_CONFIG` env var and must run with
//! `--test-threads=1` to avoid races.

#![allow(clippy::expect_used, clippy::unwrap_used, unsafe_code)]

use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::commands::config::{self, ConfigCommand};
use polis_cli::multipass::Multipass;
use polis_cli::output::OutputContext;
use tempfile::TempDir;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn ok_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

fn err_output(stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(1 << 8),
        stdout: Vec::new(),
        stderr: stderr.to_vec(),
    }
}

fn ctx() -> OutputContext {
    OutputContext::new(true, true)
}

fn set_cmd(value: &str) -> (TempDir, ConfigCommand) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("config.yaml");
    // SAFETY: tests that use this helper must run with --test-threads=1.
    // set_var is needed because config::get_config_path() reads POLIS_CONFIG.
    unsafe { std::env::set_var("POLIS_CONFIG", &path) };
    let cmd = ConfigCommand::Set {
        key: "security.level".to_string(),
        value: value.to_string(),
    };
    (dir, cmd)
}

// ── Shared mock boilerplate ──────────────────────────────────────────────────
//
// The Multipass trait has 12 methods. Only `exec` is exercised by config
// propagation. Every other method bails with its name so unexpected calls
// surface immediately (per TESTER.md manual-mock convention).

macro_rules! multipass_stub_methods {
    () => {
        async fn vm_info(&self) -> Result<Output> {
            // Propagation tests require a running VM
            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: br#"{"info":{"polis":{"state":"Running"}}}"#.to_vec(),
                stderr: Vec::new(),
            })
        }
        async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("launch not expected in this test")
        }
        async fn start(&self) -> Result<Output> {
            anyhow::bail!("start not expected in this test")
        }
        async fn stop(&self) -> Result<Output> {
            anyhow::bail!("stop not expected in this test")
        }
        async fn delete(&self) -> Result<Output> {
            anyhow::bail!("delete not expected in this test")
        }
        async fn purge(&self) -> Result<Output> {
            anyhow::bail!("purge not expected in this test")
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("transfer not expected in this test")
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("transfer_recursive not expected in this test")
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            anyhow::bail!("exec_with_stdin not expected in this test")
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            anyhow::bail!("exec_spawn not expected in this test")
        }
        async fn version(&self) -> Result<Output> {
            anyhow::bail!("version not expected in this test")
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            anyhow::bail!("exec_status not expected in this test")
        }
    };
}

// ── Mock: propagation succeeds ───────────────────────────────────────────────

/// Password read OK, valkey-cli SET returns OK.
struct MockPropagateOk;

impl Multipass for MockPropagateOk {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        if args.contains(&"cat") {
            return Ok(ok_output(b"s3cret\n"));
        }
        if args.contains(&"valkey-cli") {
            return Ok(ok_output(b"OK\n"));
        }
        anyhow::bail!("exec not expected with args: {args:?}")
    }
    multipass_stub_methods!();
}

// ── Mock: password read fails ────────────────────────────────────────────────

/// VM not running — `cat` of password file returns non-zero.
struct MockPasswordReadFails;

impl Multipass for MockPasswordReadFails {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        if args.contains(&"cat") {
            return Ok(err_output(b"No such file or directory"));
        }
        anyhow::bail!("exec not expected with args: {args:?}")
    }
    multipass_stub_methods!();
}

// ── Mock: valkey SET fails ───────────────────────────────────────────────────

/// Password read OK but valkey-cli SET returns NOPERM.
struct MockValkeySetFails;

impl Multipass for MockValkeySetFails {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        if args.contains(&"cat") {
            return Ok(ok_output(b"s3cret\n"));
        }
        if args.contains(&"valkey-cli") {
            return Ok(err_output(b"NOPERM"));
        }
        anyhow::bail!("exec not expected with args: {args:?}")
    }
    multipass_stub_methods!();
}

// ── Mock: multipass exec itself errors ───────────────────────────────────────

/// Multipass not installed or VM unreachable.
struct MockExecErrors;

impl Multipass for MockExecErrors {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        anyhow::bail!("failed to run multipass exec")
    }
    multipass_stub_methods!();
}

// ── Mock: captures exec args ─────────────────────────────────────────────────

/// Records every `exec` call for argument verification.
struct MockCaptureArgs {
    captured: std::sync::Mutex<Vec<Vec<String>>>,
}

impl MockCaptureArgs {
    fn new() -> Self {
        Self {
            captured: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn exec_calls(&self) -> Vec<Vec<String>> {
        self.captured.lock().expect("lock").clone()
    }
}

impl Multipass for MockCaptureArgs {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        self.captured
            .lock()
            .expect("lock")
            .push(args.iter().map(|s| (*s).to_string()).collect());
        Ok(ok_output(if args.contains(&"cat") {
            b"testpass\n"
        } else {
            b"OK\n"
        }))
    }
    multipass_stub_methods!();
}

// ── Tests: propagation succeeds ──────────────────────────────────────────────

#[tokio::test]
async fn test_config_set_security_level_propagates_on_success() {
    let (_dir, cmd) = set_cmd("strict");
    let result = config::run(&ctx(), cmd, false, &MockPropagateOk).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_set_security_level_saves_file_on_success() {
    let (dir, cmd) = set_cmd("strict");
    let path = dir.path().join("config.yaml");
    config::run(&ctx(), cmd, false, &MockPropagateOk)
        .await
        .expect("should succeed");
    let content = std::fs::read_to_string(&path).expect("file should exist");
    assert!(content.contains("strict"));
}

// ── Tests: propagation fails gracefully ──────────────────────────────────────

#[tokio::test]
async fn test_config_set_succeeds_when_password_read_fails() {
    let (_dir, cmd) = set_cmd("strict");
    let result = config::run(&ctx(), cmd, false, &MockPasswordReadFails).await;
    assert!(
        result.is_ok(),
        "local save should succeed even if propagation fails"
    );
}

#[tokio::test]
async fn test_config_set_succeeds_when_valkey_set_fails() {
    let (_dir, cmd) = set_cmd("balanced");
    let result = config::run(&ctx(), cmd, false, &MockValkeySetFails).await;
    assert!(
        result.is_ok(),
        "local save should succeed even if Valkey SET fails"
    );
}

#[tokio::test]
async fn test_config_set_succeeds_when_exec_errors() {
    let (_dir, cmd) = set_cmd("balanced");
    let result = config::run(&ctx(), cmd, false, &MockExecErrors).await;
    assert!(
        result.is_ok(),
        "local save should succeed even if multipass exec errors"
    );
}

#[tokio::test]
async fn test_config_set_saves_file_even_when_propagation_fails() {
    let (dir, cmd) = set_cmd("strict");
    let path = dir.path().join("config.yaml");
    config::run(&ctx(), cmd, false, &MockPasswordReadFails)
        .await
        .expect("should succeed");
    let content = std::fs::read_to_string(&path).expect("file should exist");
    assert!(
        content.contains("strict"),
        "value should be persisted locally"
    );
}

// ── Tests: show does not call exec ───────────────────────────────────────────

#[tokio::test]
async fn test_config_show_does_not_interact_with_multipass() {
    let (_dir, _) = set_cmd("balanced"); // sets POLIS_CONFIG
    let cmd = ConfigCommand::Show;
    // MockExecErrors bails on any exec call — proves show is exec-free.
    let result = config::run(&ctx(), cmd, false, &MockExecErrors).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_config_show_json_does_not_interact_with_multipass() {
    let (_dir, _) = set_cmd("balanced");
    let cmd = ConfigCommand::Show;
    let result = config::run(&ctx(), cmd, true, &MockExecErrors).await;
    assert!(result.is_ok());
}

// ── Tests: exec receives correct arguments ───────────────────────────────────

#[tokio::test]
async fn test_config_set_propagation_passes_level_as_separate_arg() {
    let (_dir, cmd) = set_cmd("strict");
    let mock = MockCaptureArgs::new();
    config::run(&ctx(), cmd, false, &mock).await.expect("ok");

    let calls = mock.exec_calls();
    assert_eq!(calls.len(), 2, "expected password read + valkey-cli SET");

    // First call: password read
    assert!(calls[0].contains(&"cat".to_string()));

    // Second call: valkey-cli SET with level as a discrete arg (no shell interpolation)
    let set_call = &calls[1];
    assert!(set_call.contains(&"valkey-cli".to_string()));
    assert!(set_call.contains(&"SET".to_string()));
    assert!(set_call.contains(&"polis:config:security_level".to_string()));
    assert!(set_call.contains(&"strict".to_string()));
    // Password passed via REDISCLI_AUTH env var (not -a flag) to avoid process list exposure
    assert!(
        set_call.iter().any(|a| a.starts_with("REDISCLI_AUTH=")),
        "password should be passed via REDISCLI_AUTH env var, got: {set_call:?}"
    );
    assert!(
        !set_call.contains(&"-a".to_string()),
        "-a flag should not be used (exposes password in process list)"
    );
}

#[tokio::test]
async fn test_config_set_propagation_does_not_use_shell() {
    let (_dir, cmd) = set_cmd("balanced");
    let mock = MockCaptureArgs::new();
    config::run(&ctx(), cmd, false, &mock).await.expect("ok");

    let calls = mock.exec_calls();
    for call in &calls {
        assert!(
            !call.contains(&"bash".to_string()) && !call.contains(&"sh".to_string()),
            "propagation must not use shell: {call:?}"
        );
    }
}
