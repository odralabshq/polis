//! Workspace domain types and pure validation functions.
//!
//! This module is intentionally free of I/O, async, and external layer imports.
//! All functions take data in and return data out.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// VM state as observed from the provisioner.
///
/// Defined in the domain layer so that pure functions like `resolve_action`
/// can operate on it without importing from the application layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    NotFound,
    Stopped,
    Starting,
    Running,
}

/// The action to take when starting a workspace, determined by `resolve_action`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartAction {
    /// VM does not exist — run full provisioning workflow.
    Create,
    /// VM is stopped — start it and restore services.
    Restart,
    /// Incomplete provisioning detected — resume from checkpoint.
    ResumeProvisioning,
    /// VM is starting — poll with bounded timeout, then re-evaluate.
    WaitThenResolve,
    /// VM is already running with no incomplete provisioning — no-op.
    AlreadyRunning,
}

/// Pure domain function mapping `(VmState, has_incomplete_provisioning)` to a `StartAction`.
///
/// # Transition table
///
/// | `VmState`  | `has_incomplete_provisioning` | Action              |
/// |----------|-----------------------------|---------------------|
/// | Any      | `true`                      | `ResumeProvisioning`  |
/// | `NotFound` | `false`                     | Create              |
/// | Stopped  | `false`                     | Restart             |
/// | Starting | `false`                     | `WaitThenResolve`     |
/// | Running  | `false`                     | `AlreadyRunning`      |
///
/// No I/O, no async, no side effects.
#[must_use]
pub fn resolve_action(vm_state: VmState, has_incomplete_provisioning: bool) -> StartAction {
    if has_incomplete_provisioning {
        return StartAction::ResumeProvisioning;
    }
    match vm_state {
        VmState::NotFound => StartAction::Create,
        VmState::Stopped => StartAction::Restart,
        VmState::Starting => StartAction::WaitThenResolve,
        VmState::Running => StartAction::AlreadyRunning,
    }
}

/// Checkpoint persisted during a provisioning workflow so that an interrupted
/// run can be resumed from the last completed step rather than restarting from
/// scratch.
///
/// Serialized as part of [`WorkspaceState`] under the `"provisioning"` key.
/// Absent from older state files — deserializes to `None` via `#[serde(default)]`
/// on the containing field.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvisioningCheckpoint {
    /// Identifier of the provisioning workflow (e.g. `"create"`, `"resume"`).
    pub workflow_id: String,
    /// IDs of steps that have already completed successfully.
    pub completed_steps: Vec<String>,
    /// ID of the step that failed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<String>,
}

impl ProvisioningCheckpoint {
    /// Returns `true` if `step_id` is in `completed_steps`.
    #[must_use]
    pub fn is_step_done(&self, step_id: &str) -> bool {
        self.completed_steps.iter().any(|s| s == step_id)
    }
}

/// Workspace state persisted to `~/.polis/state.json`.
///
/// The `created_at` field accepts the legacy `started_at` name for backward
/// compatibility with older state files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
    /// When workspace was created (accepts legacy `"started_at"` field).
    #[serde(alias = "started_at")]
    pub created_at: DateTime<Utc>,
    /// Image SHA256 used to create workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_sha256: Option<String>,
    /// Custom image source (path or URL) used to create workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_source: Option<String>,
    /// Currently active agent name, or None for control-plane-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_agent: Option<String>,
    /// In-progress provisioning checkpoint, or `None` when provisioning is
    /// complete.  Absent from older state files — deserializes to `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioning: Option<ProvisioningCheckpoint>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            created_at: chrono::Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: None,
            provisioning: None,
        }
    }
}

/// Check that the host architecture is amd64.
///
/// Sysbox (the container runtime used by Polis) does not support arm64 as of v0.6.7.
///
/// # Errors
///
/// Returns an error if the host is arm64 / aarch64.
#[allow(dead_code)] // Called from workspace_start service — not yet wired to binary
pub fn check_architecture() -> Result<()> {
    if std::env::consts::ARCH == "aarch64" {
        anyhow::bail!(
            "Polis requires an amd64 host. \
Sysbox (the container runtime used by Polis) does not support arm64 as of v0.6.7. \
Please use an amd64 machine."
        );
    }
    Ok(())
}

/// Path to `docker-compose.yml` inside the VM.
/// MAINT-001: Centralized constant used by status, update, vm, and health modules.
pub const COMPOSE_PATH: &str = "/opt/polis/docker-compose.yml";

/// Docker container name inside the VM.
/// MAINT-002: Centralized constant for container references.
pub const CONTAINER_NAME: &str = "polis-workspace";

/// Path to the polis project root inside the VM.
pub const VM_ROOT: &str = "/opt/polis";

/// Path to active compose overlay symlink inside VM.
pub const ACTIVE_OVERLAY_PATH: &str = "/opt/polis/compose.active.yaml";

/// Path to ready marker file inside VM.
/// When present, `polis.service` is allowed to auto-start.
/// CLI removes this before controlled restarts.
pub const READY_MARKER_PATH: &str = "/opt/polis/.ready";

/// Path to the guest query script inside the VM.
/// Used by status and doctor services to gather system info via a single exec call,
/// avoiding Multipass Windows pipe/buffer issues with piped commands.
pub const QUERY_SCRIPT: &str = "/opt/polis/scripts/polis-query.sh";

/// User name inside the workspace container.
/// MAINT-003: Centralized constant for container user identity.
pub const CONTAINER_USER: &str = "polis";

/// UID of the container user (matches /etc/passwd in container image).
/// MAINT-004: Centralized constant for container user UID.
pub const CONTAINER_USER_UID: u32 = 1000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_architecture_passes_on_non_arm64() {
        if std::env::consts::ARCH == "aarch64" {
            let err = check_architecture().expect_err("expected Err on arm64");
            let msg = err.to_string();
            assert!(msg.contains("amd64"), "error should mention amd64: {msg}");
        } else {
            assert!(
                check_architecture().is_ok(),
                "check_architecture() should succeed on non-arm64 host"
            );
        }
    }

    // -----------------------------------------------------------------------
    // ProvisioningCheckpoint tests
    // -----------------------------------------------------------------------

    #[test]
    fn is_step_done_returns_true_for_present_step() {
        let cp = ProvisioningCheckpoint {
            workflow_id: "create".into(),
            completed_steps: vec!["step-a".into(), "step-b".into()],
            failed_step: None,
        };
        assert!(cp.is_step_done("step-a"));
        assert!(cp.is_step_done("step-b"));
    }

    #[test]
    fn is_step_done_returns_false_for_absent_step() {
        let cp = ProvisioningCheckpoint {
            workflow_id: "create".into(),
            completed_steps: vec!["step-a".into()],
            failed_step: None,
        };
        assert!(!cp.is_step_done("step-b"));
        assert!(!cp.is_step_done(""));
    }

    #[test]
    fn is_step_done_returns_false_when_no_steps_completed() {
        let cp = ProvisioningCheckpoint::default();
        assert!(!cp.is_step_done("any-step"));
    }

    #[test]
    fn workspace_state_without_provisioning_deserializes_to_none() {
        let json = r#"{"created_at":"2024-01-01T00:00:00Z"}"#;
        let state: WorkspaceState = serde_json::from_str(json).expect("deserialize");
        assert!(state.provisioning.is_none());
    }

    #[test]
    fn workspace_state_with_provisioning_round_trips() {
        let state = WorkspaceState {
            created_at: chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                .expect("valid RFC3339 timestamp")
                .with_timezone(&Utc),
            image_sha256: None,
            image_source: None,
            active_agent: None,
            provisioning: Some(ProvisioningCheckpoint {
                workflow_id: "create".into(),
                completed_steps: vec!["step-a".into()],
                failed_step: Some("step-b".into()),
            }),
        };
        let json = serde_json::to_string(&state).expect("serialize");
        let restored: WorkspaceState = serde_json::from_str(&json).expect("deserialize");
        let cp = restored.provisioning.expect("provisioning should be Some");
        assert_eq!(cp.workflow_id, "create");
        assert_eq!(cp.completed_steps, vec!["step-a"]);
        assert_eq!(cp.failed_step.as_deref(), Some("step-b"));
    }

    #[test]
    fn provisioning_checkpoint_not_serialized_when_none() {
        let state = WorkspaceState {
            created_at: chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                .expect("valid RFC3339 timestamp")
                .with_timezone(&Utc),
            image_sha256: None,
            image_source: None,
            active_agent: None,
            provisioning: None,
        };
        let json = serde_json::to_string(&state).expect("serialize");
        assert!(
            !json.contains("provisioning"),
            "field should be omitted: {json}"
        );
    }

    // -----------------------------------------------------------------------
    // resolve_action unit tests — Requirements 1.3, 1.4, 1.5, 1.6, 1.7
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_action_not_found_no_checkpoint_returns_create() {
        assert_eq!(
            resolve_action(VmState::NotFound, false),
            StartAction::Create
        );
    }

    #[test]
    fn resolve_action_stopped_no_checkpoint_returns_restart() {
        assert_eq!(
            resolve_action(VmState::Stopped, false),
            StartAction::Restart
        );
    }

    #[test]
    fn resolve_action_starting_no_checkpoint_returns_wait_then_resolve() {
        assert_eq!(
            resolve_action(VmState::Starting, false),
            StartAction::WaitThenResolve
        );
    }

    #[test]
    fn resolve_action_running_no_checkpoint_returns_already_running() {
        assert_eq!(
            resolve_action(VmState::Running, false),
            StartAction::AlreadyRunning
        );
    }

    #[test]
    fn resolve_action_with_checkpoint_always_returns_resume_provisioning() {
        // Checkpoint priority overrides every VmState variant (Requirement 1.7)
        for state in [
            VmState::NotFound,
            VmState::Stopped,
            VmState::Starting,
            VmState::Running,
        ] {
            assert_eq!(
                resolve_action(state, true),
                StartAction::ResumeProvisioning,
                "expected ResumeProvisioning for {state:?} with has_incomplete_provisioning=true"
            );
        }
    }
}
