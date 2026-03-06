//! `polis agent` — manage AI agents.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

use crate::app::AppContext;
use crate::application::services::agent::{
    self, ActivateOutcome, AgentActivateOptions, AgentOutcome, AgentSwapOptions,
};

/// Agent subcommands.
#[derive(Subcommand)]
pub enum AgentCommand {
    /// List installed agents
    List,
    /// Install an agent from a local path
    Install {
        #[arg(long)]
        path: String,
    },
    /// Remove an installed agent
    Remove { name: String },
    /// Activate an agent on the running workspace
    Activate {
        name: String,
        #[arg(short = 'e', long = "env")]
        envs: Vec<String>,
    },
}

/// Run an agent command.
///
/// # Errors
///
/// Returns an error if the agent operation fails.
pub async fn run(cmd: AgentCommand, app: &AppContext) -> Result<ExitCode> {
    match cmd {
        AgentCommand::List => {
            let agents = agent::list_agents(app.provisioner(), app.state_store()).await?;
            app.renderer().render_agent_list(&agents)?;
        }
        AgentCommand::Install { path } => {
            let name = agent::install_agent(
                app.provisioner(),
                app.local_fs(),
                &app.terminal_reporter(),
                &path,
            )
            .await?;
            app.output.success(&format!("Agent '{name}' installed"));
        }
        AgentCommand::Remove { name } => {
            app.output.info(&format!("Removing agent {name}..."));
            agent::remove_agent(
                app.provisioner(),
                app.state_store(),
                &app.terminal_reporter(),
                &name,
            )
            .await?;
            app.output.success(&format!("Agent '{name}' removed"));
        }
        AgentCommand::Activate { name, envs } => return activate_agent(app, &name, envs).await,
    }
    Ok(ExitCode::SUCCESS)
}

async fn activate_agent(app: &AppContext, name: &str, envs: Vec<String>) -> Result<ExitCode> {
    let reporter = app.terminal_reporter();
    let opts = AgentActivateOptions {
        reporter: &reporter,
        agent_name: name,
        envs: envs.clone(),
    };
    let outcome =
        agent::activate_agent(app.provisioner(), app.state_store(), app.local_fs(), opts).await?;

    if let ActivateOutcome::SwapRequired { active, requested } = outcome {
        let prompt = format!("Agent '{active}' is active. Swap to '{requested}'?");
        if !app.confirm(&prompt, true)? {
            app.output.info("Swap cancelled.");
            return Ok(ExitCode::SUCCESS);
        }
        let swap_opts = AgentSwapOptions {
            reporter: &reporter,
            active_name: &active,
            new_name: &requested,
            envs,
        };
        let swap_outcome = agent::swap_agent(
            app.provisioner(),
            app.state_store(),
            app.local_fs(),
            swap_opts,
        )
        .await?;
        render_outcome(swap_outcome, app);
    } else {
        render_outcome(outcome, app);
    }
    Ok(ExitCode::SUCCESS)
}

fn render_outcome(outcome: ActivateOutcome, app: &AppContext) {
    let (o, unhealthy) = match outcome {
        ActivateOutcome::Activated(o) | ActivateOutcome::AlreadyActive(o) => (o, false),
        ActivateOutcome::ActivatedUnhealthy(o) => (o, true),
        ActivateOutcome::SwapRequired { .. } => unreachable!(),
    };
    if unhealthy {
        app.output
            .warn("Agent activated but health check timed out — it may not be ready yet.");
    }
    let renderer = app.renderer();
    match o {
        AgentOutcome::Activated { agent, onboarding } => {
            renderer.render_agent_activated(&agent, false);
            renderer.render_onboarding(&onboarding);
        }
        AgentOutcome::AlreadyActive { agent, onboarding } => {
            renderer.render_agent_activated(&agent, true);
            renderer.render_onboarding(&onboarding);
        }
    }
}
