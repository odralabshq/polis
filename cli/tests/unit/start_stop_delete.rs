//! Unit tests for `polis start`, `polis stop`, and `polis delete [--all]`.
//!
//! Tests use mocked Multipass to avoid slow real calls.

#![allow(clippy::expect_used)]

use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::commands::delete;
use polis_cli::commands::{DeleteArgs, stop};
use polis_cli::multipass::Multipass;

/// Mock multipass that simulates VM not found (exit code 1).
struct MultipassVmNotFound;

impl Multipass for MultipassVmNotFound {
    async fn vm_info(&self) -> Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: b"instance \"polis\" does not exist".to_vec(),
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
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        anyhow::bail!("exec not expected in this test")
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
}

/// Mock multipass that simulates VM in stopped state.
struct MultipassVmStopped;

impl Multipass for MultipassVmStopped {
    async fn vm_info(&self) -> Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: br#"{"info":{"polis":{"state":"Stopped"}}}"#.to_vec(),
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
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        anyhow::bail!("exec not expected in this test")
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
}

// ============================================================================
// polis stop
// ============================================================================

#[tokio::test]
async fn test_stop_no_workspace_succeeds() {
    let mp = MultipassVmNotFound;
    let result = stop::run(&mp, true).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_stop_already_stopped_succeeds() {
    let mp = MultipassVmStopped;
    let result = stop::run(&mp, true).await;
    assert!(result.is_ok());
}

// ============================================================================
// polis delete
// ============================================================================

#[tokio::test]
async fn test_delete_no_workspace_succeeds() {
    let mp = MultipassVmNotFound;
    let args = DeleteArgs {
        all: false,
        yes: true,
    };
    let result = delete::run(&args, &mp, true).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_all_no_workspace_succeeds() {
    let mp = MultipassVmNotFound;
    let args = DeleteArgs {
        all: true,
        yes: true,
    };
    let result = delete::run(&args, &mp, true).await;
    assert!(result.is_ok());
}
