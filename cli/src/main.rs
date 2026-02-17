//! Polis CLI - Secure workspaces for AI coding agents

use clap::Parser;

mod cli;
mod commands;
mod output;
#[allow(dead_code)] // Module will be used by status command
mod valkey;

use cli::Cli;

fn main() {
    let cli = Cli::parse();
    if let Err(e) = cli.run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
