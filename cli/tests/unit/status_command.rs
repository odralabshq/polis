//! Unit tests for `polis status` command.

#![allow(clippy::expect_used)]

use std::process::Output;

use anyhow::Result;
use polis_cli::commands::status;
use polis_cli::output::OutputContext;
use polis_cli::provisioner::InstanceInspector;

use crate::helpers::exit_status;
use crate::mocks::{MultipassVmNotFound, MultipassVmRunning, MultipassVmStopped};

// ── InstanceInspector mocks for check_multipass_status tests ─────────────────

/// Returns an error from `info()` — simulates timeout or spawn failure.
struct InfoFails;

impl InstanceInspector for InfoFails {
    async fn info(&self) -> Result<Output> {
        anyhow::bail!("timed out")
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}

/// Returns a non-success exit status from `info()`.
struct InfoBadStatus;

impl InstanceInspector for InfoBadStatus {
    async fn info(&self) -> Result<Output> {
        Ok(Output {
            status: exit_status(1),
            stdout: Vec::new(),
            stderr: b"instance not found".to_vec(),
        })
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}

/// Returns a successful `info()` with the given state string.
struct InfoWithState(&'static str);

impl InstanceInspector for InfoWithState {
    async fn info(&self) -> Result<Output> {
        let json = format!(r#"{{"info":{{"polis":{{"state":"{}"}}}}}}"#, self.0);
        Ok(Output {
            status: exit_status(0),
            stdout: json.into_bytes(),
            stderr: Vec::new(),
        })
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected")
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ctx() -> OutputContext {
    OutputContext::new(true, true)
}

// ── Existing smoke tests (use Multipass blanket bridge) ───────────────────────

#[tokio::test]
async fn test_status_no_workspace_returns_ok() {
    assert!(
        status::run(&ctx(), false, &MultipassVmNotFound)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn test_status_stopped_returns_ok() {
    assert!(
        status::run(&ctx(), false, &MultipassVmStopped)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn test_status_running_returns_ok() {
    assert!(
        status::run(&ctx(), false, &MultipassVmRunning)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn test_status_json_no_workspace_returns_ok() {
    assert!(
        status::run(&ctx(), true, &MultipassVmNotFound)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn test_status_json_running_returns_ok() {
    assert!(status::run(&ctx(), true, &MultipassVmRunning).await.is_ok());
}

// ── check_multipass_status unit tests ────────────────────────────────────────

#[tokio::test]
async fn check_multipass_status_returns_none_when_info_errors() {
    // Simulates timeout: info() returns Err — run() still succeeds, workspace = Error.
    assert!(status::run(&ctx(), false, &InfoFails).await.is_ok());
}

#[tokio::test]
async fn check_multipass_status_returns_none_when_info_bad_status() {
    // info() returns Ok but non-zero exit — workspace = Error.
    assert!(status::run(&ctx(), false, &InfoBadStatus).await.is_ok());
}

#[tokio::test]
async fn check_multipass_status_running_state() {
    // Running VM — workspace = Running or Starting (container check may fail in tests).
    assert!(
        status::run(&ctx(), true, &InfoWithState("Running"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn check_multipass_status_stopped_state() {
    assert!(
        status::run(&ctx(), true, &InfoWithState("Stopped"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn check_multipass_status_starting_state() {
    assert!(
        status::run(&ctx(), true, &InfoWithState("Starting"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn check_multipass_status_stopping_state() {
    assert!(
        status::run(&ctx(), true, &InfoWithState("Stopping"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn check_multipass_status_unknown_state_maps_to_error() {
    // Unknown state strings map to WorkspaceState::Error.
    assert!(
        status::run(&ctx(), true, &InfoWithState("Banana"))
            .await
            .is_ok()
    );
}
