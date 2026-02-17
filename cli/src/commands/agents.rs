//! Agents command

use clap::Subcommand;

/// Agents subcommands.
#[derive(Subcommand)]
pub enum AgentsCommand {
    /// List available agents
    List,
    /// Show agent details
    Info {
        /// Agent name
        name: String,
    },
    /// Add custom agent
    Add {
        /// Path to agent directory
        path: String,
    },
}
