//! Polis CLI - Secure workspaces for AI coding agents

use clap::Parser;

mod cli;
mod commands;
mod output;
mod state;
#[allow(dead_code)] // Module will be used by status command
mod valkey;
mod workspace;

use cli::Cli;

fn main() {
    let cli = Cli::parse();
    if let Err(e) = cli.run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
