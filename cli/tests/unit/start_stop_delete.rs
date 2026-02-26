//! Unit tests for `polis start`, `polis stop`, and `polis delete [--all]`.

#![allow(clippy::expect_used)]

use polis_cli::commands::delete;
use polis_cli::commands::start::{self, StartArgs};
use polis_cli::commands::{DeleteArgs, stop};
use polis_cli::state::StateManager;

use crate::mocks::{MultipassVmNotFound, MultipassVmRunning, MultipassVmStopped};

fn isolated_state() -> (tempfile::TempDir, StateManager) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let mgr = StateManager::with_path(dir.path().join("state.json"));
    (dir, mgr)
}

// ── polis stop ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_stop_no_workspace_succeeds() {
    assert!(stop::run(&MultipassVmNotFound, true).await.is_ok());
}

#[tokio::test]
async fn test_stop_already_stopped_succeeds() {
    assert!(stop::run(&MultipassVmStopped, true).await.is_ok());
}

// ── polis delete ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_delete_no_workspace_succeeds() {
    let (_dir, state_mgr) = isolated_state();
    let args = DeleteArgs {
        all: false,
        yes: true,
    };
    assert!(
        delete::run(&args, &MultipassVmNotFound, &state_mgr, true)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn test_delete_all_no_workspace_succeeds() {
    let (_dir, state_mgr) = isolated_state();
    let args = DeleteArgs {
        all: true,
        yes: true,
    };
    assert!(
        delete::run(&args, &MultipassVmNotFound, &state_mgr, true)
            .await
            .is_ok()
    );
}

// ── polis start ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_start_already_running_returns_ok() {
    let args = StartArgs {
        agent: None,
        dev: false,
    };
    assert!(start::run(&args, &MultipassVmRunning, true).await.is_ok());
}

#[tokio::test]
async fn test_start_dev_mode_skips_pull_and_returns_ok() {
    // --dev skips pull/verify/compose/health; VM already running → returns Ok immediately.
    let args = StartArgs {
        agent: None,
        dev: true,
    };
    assert!(start::run(&args, &MultipassVmRunning, true).await.is_ok());
}
