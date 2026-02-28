//! CLI argument parsing with clap derive

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::app::AppContext;
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
#[allow(clippy::struct_excessive_bools)] // Clap CLI struct — bools map to flags, not state
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

    /// Skip interactive confirmation prompts (also set by `CI` or `POLIS_YES` env vars)
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

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
            yes,
            command,
        } = self;
        let no_color = no_color || std::env::var("NO_COLOR").is_ok();

        // Construct AppContext once at the top — passed as &AppContext to all handlers.
        let app = AppContext::new(&crate::app::AppFlags {
            output: crate::app::OutputFlags {
                no_color,
                quiet,
                json,
            },
            behaviour: crate::app::BehaviourFlags { yes },
        })?;

        match command {
            Command::Start(args) => commands::start::run(&args, &app).await,

            Command::Stop => commands::stop::run(&app).await,

            Command::Delete(args) => commands::delete::run(&args, &app).await,

            Command::Status => commands::status::run(&app, &app.provisioner).await,

            Command::Connect(args) => commands::connect::run(&app, args).await,

            Command::Config(cmd) => commands::config::run(&app, cmd, &app.provisioner).await,

            Command::Update(args) => {
                commands::update::run(&args, &app, &commands::update::GithubUpdateChecker).await
            }

            Command::Doctor { verbose, fix } => commands::doctor::run(&app, verbose, fix).await,

            Command::Exec(args) => commands::exec::run(&args, &app.provisioner).await,

            Command::Version => commands::version::run(&app),

            Command::Agent(cmd) => commands::agent::run(cmd, &app.provisioner, &app).await,

            // --- Internal commands ---
            #[allow(clippy::large_futures)]
            Command::SshProxy => commands::internal::ssh_proxy(&app.provisioner).await,
            Command::ExtractHostKey => commands::internal::extract_host_key(&app.provisioner).await,
            Command::Provision => {
                anyhow::bail!("Provision command is internal only")
            }
        }
    }
}
