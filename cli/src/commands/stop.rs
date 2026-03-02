//! `polis stop` — stop workspace, preserving all data.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::ports::ProgressReporter as _;
use crate::application::services::vm::lifecycle::{self as vm, VmState};
use crate::output::reporter::TerminalReporter;

/// Run `polis stop`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be stopped.
pub async fn run(app: &AppContext) -> Result<ExitCode> {
    let mp = &app.provisioner;
    let ctx = &app.output;
    let reporter = TerminalReporter::new(ctx);
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
            reporter.begin_stage("Stopping workspace...");
            match vm::stop(mp).await {
                Ok(()) => {
                    reporter.complete_stage();
                    ctx.info("Your data is preserved.");
                    ctx.info("Resume: polis start");
                }
                Err(e) => {
                    reporter.fail_stage();
                    return Err(e);
                }
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}
