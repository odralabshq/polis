//! Integration tests for `polis status` command (issue 06).
//!
//! Tests use mocked Multipass to avoid slow real VM checks.

#![allow(clippy::expect_used)]

use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::commands::status;
use polis_cli::multipass::Multipass;
use polis_cli::output::OutputContext;

/// Mock multipass returning "VM not found"
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
        unimplemented!()
    }
    fn start(&self) -> Result<Output> {
        unimplemented!()
    }
    fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        unimplemented!()
    }
    fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
    fn version(&self) -> Result<Output> {
        unimplemented!()
    }
}

/// Mock multipass returning "VM stopped"
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
        unimplemented!()
    }
    fn start(&self) -> Result<Output> {
        unimplemented!()
    }
    fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        unimplemented!()
    }
    fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
    fn version(&self) -> Result<Output> {
        unimplemented!()
    }
}

#[tokio::test]
async fn test_status_no_workspace_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = MockNotFound;
    let result = status::run(&ctx, false, &mp).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_status_stopped_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = MockStopped;
    let result = status::run(&ctx, false, &mp).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_status_json_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let mp = MockNotFound;
    let result = status::run(&ctx, true, &mp).await;
    assert!(result.is_ok());
}
