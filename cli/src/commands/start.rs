//! `polis start` — start workspace (download and create if needed).

use anyhow::{Context, Result};
use clap::Args;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::services::workspace_start::{self as service, StartOutcome};
use crate::output::OutputContext;

/// Arguments for the start command.
#[derive(Args, Default)]
pub struct StartArgs {
    /// Agent to activate (must match agents/<name>/ directory inside the VM)
    #[arg(long)]
    pub agent: Option<String>,

    /// Environment variables to pass to the agent (e.g. -e KEY=VAL)
    #[arg(short = 'e', long = "env")]
    pub envs: Vec<String>,
}

/// # Errors
///
/// This function will return an error if the underlying operations fail.
/// Run `polis start`.
pub async fn run(args: &StartArgs, app: &AppContext) -> Result<ExitCode> {
    let (assets_dir, _assets_guard) = app.assets_dir().context("extracting assets")?;
    let version = env!("CARGO_PKG_VERSION");
    let reporter = app.terminal_reporter();

    let opts = crate::application::services::workspace_start::StartOptions {
        reporter: &reporter,
        agent: args.agent.as_deref(),
        envs: args.envs.clone(),
        assets_dir: &assets_dir,
        version,
    };
    let outcome = service::start_workspace(
        &app.provisioner,
        &app.state_mgr,
        &app.assets,
        &app.ssh,
        &app.local_fs,
        &app.local_fs,
        opts,
    )
    .await?;

    match outcome {
        StartOutcome::AlreadyRunning { agent } => {
            print_already_running_message(agent.as_deref(), &app.output);
        }
        StartOutcome::Created { agent } | StartOutcome::Restarted { agent } => {
            print_success_message(agent.as_deref(), &app.output);
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Print message when workspace is already running with matching config.
fn print_already_running_message(agent: Option<&str>, ctx: &OutputContext) {
    if ctx.quiet {
        return;
    }
    let label = agent.map_or_else(|| "workspace running".to_string(), |n| format!("workspace running · agent: {n}"));
    ctx.success(&label);
    println!();
    ctx.kv("Connect", "polis connect");
    ctx.kv("Status", "polis status");
}

/// Print success message after workspace is ready.
fn print_success_message(agent: Option<&str>, ctx: &OutputContext) {
    if ctx.quiet {
        return;
    }
    let label = agent.map_or_else(|| "workspace ready".to_string(), |n| format!("workspace ready · agent: {n}"));
    ctx.success(&label);
    println!();
    ctx.kv("Connect", "polis connect");
    ctx.kv("Status", "polis status");
}

#[cfg(test)]
mod tests {

    #[test]
    fn check_architecture_passes_on_non_arm64() {
        if std::env::consts::ARCH == "aarch64" {
            let err = crate::domain::workspace::check_architecture().expect_err("expected Err");
            let msg = err.to_string();
            assert!(msg.contains("amd64"), "error should mention amd64: {msg}");
        } else {
            assert!(
                crate::domain::workspace::check_architecture().is_ok(),
                "check_architecture() should succeed on non-arm64 host"
            );
        }
    }
}
