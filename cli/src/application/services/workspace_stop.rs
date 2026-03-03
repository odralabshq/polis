//! Application service — workspace stop use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use anyhow::Result;

use crate::application::ports::{
    InstanceInspector, InstanceLifecycle, ProgressReporter, ShellExecutor,
};
use crate::application::services::vm::lifecycle::{self as vm, VmState};

/// Outcome of the `stop_workspace` use-case.
#[derive(Debug, PartialEq, Eq)]
pub enum StopOutcome {
    /// Workspace was stopped successfully.
    Stopped,
    /// Workspace was already stopped.
    AlreadyStopped,
    /// No workspace found.
    NotFound,
}

/// Stop the workspace.
///
/// # Errors
///
/// Returns an error if the stop command fails.
pub async fn stop_workspace(
    provisioner: &(impl InstanceInspector + InstanceLifecycle + ShellExecutor),
    reporter: &impl ProgressReporter,
) -> Result<StopOutcome> {
    match vm::state(provisioner).await? {
        VmState::NotFound => Ok(StopOutcome::NotFound),
        VmState::Stopped => Ok(StopOutcome::AlreadyStopped),
        VmState::Running | VmState::Starting => {
            reporter.begin_stage("Stopping workspace...");
            vm::stop(provisioner).await?;
            reporter.complete_stage();
            Ok(StopOutcome::Stopped)
        }
    }
}

/// Return the current VM running state (for use-cases that need to branch on it).
///
/// # Errors
///
/// Returns an error if the VM state cannot be determined.
pub async fn is_vm_running(provisioner: &impl InstanceInspector) -> Result<bool> {
    Ok(vm::state(provisioner).await? == VmState::Running)
}
