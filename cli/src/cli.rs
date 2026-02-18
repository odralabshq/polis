//! CLI argument parsing with clap derive

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands;

/// Secure workspaces for AI coding agents
#[derive(Parser)]
#[command(
    name = "polis",
    version,
    propagate_version = true,
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    /// Output in JSON format
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable colored output
    #[arg(long, global = true, env = "NO_COLOR")]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create workspace and start agent
    Run(commands::run::RunArgs),

    /// Start existing workspace
    Start,

    /// Stop workspace (preserves state)
    Stop,

    /// Remove workspace
    Delete(commands::DeleteArgs),

    /// Show workspace and agent status
    Status,

    /// Show agent activity
    Logs(commands::logs::LogsArgs),

    /// Enter workspace terminal
    Shell,

    /// Show/open connection options
    Connect(commands::connect::ConnectArgs),

    /// Manage agents
    #[command(subcommand)]
    Agents(commands::agents::AgentsCommand),

    /// Manage configuration
    #[command(subcommand)]
    Config(commands::config::ConfigCommand),

    /// Diagnose issues
    Doctor,

    /// Update Polis
    Update,

    /// Show version
    Version,

    #[command(hide = true, name = "_ssh-proxy")]
    SshProxy,

    #[command(hide = true, name = "_provision")]
    Provision,

    #[command(hide = true, name = "_extract-host-key")]
    ExtractHostKey,
}

impl Cli {
    /// Execute the CLI command.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails or is not yet implemented.
    pub fn run(self) -> Result<()> {
        match self.command {
            Command::Version => {
                commands::version::run(self.json);
                Ok(())
            }
            Command::Run(args) => commands::run::run(&args),
            _ => anyhow::bail!("Command not yet implemented"),
        }
    }
}
