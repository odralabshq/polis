//! `polis stop` — stop workspace, preserving all data.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::AppContext;
use crate::application::services::workspace_stop::stop_workspace;

/// Run `polis stop`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be stopped.
pub async fn run(app: &AppContext) -> Result<ExitCode> {
    let reporter = app.terminal_reporter();
    let outcome = stop_workspace(&app.provisioner, &reporter).await?;
    app.renderer().render_stop_outcome(&outcome)?;
    Ok(ExitCode::SUCCESS)
}
