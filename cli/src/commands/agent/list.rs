//! `polis agent list` — list installed agents.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::App;
use crate::application::services::agent;

/// Run the `agent list` subcommand.
///
/// # Errors
///
/// Returns an error if listing agents or rendering fails.
pub async fn run(app: &impl App) -> Result<ExitCode> {
    let agents = agent::list_agents(app.provisioner(), app.state_store()).await?;
    app.renderer().render_agent_list(&agents)?;
    Ok(ExitCode::SUCCESS)
}
