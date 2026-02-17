//! Connect command

use clap::Args;

/// Arguments for the connect command.
#[derive(Args)]
pub struct ConnectArgs {
    /// Open in IDE (vscode, cursor)
    #[arg(long)]
    pub ide: Option<String>,
}
