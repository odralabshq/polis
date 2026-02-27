//! Unit tests for `polis start` command.

#![allow(clippy::expect_used)]

use std::process::Output;

use anyhow::Result;
use polis_cli::commands::start::{generate_agent_artifacts, start_compose, validate_agent};
use polis_cli::provisioner::{
    FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec, ShellExecutor,
};
use polis_cli::workspace::vm::generate_certs_and_secrets;

use crate::helpers::{err_output, exit_status, ok_output};
use crate::mocks::MultipassExecRecorder;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// VM is running; exec returns success for all calls.
struct VmRunningExecOk;

impl InstanceInspector for VmRunningExecOk {
    async fn info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}
impl InstanceLifecycle for VmRunningExecOk {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn start(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn stop(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
}
impl FileTransfer for VmRunningExecOk {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}
impl ShellExecutor for VmRunningExecOk {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        anyhow::bail!("not expected")
    }
}

/// VM is running; exec always returns failure.
struct VmRunningExecFail;

impl InstanceInspector for VmRunningExecFail {
    async fn info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}
impl InstanceLifecycle for VmRunningExecFail {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn start(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn stop(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
}
impl FileTransfer for VmRunningExecFail {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}
impl ShellExecutor for VmRunningExecFail {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(err_output(1, b"script failed"))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        anyhow::bail!("not expected")
    }
}

/// exec returns exit code 2 (missing yq).
struct VmRunningExecExitTwo;

impl InstanceInspector for VmRunningExecExitTwo {
    async fn info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}
impl InstanceLifecycle for VmRunningExecExitTwo {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn start(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn stop(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
}
impl FileTransfer for VmRunningExecExitTwo {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}
impl ShellExecutor for VmRunningExecExitTwo {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(Output {
            status: exit_status(2),
            stdout: Vec::new(),
            stderr: b"yq: command not found".to_vec(),
        })
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        anyhow::bail!("not expected")
    }
}

// ============================================================================
// validate_agent
// ============================================================================

#[tokio::test]
async fn test_validate_agent_manifest_exists_returns_ok() {
    let result: anyhow::Result<()> = validate_agent(&VmRunningExecOk, "openclaw").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_validate_agent_manifest_missing_returns_error() {
    let result: anyhow::Result<()> = validate_agent(&VmRunningExecFail, "nonexistent").await;
    assert!(result.is_err());
    let msg = result.expect_err("expected error").to_string();
    assert!(msg.contains("unknown agent"), "got: {msg}");
}

// ============================================================================
// generate_agent_artifacts
// ============================================================================

#[tokio::test]
async fn test_generate_agent_artifacts_success() {
    let result: anyhow::Result<()> = generate_agent_artifacts(&VmRunningExecOk, "openclaw").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_generate_agent_artifacts_failure_returns_error() {
    let result: anyhow::Result<()> = generate_agent_artifacts(&VmRunningExecFail, "openclaw").await;
    assert!(result.is_err());
    let msg = result.expect_err("expected error").to_string();
    assert!(
        msg.contains("artifact generation failed") || msg.contains("Agent artifact"),
        "got: {msg}"
    );
}

#[tokio::test]
async fn test_generate_agent_artifacts_exit_2_mentions_yq() {
    let result: anyhow::Result<()> =
        generate_agent_artifacts(&VmRunningExecExitTwo, "openclaw").await;
    assert!(result.is_err());
    let msg = result.expect_err("expected error").to_string();
    assert!(msg.contains("yq"), "expected yq mention, got: {msg}");
}

// ============================================================================
// start_compose
// ============================================================================

#[tokio::test]
async fn test_start_compose_no_agent_succeeds() {
    let result: anyhow::Result<()> = start_compose(&VmRunningExecOk, None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_start_compose_with_agent_succeeds() {
    let result: anyhow::Result<()> = start_compose(&VmRunningExecOk, Some("openclaw")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_start_compose_failure_returns_error() {
    let result: anyhow::Result<()> = start_compose(&VmRunningExecFail, None).await;
    assert!(result.is_err());
    let msg = result.expect_err("expected error").to_string();
    assert!(
        msg.contains("failed to start platform") || msg.contains("platform"),
        "got: {msg}"
    );
}

// ============================================================================
// generate_certs_and_secrets
// ============================================================================

/// Expected exec calls in order: 5 scripts + 1 logger = 6 total.
const EXPECTED_CALLS: &[&[&str]] = &[
    &[
        "sudo",
        "bash",
        "-c",
        "/opt/polis/scripts/generate-ca.sh /opt/polis/certs/ca",
    ],
    &[
        "sudo",
        "bash",
        "-c",
        "/opt/polis/services/state/scripts/generate-certs.sh /opt/polis/certs/valkey",
    ],
    &[
        "sudo",
        "bash",
        "-c",
        "/opt/polis/services/state/scripts/generate-secrets.sh /opt/polis/secrets /opt/polis",
    ],
    &[
        "sudo",
        "bash",
        "-c",
        "/opt/polis/services/toolbox/scripts/generate-certs.sh /opt/polis/certs/toolbox /opt/polis/certs/ca",
    ],
    &[
        "sudo",
        "bash",
        "-c",
        "/opt/polis/scripts/fix-cert-ownership.sh /opt/polis",
    ],
    &[
        "bash",
        "-c",
        "logger -t polis 'Certificate and secret generation completed'",
    ],
];

#[tokio::test]
async fn test_generate_certs_and_secrets_makes_exactly_6_exec_calls() {
    let mp = MultipassExecRecorder::new();
    generate_certs_and_secrets(&mp)
        .await
        .expect("should succeed");
    let calls = mp.recorded_calls();
    assert_eq!(
        calls.len(),
        6,
        "expected exactly 6 exec calls (5 scripts + 1 logger), got {}: {calls:?}",
        calls.len()
    );
}

#[tokio::test]
async fn test_generate_certs_and_secrets_calls_in_correct_order() {
    let mp = MultipassExecRecorder::new();
    generate_certs_and_secrets(&mp)
        .await
        .expect("should succeed");
    let calls = mp.recorded_calls();

    for (i, expected) in EXPECTED_CALLS.iter().enumerate() {
        let actual: Vec<&str> = calls[i].iter().map(String::as_str).collect();
        assert_eq!(
            actual, *expected,
            "call {i} mismatch: expected {expected:?}, got {actual:?}"
        );
    }
}

#[tokio::test]
async fn test_generate_certs_and_secrets_script_calls_use_sudo() {
    let mp = MultipassExecRecorder::new();
    generate_certs_and_secrets(&mp)
        .await
        .expect("should succeed");
    let calls = mp.recorded_calls();

    // The first 5 calls are script invocations — all must start with "sudo".
    for (i, call) in calls.iter().take(5).enumerate() {
        assert_eq!(
            call.first().map(String::as_str),
            Some("sudo"),
            "call {i} should start with 'sudo', got: {call:?}"
        );
    }
}

#[tokio::test]
async fn test_generate_certs_and_secrets_logger_call_does_not_use_sudo() {
    let mp = MultipassExecRecorder::new();
    generate_certs_and_secrets(&mp)
        .await
        .expect("should succeed");
    let calls = mp.recorded_calls();

    // The 6th call (index 5) is the logger — must NOT start with "sudo".
    let logger_call = &calls[5];
    assert_ne!(
        logger_call.first().map(String::as_str),
        Some("sudo"),
        "logger call should not use sudo, got: {logger_call:?}"
    );
    assert_eq!(
        logger_call.first().map(String::as_str),
        Some("bash"),
        "logger call should start with 'bash', got: {logger_call:?}"
    );
}

#[tokio::test]
async fn test_generate_certs_and_secrets_all_script_paths_rooted_at_opt_polis() {
    let mp = MultipassExecRecorder::new();
    generate_certs_and_secrets(&mp)
        .await
        .expect("should succeed");
    let calls = mp.recorded_calls();

    // For each of the 5 script calls, the 4th argument (index 3) is the
    // shell command string — it must contain a path rooted at /opt/polis/.
    for (i, call) in calls.iter().take(5).enumerate() {
        let cmd = call.get(3).map_or("", String::as_str);
        assert!(
            cmd.contains("/opt/polis/"),
            "call {i} script path should be rooted at /opt/polis/, got: {cmd:?}"
        );
    }
}
