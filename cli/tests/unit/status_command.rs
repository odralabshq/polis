//! Unit tests for `polis status` command.

#![allow(clippy::expect_used)]

use polis_cli::commands::status;
use polis_cli::output::OutputContext;

use crate::helpers::{VmNotFound, VmRunning, VmStopped};

#[tokio::test]
async fn test_status_no_workspace_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = status::run(&ctx, false, &VmNotFound).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_status_stopped_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = status::run(&ctx, false, &VmStopped).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_status_running_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = status::run(&ctx, false, &VmRunning).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_status_json_returns_ok() {
    let ctx = OutputContext::new(true, true);
    let result = status::run(&ctx, true, &VmNotFound).await;
    assert!(result.is_ok());
}
