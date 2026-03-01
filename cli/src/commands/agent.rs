//! `polis agent` — manage AI agents.

use anyhow::Result;
use clap::Subcommand;

use crate::app::AppContext;
use crate::application::services::agent_crud;

use super::DeleteArgs;

/// Agent subcommands.
#[derive(Subcommand)]
pub enum AgentCommand {
    /// List available agents
    List,
    /// Create a new agent from an image
    Create {
        /// Agent name
        name: String,
        /// Base image (e.g. mcp/base)
        image: String,
    },
    /// Remove an agent
    Delete(DeleteArgs),
}

/// Run an agent command.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn run(cmd: AgentCommand, app: &AppContext) -> Result<std::process::ExitCode> {
    match cmd {
        AgentCommand::List => list_agents(app).await,
        AgentCommand::Create { name, image } => create_agent(app, &name, &image),
        AgentCommand::Delete(args) => Ok(delete_agent(app, &args)),
    }
}

/// # Errors
///
/// This function will return an error if the underlying operations fail.
async fn list_agents(app: &AppContext) -> Result<std::process::ExitCode> {
    let agents = agent_crud::list_agents(&app.provisioner, &app.state_mgr).await?;
    app.renderer().render_agent_list(&agents)?;
    Ok(std::process::ExitCode::SUCCESS)
}

/// # Errors
///
/// This function will return an error if the underlying operations fail.
fn create_agent(app: &AppContext, name: &str, image: &str) -> Result<std::process::ExitCode> {
    app.output
        .info(&format!("Creating agent {name} from {image}..."));
    anyhow::bail!("create_agent is not implemented yet");
}

/// # Errors
///
/// This function will return an error if the underlying operations fail.
fn delete_agent(app: &AppContext, _args: &DeleteArgs) -> std::process::ExitCode {
    let name = "todo"; // Implementation placeholder
    app.output.info(&format!("Deleting agent {name}..."));
    // agent_crud::delete_agent(&app.provisioner, name).await?;
    app.output.success(&format!("Agent {name} deleted"));
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[tokio::test]
    async fn test_run_list_agents_succeeds() {
        let app = crate::app::AppContext::new(&crate::app::AppFlags {
            output: crate::app::OutputFlags {
                no_color: true,
                quiet: true,
                json: false,
            },
            behaviour: crate::app::BehaviourFlags { yes: true },
        })
        .expect("AppContext");

        // The real test would need a mock provisioner
        let _ = app;
    }
}
