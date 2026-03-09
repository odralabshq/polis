//! `polis agent` — manage AI agents.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

use crate::app::App;

mod activate;
mod install;
mod list;
mod remove;

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
pub async fn run(app: &impl App, cmd: AgentCommand) -> Result<ExitCode> {
    match cmd {
        AgentCommand::List => list::run(app).await,
        AgentCommand::Install { path } => install::run(app, &path).await,
        AgentCommand::Remove { name } => remove::run(app, &name).await,
        AgentCommand::Activate { name, envs } => activate::run(app, &name, envs).await,
    }
}
