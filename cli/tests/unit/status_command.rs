//! Unit tests for `polis status` command.

#![allow(clippy::expect_used)]

use polis_cli::commands::status;
use polis_cli::output::OutputContext;

use crate::mocks::{MultipassVmNotFound, MultipassVmRunning, MultipassVmStopped};

fn ctx() -> OutputContext {
    OutputContext::new(true, true)
}

#[tokio::test]
async fn test_status_no_workspace_returns_ok() {
    assert!(status::run(&ctx(), false, &MultipassVmNotFound).await.is_ok());
}

#[tokio::test]
async fn test_status_stopped_returns_ok() {
    assert!(status::run(&ctx(), false, &MultipassVmStopped).await.is_ok());
}

#[tokio::test]
async fn test_status_running_returns_ok() {
    assert!(status::run(&ctx(), false, &MultipassVmRunning).await.is_ok());
}

#[tokio::test]
async fn test_status_json_no_workspace_returns_ok() {
    assert!(status::run(&ctx(), true, &MultipassVmNotFound).await.is_ok());
}

#[tokio::test]
async fn test_status_json_running_returns_ok() {
    assert!(status::run(&ctx(), true, &MultipassVmRunning).await.is_ok());
}
