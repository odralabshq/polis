//! `polis agent remove` — remove an installed agent.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::App;
use crate::application::services::agent;

/// Run the `agent remove` subcommand.
///
/// # Errors
///
/// Returns an error if agent removal fails.
pub async fn run(app: &impl App, name: &str) -> Result<ExitCode> {
    app.output().info(&format!("Removing agent {name}..."));
    agent::remove_agent(
        app.provisioner(),
        app.state_store(),
        &app.terminal_reporter(),
        name,
    )
    .await?;
    app.output().success(&format!("Agent '{name}' removed"));
    Ok(ExitCode::SUCCESS)
}
