//! `polis stop` — stop workspace, preserving all data.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::services::vm::lifecycle::{self as vm, VmState};

/// Run `polis stop`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be stopped.
pub async fn run(app: &AppContext) -> Result<ExitCode> {
    let mp = &app.provisioner;
    let ctx = &app.output;
    let state = vm::state(mp).await?;

    match state {
        VmState::NotFound => {
            ctx.info("No workspace to stop.");
            ctx.info("Create one: polis start");
        }
        VmState::Stopped => {
            ctx.info("Workspace is already stopped.");
            ctx.info("Resume: polis start");
        }
        VmState::Running | VmState::Starting => {
            ctx.info("Stopping workspace...");
            vm::stop(mp).await?;
            ctx.success("Workspace stopped. Your data is preserved.");
            ctx.info("Resume: polis start");
        }
    }

    Ok(ExitCode::SUCCESS)
}
