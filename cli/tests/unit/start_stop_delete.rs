//! Unit tests for start/stop/delete lifecycle behaviour.
//!
//! Tests exercise the vm lifecycle service layer directly with mocks,
//! rather than going through command handlers that own their provisioner.

#![allow(clippy::expect_used)]

use polis_cli::application::services::vm::lifecycle::{self as vm, VmState};

use crate::helpers::{VmNotFound, VmRunning, VmStopped};

// ── vm::state ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn vm_state_not_found_when_instance_missing() {
    let state = vm::state(&VmNotFound).await.expect("state");
    assert_eq!(state, VmState::NotFound);
}

#[tokio::test]
async fn vm_state_stopped_when_instance_stopped() {
    let state = vm::state(&VmStopped).await.expect("state");
    assert_eq!(state, VmState::Stopped);
}

#[tokio::test]
async fn vm_state_running_when_instance_running() {
    let state = vm::state(&VmRunning).await.expect("state");
    assert_eq!(state, VmState::Running);
}

// ── vm::exists ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn vm_exists_false_when_not_found() {
    assert!(!vm::exists(&VmNotFound).await);
}

#[tokio::test]
async fn vm_exists_true_when_stopped() {
    assert!(vm::exists(&VmStopped).await);
}

#[tokio::test]
async fn vm_exists_true_when_running() {
    assert!(vm::exists(&VmRunning).await);
}
