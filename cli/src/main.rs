//! Polis CLI - Secure workspaces for AI coding agents

#![cfg_attr(test, allow(clippy::expect_used))]

use clap::Parser;

mod cli;
mod commands;
mod output;
#[allow(dead_code)] // exists/remove used by tests and future commands (delete, connect)
mod ssh;
mod state;
mod workspace;

use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = cli.run().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
