//! Run command

use clap::Args;

/// Arguments for the run command.
#[derive(Args)]
pub struct RunArgs {
    /// Agent to run (e.g., claude-dev, gpt-dev)
    pub agent: Option<String>,
}
