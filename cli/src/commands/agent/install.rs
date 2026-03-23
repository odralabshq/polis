//! `polis agent install` — install an agent from a local path.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::App;
use crate::application::services::agent;

/// Run the `agent install` subcommand.
///
/// # Errors
///
/// Returns an error if agent installation fails.
pub async fn run(app: &impl App, path: &str) -> Result<ExitCode> {
    let name =
        agent::install_agent(app.provisioner(), app.fs(), &app.terminal_reporter(), path).await?;
    app.output().success(&format!("Agent '{name}' installed"));
    Ok(ExitCode::SUCCESS)
}
