//! `polis status` — show workspace status.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::App;
use crate::application::ports::ProgressReporter;
use crate::application::services::workspace::gather;

/// Run the status command.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub async fn run(app: &impl App) -> Result<ExitCode> {
    let reporter = app.terminal_reporter();
    reporter.begin_stage("gathering status...");

    let output = gather(app.provisioner()).await;

    reporter.complete_stage();

    app.renderer().render_status(&output)?;
    Ok(ExitCode::SUCCESS)
}
