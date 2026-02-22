//! Unit tests for `polis start`, `polis stop`, and `polis delete [--all]`.

#![allow(clippy::expect_used)]

use std::process::Output;

use anyhow::Result;
use polis_cli::commands::delete;
use polis_cli::commands::{DeleteArgs, stop};
use polis_cli::multipass::Multipass;

use crate::helpers::{VmNotFound, VmRunning, VmStopped, ok_output};

// ── VmRunning that also handles stop/exec for compose shutdown ───────────────

struct VmRunningWithStop;

impl Multipass for VmRunningWithStop {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected")
    }
    async fn start(&self) -> Result<Output> {
        anyhow::bail!("not expected")
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
        // compose stop + multipass stop both go through here or stop()
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

// ============================================================================
// polis stop
// ============================================================================

#[tokio::test]
async fn test_stop_no_workspace_succeeds() {
    let result = stop::run(&VmNotFound, true).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_stop_already_stopped_succeeds() {
    let result = stop::run(&VmStopped, true).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_stop_running_workspace_succeeds() {
    let result = stop::run(&VmRunningWithStop, true).await;
    assert!(result.is_ok());
}

// ============================================================================
// polis delete
// ============================================================================

#[tokio::test]
async fn test_delete_no_workspace_succeeds() {
    let args = DeleteArgs {
        all: false,
        yes: true,
    };
    let result = delete::run(&args, &VmNotFound, true).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_all_no_workspace_succeeds() {
    let args = DeleteArgs {
        all: true,
        yes: true,
    };
    let result = delete::run(&args, &VmNotFound, true).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_running_workspace_succeeds() {
    let args = DeleteArgs {
        all: false,
        yes: true,
    };
    let result = delete::run(&args, &VmRunning, true).await;
    assert!(result.is_ok());
}
