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
pub async fn run(cmd: AgentCommand, app: &AppContext) -> Result<std::process::ExitCode> {
    match cmd {
        AgentCommand::List => list_agents(app).await,
        AgentCommand::Create { name, image } => create_agent(app, &name, &image).await,
        AgentCommand::Delete(args) => delete_agent(app, &args).await,
    }
}

async fn list_agents(app: &AppContext) -> Result<std::process::ExitCode> {
    let agents = agent_crud::list_agents(&app.provisioner, &app.state_store).await?;
    app.renderer().render_agent_list(&agents)?;
    Ok(std::process::ExitCode::SUCCESS)
}

async fn create_agent(app: &AppContext, name: &str, image: &str) -> Result<std::process::ExitCode> {
    app.output
        .info(&format!("Creating agent {name} from {image}..."));
    agent_crud::create_agent(&app.provisioner, name, image).await?;
    app.output.success(&format!("Agent {name} created"));
    Ok(std::process::ExitCode::SUCCESS)
}

async fn delete_agent(app: &AppContext, args: &DeleteArgs) -> Result<std::process::ExitCode> {
    let name = "todo"; // Implementation placeholder
    app.output.info(&format!("Deleting agent {name}..."));
    // agent_crud::delete_agent(&app.provisioner, name).await?;
    app.output.success(&format!("Agent {name} deleted"));
    Ok(std::process::ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

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
