//! Command implementations

pub mod agents;
pub mod init;
pub mod config;
pub mod connect;
pub mod delete;
pub mod doctor;
pub mod internal;
pub mod logs;
pub mod run;
pub mod start;
pub mod status;
pub mod stop;
pub mod update;
pub mod version;

use clap::Args;

/// Arguments for the delete command.
#[derive(Args)]
pub struct DeleteArgs {
    /// Remove cached images (~3.5 GB)
    #[arg(long)]
    pub all: bool,
}
