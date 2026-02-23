//! Polis CLI - Secure workspaces for AI coding agents

#![cfg_attr(test, allow(clippy::expect_used))]

use clap::Parser;

mod cli;
mod commands;
mod multipass;
mod output;
mod ssh;
mod state;
mod workspace;

use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // REL-002: Handle Ctrl+C gracefully
    tokio::select! {
        result = cli.run() => {
            if let Err(e) = result {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted");
        }
    }
}
