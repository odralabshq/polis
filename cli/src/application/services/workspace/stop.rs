//! Application service — workspace stop use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use anyhow::Result;

use crate::application::ports::{
    InstanceInspector, InstanceLifecycle, ProgressReporter, ShellExecutor,
};
use crate::application::vm::lifecycle::{self as vm, VmState};

/// Outcome of the `stop` use-case.
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
pub async fn stop(
    provisioner: &(impl InstanceInspector + InstanceLifecycle + ShellExecutor),
    reporter: &impl ProgressReporter,
) -> Result<StopOutcome> {
    match vm::state(provisioner).await? {
        VmState::NotFound => Ok(StopOutcome::NotFound),
        VmState::Stopped => Ok(StopOutcome::AlreadyStopped),
        VmState::Running | VmState::Starting => {
            // Clear ready marker so polis.service won't auto-start on next boot.
            let _ = provisioner
                .exec(&["rm", "-f", crate::domain::workspace::READY_MARKER_PATH])
                .await;

            reporter.begin_stage("stopping workspace...");
            vm::stop(provisioner).await?;
            reporter.complete_stage();
            Ok(StopOutcome::Stopped)
        }
    }
}
