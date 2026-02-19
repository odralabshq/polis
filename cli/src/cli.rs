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

    /// Download and verify the workspace VM image
    Init(commands::init::InitArgs),

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
    pub async fn run(self) -> Result<()> {
        let Cli {
            no_color,
            quiet,
            json,
            command,
        } = self;
        let no_color = no_color || std::env::var("NO_COLOR").is_ok();
        match command {
            Command::Version => commands::version::run(json),
            Command::Status => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                let mp = crate::multipass::MultipassCli;
                commands::status::run(&ctx, json, &mp).await
            }
            Command::Run(args) => {
                let mp = crate::multipass::MultipassCli;
                commands::run::run(&args, &mp)
            }
            Command::Start => {
                let state_mgr = crate::state::StateManager::new()?;
                let driver = crate::workspace::MultipassDriver;
                commands::start::run(&state_mgr, &driver)
            }
            Command::Stop => {
                let state_mgr = crate::state::StateManager::new()?;
                let driver = crate::workspace::MultipassDriver;
                commands::stop::run(&state_mgr, &driver)
            }
            Command::Delete(args) => {
                let state_mgr = crate::state::StateManager::new()?;
                let driver = crate::workspace::MultipassDriver;
                commands::delete::run(&args, &state_mgr, &driver)
            }
            Command::Connect(args) => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::connect::run(&ctx, args).await
            }
            Command::Agents(cmd) => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::agents::run(&ctx, cmd, json)
            }
            Command::Config(cmd) => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::config::run(&ctx, cmd)
            }
            Command::Update => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                let mp = crate::multipass::MultipassCli;
                commands::update::run(&ctx, &commands::update::GithubUpdateChecker, &mp).await
            }
            Command::Init(args) => commands::init::run(&args),
            Command::Doctor => {
                let ctx = crate::output::OutputContext::new(no_color, quiet);
                commands::doctor::run(&ctx, json).await
            }
            Command::SshProxy => commands::internal::ssh_proxy().await,
            Command::ExtractHostKey => commands::internal::extract_host_key().await,
            _ => anyhow::bail!("Command not yet implemented"),
        }
    }
}
