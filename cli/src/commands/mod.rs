//! Command implementations

pub mod agent;
pub mod config;
pub mod connect;
pub mod delete;
pub mod doctor;
pub mod internal;
pub mod start;
pub mod status;
pub mod stop;
pub mod update;
pub mod version;

use clap::Args;

/// Arguments for the delete command.
#[derive(Args)]
pub struct DeleteArgs {
    /// Remove everything including certificates, cache, and configuration
    #[arg(long)]
    pub all: bool,

    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,
}
