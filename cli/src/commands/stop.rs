//! `polis stop` — stop workspace, preserving all data.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::services::workspace_stop::{StopOutcome, stop_workspace};

/// Run `polis stop`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be stopped.
pub async fn run(app: &AppContext) -> Result<ExitCode> {
    let ctx = &app.output;
    let reporter = app.terminal_reporter();

    match stop_workspace(&app.provisioner, &reporter).await {
        Ok(StopOutcome::NotFound) => {
            ctx.info("No workspace to stop.");
            ctx.info("Create one: polis start");
        }
        Ok(StopOutcome::AlreadyStopped) => {
            ctx.info("Workspace is already stopped.");
            ctx.info("Resume: polis start");
        }
        Ok(StopOutcome::Stopped) => {
            ctx.info("Your data is preserved.");
            ctx.info("Resume: polis start");
        }
        Err(e) => return Err(e),
    }

    Ok(ExitCode::SUCCESS)
}
