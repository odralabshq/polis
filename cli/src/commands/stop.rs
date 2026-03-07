//! `polis stop` — stop workspace, preserving all data.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::App;
use crate::application::services::workspace::stop;

/// Run `polis stop`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be stopped.
pub async fn run(app: &impl App) -> Result<ExitCode> {
    let reporter = app.terminal_reporter();
    let outcome = stop::stop(app.provisioner(), &reporter).await?;
    app.renderer().render_stop_outcome(&outcome)?;
    Ok(ExitCode::SUCCESS)
}
