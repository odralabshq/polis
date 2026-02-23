//! Unit tests for `polis start` command.

#![allow(clippy::expect_used)]

use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::commands::start::{generate_agent_artifacts, start_compose, validate_agent};
use polis_cli::multipass::Multipass;

use crate::helpers::{err_output, ok_output};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// VM is running; exec returns success for all calls.
struct VmRunningExecOk;

impl Multipass for VmRunningExecOk {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
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
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected")
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}

/// VM is running; exec always returns failure.
struct VmRunningExecFail;

impl Multipass for VmRunningExecFail {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
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
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(err_output(1, b"script failed"))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected")
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}

/// exec returns exit code 2 (missing yq).
struct VmRunningExecExitTwo;

impl Multipass for VmRunningExecExitTwo {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
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
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(2 << 8),
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
    async fn version(&self) -> Result<Output> {
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
    assert!(msg.contains("Unknown agent"), "got: {msg}");
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
        msg.contains("Failed to start platform") || msg.contains("platform"),
        "got: {msg}"
    );
}
