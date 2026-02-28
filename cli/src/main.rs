//! Polis CLI - Secure workspaces for AI coding agents

#![cfg_attr(test, allow(clippy::expect_used))]

use clap::Parser;

mod app;
mod application;
mod assets;
mod cli;
mod command_runner;
mod commands;
mod domain;
mod infra;
mod output;
mod provisioner;
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
