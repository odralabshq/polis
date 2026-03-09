//! Agent remove service — remove an installed agent from the VM.
//!
//! Implements the Saga pattern with compensating actions for reliability.
//! Each step has a compensating action that restores consistency on failure.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::{Context, Result};

use crate::application::ports::{
    InstanceInspector, ProgressReporter, ShellExecutor, WorkspaceStateStore,
};
use crate::domain::error::RemoveError;
use crate::domain::workspace::{ACTIVE_OVERLAY_PATH, VM_ROOT};

use super::ensure_vm_running;

/// Remove an installed agent from the VM using the Saga pattern.
///
/// Each step has a compensating action that restores consistency on failure.
///
/// # Saga Steps
///
/// 1. Validate agent name and existence
/// 2. Check if agent is active
/// 3. If active: stop compose stack
/// 4. If active: remove overlay symlink (abort on failure)
/// 5. Remove agent directory
/// 6. Remove registry entry from agents.json
/// 7. If was active: restart base control plane
/// 8. If was active: clear `active_agent` state
///
/// # Compensating Actions
///
/// - If directory removal fails after compose stop: restart compose stack
/// - If control plane restart fails: return typed error with recovery command
///
/// # Errors
///
/// Returns typed `RemoveError` variants with recovery commands:
/// - `NotInstalled`: Agent directory doesn't exist
/// - `InvalidName`: Agent name fails validation
/// - `NotRunning`: VM is not running
/// - `SymlinkRemovalFailed`: Symlink removal failed (aborts operation)
/// - `StepFailed`: A step failed with recovery command
/// - `StepFailedWithCompensatingFailure`: Step and compensating action both failed
///
/// # Requirements
///
/// - 3.3: Separate service module for agent remove use case
/// - 5.1: Compensating action on directory removal failure
/// - 5.2: Report failure with recovery command on control plane restart failure
/// - 5.3: Typed error with step and recovery info
/// - 5.4: Persist state only after all VM operations complete
/// - 5.5: Typed error describing both failures when compensating action fails
/// - 5.6: Abort on symlink removal failure
/// - 12.1: Remove agent directory when not active
/// - 12.2: Stop compose stack when active
/// - 12.3: Restart base control plane after removal of active agent
/// - 12.4: Clear `active_agent` state after removal
/// - 12.5: Error when agent not installed
/// - 12.6: Error when name fails validation
/// - 12.7: Remove overlay symlink before directory removal
/// - 12.8: Error when VM not running
pub async fn remove_agent(
    provisioner: &(impl ShellExecutor + InstanceInspector),
    state_mgr: &impl WorkspaceStateStore,
    reporter: &impl ProgressReporter,
    agent_name: &str,
) -> Result<()> {
    // Step 1: Validate agent name (Req 12.6)
    if !crate::domain::agent::validate::is_valid_agent_name(agent_name) {
        return Err(RemoveError::InvalidName(agent_name.to_string()).into());
    }

    // Step 2: Ensure VM is running (Req 12.8)
    ensure_vm_running(provisioner)
        .await
        .map_err(|_| RemoveError::NotRunning)?;

    // Step 3: Check agent exists (Req 12.5)
    let agent_dir = format!("{VM_ROOT}/agents/{agent_name}");
    let exists = provisioner.exec(&["test", "-d", &agent_dir]).await?;
    if !exists.status.success() {
        return Err(RemoveError::NotInstalled {
            agent: agent_name.to_string(),
        }
        .into());
    }

    // Step 4: Check if agent is active
    let active = state_mgr.load_async().await?.and_then(|s| s.active_agent);
    let is_active = active.as_deref() == Some(agent_name);

    // Step 5-6: If active, stop compose and remove symlink (Req 12.2, 5.6, 12.7)
    if is_active {
        stop_active_agent(provisioner, reporter, agent_name).await?;
    }

    // Step 7: Remove agent directory (Req 12.1)
    remove_agent_directory(provisioner, reporter, agent_name, &agent_dir, is_active).await?;

    // Step 8: Remove registry entry from agents.json
    reporter.step("updating agent registry...");
    if let Err(e) = remove_from_registry(provisioner, agent_name).await {
        reporter.step(&format!(
            "warning: failed to update registry: {e}. Agent directory removed successfully."
        ));
    }

    // Step 9-10: If was active, restart control plane and clear state (Req 12.3, 12.4)
    if is_active {
        restart_control_plane_and_clear_state(provisioner, state_mgr, reporter, agent_name).await?;
    }

    reporter.success(&format!("agent '{agent_name}' removed"));
    Ok(())
}

/// Stop the active agent's compose stack and remove the overlay symlink.
///
/// Returns an error if stopping fails or symlink removal fails.
async fn stop_active_agent(
    provisioner: &impl ShellExecutor,
    reporter: &impl ProgressReporter,
    agent_name: &str,
) -> Result<()> {
    reporter.step(&format!("stopping active agent '{agent_name}'..."));
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let overlay = format!("{VM_ROOT}/agents/{agent_name}/.generated/compose.agent.yaml");

    let down = provisioner
        .exec(&["docker", "compose", "-f", &base, "-f", &overlay, "down"])
        .await?;

    if !down.status.success() {
        return Err(RemoveError::StepFailed {
            agent: agent_name.to_string(),
            step: "compose_stop".to_string(),
            recovery: format!(
                "docker compose -f {base} -f {overlay} down && polis agent remove {agent_name}"
            ),
        }
        .into());
    }

    // Remove overlay symlink (Req 5.6, 12.7) - failure aborts the operation
    reporter.step("removing overlay symlink...");
    let rm_symlink = provisioner.exec(&["rm", "-f", ACTIVE_OVERLAY_PATH]).await?;

    if !rm_symlink.status.success() {
        // Try to restart compose as compensating action first
        let restart = provisioner
            .exec(&["docker", "compose", "-f", &base, "-f", &overlay, "up", "-d"])
            .await;

        let restart_succeeded = restart.map(|o| o.status.success()).unwrap_or(false);

        if restart_succeeded {
            return Err(RemoveError::SymlinkRemovalFailed {
                agent: agent_name.to_string(),
                recovery: format!("rm -f {ACTIVE_OVERLAY_PATH} && polis agent remove {agent_name}"),
            }
            .into());
        }
        return Err(RemoveError::StepFailedWithCompensatingFailure {
            agent: agent_name.to_string(),
            step: "symlink_removal".to_string(),
            original: format!("Failed to remove overlay symlink at {ACTIVE_OVERLAY_PATH}"),
            compensating: "Failed to restart compose stack".to_string(),
            recovery: format!(
                "docker compose -f {base} -f {overlay} up -d && rm -f {ACTIVE_OVERLAY_PATH} && polis agent remove {agent_name}"
            ),
        }
        .into());
    }

    Ok(())
}

/// Remove the agent directory with compensating actions on failure.
async fn remove_agent_directory(
    provisioner: &impl ShellExecutor,
    reporter: &impl ProgressReporter,
    agent_name: &str,
    agent_dir: &str,
    is_active: bool,
) -> Result<()> {
    reporter.step(&format!("removing '{agent_name}'..."));
    let rm = provisioner.exec(&["rm", "-rf", agent_dir]).await?;

    if !rm.status.success() {
        if is_active {
            // Compensating action: restart compose stack (Req 5.1)
            reporter.step("directory removal failed, attempting to restart compose stack...");
            let base = format!("{VM_ROOT}/docker-compose.yml");
            let overlay = format!("{VM_ROOT}/agents/{agent_name}/.generated/compose.agent.yaml");

            // Re-create symlink first
            let _ = provisioner
                .exec(&["ln", "-sf", &overlay, ACTIVE_OVERLAY_PATH])
                .await;

            let restart = provisioner
                .exec(&["docker", "compose", "-f", &base, "-f", &overlay, "up", "-d"])
                .await;

            let restart_succeeded = restart.map(|o| o.status.success()).unwrap_or(false);

            if restart_succeeded {
                return Err(RemoveError::StepFailed {
                    agent: agent_name.to_string(),
                    step: "directory_removal".to_string(),
                    recovery: format!("polis agent remove {agent_name}"),
                }
                .into());
            }
            return Err(RemoveError::StepFailedWithCompensatingFailure {
                agent: agent_name.to_string(),
                step: "directory_removal".to_string(),
                original: format!("Failed to remove agent directory: {agent_dir}"),
                compensating: "Failed to restart compose stack".to_string(),
                recovery: format!(
                    "docker compose -f {base} -f {overlay} up -d && polis agent remove {agent_name}"
                ),
            }
            .into());
        }
        // Not active, just return error
        return Err(RemoveError::StepFailed {
            agent: agent_name.to_string(),
            step: "directory_removal".to_string(),
            recovery: format!("rm -rf {agent_dir} && polis agent remove {agent_name}"),
        }
        .into());
    }

    Ok(())
}

/// Restart the base control plane and clear the active agent state.
async fn restart_control_plane_and_clear_state(
    provisioner: &impl ShellExecutor,
    state_mgr: &impl WorkspaceStateStore,
    reporter: &impl ProgressReporter,
    agent_name: &str,
) -> Result<()> {
    reporter.step("restarting control plane...");
    let base = format!("{VM_ROOT}/docker-compose.yml");
    let up = provisioner
        .exec(&["docker", "compose", "-f", &base, "up", "-d"])
        .await?;

    if !up.status.success() {
        return Err(RemoveError::StepFailed {
            agent: agent_name.to_string(),
            step: "control_plane_restart".to_string(),
            recovery: format!("docker compose -f {base} up -d"),
        }
        .into());
    }

    // Clear active_agent state (Req 5.4, 12.4)
    if let Ok(Some(mut state)) = state_mgr.load_async().await {
        state.active_agent = None;
        state_mgr
            .save_async(&state)
            .await
            .context("clearing active_agent state")?;
    }

    Ok(())
}

/// Remove an agent entry from the agents.json registry on the VM.
///
/// Delegates to the shared registry module for read/write operations.
async fn remove_from_registry(provisioner: &impl ShellExecutor, agent_name: &str) -> Result<()> {
    let result = super::registry::read_registry(provisioner).await?;
    let mut entries = result.entries;
    let original_len = entries.len();
    entries.retain(|e| e.name != agent_name);

    // If nothing was removed, we're done
    if entries.len() == original_len {
        return Ok(());
    }

    super::registry::write_registry(provisioner, &entries).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_error_not_installed_formats_correctly() {
        let err = RemoveError::NotInstalled {
            agent: "test-agent".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("test-agent"));
        assert!(msg.contains("not installed"));
    }

    #[test]
    fn remove_error_invalid_name_formats_correctly() {
        let err = RemoveError::InvalidName("INVALID".to_string());
        let msg = err.to_string();
        assert!(msg.contains("INVALID"));
        assert!(msg.contains("Invalid"));
    }

    #[test]
    fn remove_error_step_failed_includes_recovery() {
        let err = RemoveError::StepFailed {
            agent: "test-agent".to_string(),
            step: "directory_removal".to_string(),
            recovery: "polis agent remove test-agent".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("directory_removal"));
        assert!(msg.contains("test-agent"));
        assert!(msg.contains("polis agent remove"));
    }

    #[test]
    fn remove_error_with_compensating_failure_includes_both() {
        let err = RemoveError::StepFailedWithCompensatingFailure {
            agent: "test-agent".to_string(),
            step: "directory_removal".to_string(),
            original: "Failed to remove directory".to_string(),
            compensating: "Failed to restart compose".to_string(),
            recovery: "manual recovery command".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("directory_removal"));
        assert!(msg.contains("Failed to remove directory"));
        assert!(msg.contains("Failed to restart compose"));
        assert!(msg.contains("manual recovery command"));
    }

    #[test]
    fn remove_error_symlink_failed_formats_correctly() {
        let err = RemoveError::SymlinkRemovalFailed {
            agent: "test-agent".to_string(),
            recovery: "rm -f /opt/polis/compose.active.yaml".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("test-agent"));
        assert!(msg.contains("symlink"));
        assert!(msg.contains("rm -f"));
    }

    // ── remove_agent service tests ────────────────────────────────────────

    use crate::application::ports::{InstanceInspector, ShellExecutor};
    use crate::application::vm::test_support::{
        NoopReporter, StateStoreStub, fail_output, impl_shell_executor_stubs, ok_output,
    };
    use anyhow::Result;
    use std::process::Output;

    /// Configurable stub: controls info (running/not), and per-command responses.
    struct RemoveStub {
        running: bool,
        agent_exists: bool,
        /// If true, `rm -rf` (directory removal) fails.
        rm_fails: bool,
    }

    impl InstanceInspector for RemoveStub {
        async fn info(&self) -> Result<Output> {
            if self.running {
                Ok(ok_output(
                    br#"{"info":{"polis":{"state":"Running","ipv4":[]}}}"#,
                ))
            } else {
                Ok(fail_output())
            }
        }
        async fn version(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }

    impl ShellExecutor for RemoveStub {
        async fn exec(&self, args: &[&str]) -> Result<Output> {
            match args.first().copied() {
                Some("test") => {
                    if self.agent_exists {
                        Ok(ok_output(b""))
                    } else {
                        Ok(fail_output())
                    }
                }
                Some("rm") if args.contains(&"-rf") => {
                    if self.rm_fails {
                        Ok(fail_output())
                    } else {
                        Ok(ok_output(b""))
                    }
                }
                _ => Ok(ok_output(b"[]")), // cat agents.json, docker compose, tee, etc.
            }
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            Ok(ok_output(b"")) // tee for registry write
        }
        impl_shell_executor_stubs!(exec_spawn, exec_status);
    }

    #[tokio::test]
    async fn remove_agent_invalid_name_returns_error() {
        let stub = RemoveStub {
            running: true,
            agent_exists: true,
            rm_fails: false,
        };
        let store = StateStoreStub::empty();
        let err = remove_agent(&stub, &store, &NoopReporter, "INVALID NAME")
            .await
            .expect_err("should fail for invalid name");
        assert!(err.to_string().contains("Invalid"));
    }

    #[tokio::test]
    async fn remove_agent_vm_not_running_returns_error() {
        let stub = RemoveStub {
            running: false,
            agent_exists: true,
            rm_fails: false,
        };
        let store = StateStoreStub::empty();
        assert!(
            remove_agent(&stub, &store, &NoopReporter, "openclaw")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn remove_agent_not_installed_returns_error() {
        let stub = RemoveStub {
            running: true,
            agent_exists: false,
            rm_fails: false,
        };
        let store = StateStoreStub::empty();
        let err = remove_agent(&stub, &store, &NoopReporter, "openclaw")
            .await
            .expect_err("should fail for not installed");
        assert!(err.to_string().contains("not installed"));
    }

    #[tokio::test]
    async fn remove_agent_inactive_agent_succeeds() {
        let stub = RemoveStub {
            running: true,
            agent_exists: true,
            rm_fails: false,
        };
        let store = StateStoreStub::empty();
        assert!(
            remove_agent(&stub, &store, &NoopReporter, "openclaw")
                .await
                .is_ok()
        );
    }
}
