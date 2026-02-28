//! Polis CLI - Secure workspaces for AI coding agents

#![cfg_attr(test, allow(clippy::expect_used))]

use clap::Parser;

mod app;
mod application;
mod cli;
mod commands;
mod domain;
mod infra;
mod output;

use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // REL-002: Handle Ctrl+C gracefully
    tokio::select! {
        result = cli.run() => {
            match result {
                Ok(code) => std::process::exit(match code {
                    v if v == std::process::ExitCode::SUCCESS => 0,
                    _ => 1, // ExitCode doesn't expose its value easily, but we can map SUCCESS to 0
                }),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted");
            std::process::exit(130);
        }
    }
}
