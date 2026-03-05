//! `polis agent` — manage AI agents.

use anyhow::Result;
use clap::Subcommand;
use owo_colors::OwoColorize as _;

use crate::app::AppContext;
use crate::application::services::agent_activate::{self, AgentActivateOptions, AgentOutcome};
use crate::application::services::agent_crud;
use crate::application::services::vm::lifecycle::{self as vm, VmState};
use crate::domain::error::WorkspaceError;

/// Agent subcommands.
#[derive(Subcommand)]
pub enum AgentCommand {
    /// List available agents
    List,
    /// Create a new agent from an image
    #[clap(hide = true)]
    Create {
        /// Agent name
        name: String,
        /// Base image (e.g. mcp/base)
        image: String,
    },
    /// Remove an agent
    Delete {
        /// Name of the agent to remove
        name: String,
    },
    /// Activate an agent on the running workspace
    Start {
        /// Agent name to activate
        name: String,
        /// Environment variables to pass to the agent (e.g. -e KEY=VAL)
        #[arg(short = 'e', long = "env")]
        envs: Vec<String>,
    },
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
        AgentCommand::Delete { name } => delete_agent(app, &name).await,
        AgentCommand::Start { name, envs } => start_agent(app, &name, envs).await,
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
async fn delete_agent(app: &AppContext, name: &str) -> Result<std::process::ExitCode> {
    app.output.info(&format!("Deleting agent {name}..."));
    agent_crud::remove_agent(
        &app.provisioner,
        &app.state_mgr,
        &app.terminal_reporter(),
        name,
    )
    .await?;
    app.output.success(&format!("Agent {name} deleted"));
    Ok(std::process::ExitCode::SUCCESS)
}

/// Activate an agent on the running workspace (Req 8.4, 2.3).
///
/// # Errors
///
/// Returns `WorkspaceError::NotRunning` if the workspace is not running.
/// Returns `WorkspaceError::AgentMismatch` if a different agent is already active.
async fn start_agent(
    app: &AppContext,
    name: &str,
    envs: Vec<String>,
) -> Result<std::process::ExitCode> {
    // Req 2.3 — check VM is Running before activating agent.
    let vm_state = vm::state(&app.provisioner).await?;
    if vm_state != VmState::Running {
        return Err(WorkspaceError::NotRunning.into());
    }

    let reporter = app.terminal_reporter();
    let opts = AgentActivateOptions {
        reporter: &reporter,
        agent_name: name,
        envs,
    };

    let outcome = agent_activate::activate_agent(
        &app.provisioner,
        &app.state_mgr,
        &app.local_fs,
        opts,
    )
    .await?;

    render_agent_outcome(&outcome, app);
    Ok(std::process::ExitCode::SUCCESS)
}

/// Render the result of agent activation to the terminal.
fn render_agent_outcome(outcome: &AgentOutcome, app: &AppContext) {
    if app.output.quiet {
        return;
    }
    match outcome {
        AgentOutcome::Installed { agent, onboarding } => {
            app.output.success(&format!("Agent '{agent}' activated"));
            render_onboarding(onboarding, app);
        }
        AgentOutcome::AlreadyInstalled { agent, onboarding } => {
            app.output
                .info(&format!("Agent '{agent}' is already active"));
            render_onboarding(onboarding, app);
        }
    }
}

fn render_onboarding(steps: &[polis_common::agent::OnboardingStep], app: &AppContext) {
    if steps.is_empty() {
        return;
    }
    app.output.blank();
    app.output.header("Getting started");
    for (i, step) in steps.iter().enumerate() {
        let cmd = step.command.style(app.output.styles.bold);
        app.output
            .info(&format!("{}. {}  {}", i + 1, step.title, cmd));
    }
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
