//! Logs command

use clap::Args;

/// Arguments for the logs command.
#[derive(Args)]
pub struct LogsArgs {
    /// Follow log output (like tail -f)
    #[arg(short, long)]
    pub follow: bool,

    /// Show only security events
    #[arg(long)]
    pub security: bool,
}
