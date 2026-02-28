//! `polis stop` — stop workspace, preserving all data.

use anyhow::Result;

use crate::application::ports::{InstanceInspector, InstanceLifecycle, ShellExecutor};
use crate::output::OutputContext;
use crate::workspace::vm;

/// Run `polis stop`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be stopped.
pub async fn run(
    ctx: &OutputContext,
    mp: &(impl InstanceLifecycle + InstanceInspector + ShellExecutor),
) -> Result<()> {
    let state = vm::state(mp).await?;

    match state {
        vm::VmState::NotFound => {
            ctx.info("No workspace to stop.");
            ctx.info("Create one: polis start");
        }
        vm::VmState::Stopped => {
            ctx.info("Workspace is already stopped.");
            ctx.info("Resume: polis start");
        }
        vm::VmState::Running | vm::VmState::Starting => {
            ctx.info("Stopping workspace...");
            vm::stop(mp).await?;
            ctx.success("Workspace stopped. Your data is preserved.");
            ctx.info("Resume: polis start");
        }
    }

    Ok(())
}
