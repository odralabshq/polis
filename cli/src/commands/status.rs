//! `polis status` — show workspace status.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::AppContext;
use crate::application::ports::ProgressReporter;
use crate::application::services::workspace_status::gather_status;

/// Run the status command.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub async fn run(app: &AppContext) -> Result<ExitCode> {
    let reporter = app.terminal_reporter();
    reporter.begin_stage("gathering status...");

    let output = gather_status(app.provisioner()).await;

    reporter.complete_stage();

    app.renderer().render_status(&output)?;
    Ok(ExitCode::SUCCESS)
}
