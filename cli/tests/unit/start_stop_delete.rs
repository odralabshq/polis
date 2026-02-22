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

/// Mock multipass that returns "VM not found"
struct MockNotFound;

impl Multipass for MockNotFound {
    fn vm_info(&self) -> Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: b"instance \"polis\" does not exist".to_vec(),
        })
    }
    fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("launch not expected in this test")
    }
    fn start(&self) -> Result<Output> {
        anyhow::bail!("start not expected in this test")
    }
    fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer not expected in this test")
    }
    fn exec(&self, _: &[&str]) -> Result<Output> {
        anyhow::bail!("exec not expected in this test")
    }
    fn version(&self) -> Result<Output> {
        anyhow::bail!("version not expected in this test")
    }
}

/// Mock multipass that returns "VM stopped"
struct MockStopped;

impl Multipass for MockStopped {
    fn vm_info(&self) -> Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: br#"{"info":{"polis":{"state":"Stopped"}}}"#.to_vec(),
            stderr: Vec::new(),
        })
    }
    fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("launch not expected in this test")
    }
    fn start(&self) -> Result<Output> {
        anyhow::bail!("start not expected in this test")
    }
    fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer not expected in this test")
    }
    fn exec(&self, _: &[&str]) -> Result<Output> {
        anyhow::bail!("exec not expected in this test")
    }
    fn version(&self) -> Result<Output> {
        anyhow::bail!("version not expected in this test")
    }
}

// ============================================================================
// polis stop
// ============================================================================

#[test]
fn test_stop_no_workspace_succeeds() {
    let mp = MockNotFound;
    let result = stop::run(&mp, true);
    assert!(result.is_ok());
}

#[test]
fn test_stop_already_stopped_succeeds() {
    let mp = MockStopped;
    let result = stop::run(&mp, true);
    assert!(result.is_ok());
}

// ============================================================================
// polis delete
// ============================================================================

#[test]
fn test_delete_no_workspace_succeeds() {
    let mp = MockNotFound;
    let args = DeleteArgs {
        all: false,
        yes: true,
    };
    let result = delete::run(&args, &mp, true);
    assert!(result.is_ok());
}

#[test]
fn test_delete_all_no_workspace_succeeds() {
    let mp = MockNotFound;
    let args = DeleteArgs {
        all: true,
        yes: true,
    };
    let result = delete::run(&args, &mp, true);
    assert!(result.is_ok());
}
