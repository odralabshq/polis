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
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start workspace
    Start(commands::start::StartArgs),

    /// Stop workspace
    Stop,

    /// Remove workspace
    Delete(commands::DeleteArgs),

    /// Show workspace status
    Status,

    /// Show connection options
    Connect(commands::connect::ConnectArgs),

    /// Manage configuration
    #[command(subcommand)]
    Config(commands::config::ConfigCommand),

    /// Diagnose issues
    Doctor {
        /// Show remediation details for each issue
        #[arg(long)]
        verbose: bool,
        /// Attempt to automatically repair detected issues
        #[arg(long)]
        fix: bool,
    },

    /// Run a command in the workspace
    Exec(commands::exec::ExecArgs),

    /// Update Polis
    Update(commands::update::UpdateArgs),

    /// Manage agents
    #[command(subcommand)]
    Agent(commands::agent::AgentCommand),

    /// Show version
    Version,

    // --- Internal ---
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
    pub async fn run(self) -> Result<()> {
        let Cli {
            no_color,
            quiet,
            json,
            command,
        } = self;
        let no_color = no_color || std::env::var("NO_COLOR").is_ok();
        let mp = crate::multipass::MultipassCli;

        match command {
            Command::Start(args) => commands::start::run(&args, &mp, quiet).await,

            Command::Stop => commands::stop::run(&mp, quiet).await,

            Command::Delete(args) => {
                let state_mgr = crate::state::StateManager::new()?;
                commands::delete::run(&args, &mp, &state_mgr, quiet).await
            }

            Command::Status => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::status::run(&ctx, json, &mp).await
            }

            Command::Connect(args) => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::connect::run(&ctx, args, &mp).await
            }

            Command::Config(cmd) => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::config::run(&ctx, cmd, json, &mp).await
            }

            Command::Update(args) => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::update::run(&args, &ctx, &commands::update::GithubUpdateChecker, &mp)
                    .await
            }

            Command::Doctor { verbose, fix } => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::doctor::run(&ctx, json, verbose, fix, &mp).await
            }

            Command::Exec(args) => commands::exec::run(&args, &mp).await,

            Command::Version => commands::version::run(json),

            Command::Agent(cmd) => {
                let mp = crate::multipass::MultipassCli;
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::agent::run(cmd, &mp, &ctx, json).await
            }

            // --- Internal commands ---
            #[allow(clippy::large_futures)]
            Command::SshProxy => commands::internal::ssh_proxy(&mp).await,
            Command::ExtractHostKey => commands::internal::extract_host_key(&mp).await,
            Command::Provision => {
                anyhow::bail!("Provision command is internal only")
            }
        }
    }
}
