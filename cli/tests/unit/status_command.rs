//! Unit tests for `polis status` command (issue 06).
//!
//! Tests use mocked Multipass to avoid slow real VM checks.

#![allow(clippy::expect_used)]

use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::commands::status;
use polis_cli::multipass::Multipass;
use polis_cli::output::OutputContext;

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
        Ok(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
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
        Ok(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
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

#[tokio::test]
async fn test_status_no_workspace_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = MultipassVmNotFound;
    let result = status::run(&ctx, false, &mp).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_status_stopped_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = MultipassVmStopped;
    let result = status::run(&ctx, false, &mp).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_status_json_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = MultipassVmNotFound;
    let result = status::run(&ctx, true, &mp).await;
    assert!(result.is_ok());
}
