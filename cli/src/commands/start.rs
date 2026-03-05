//! `polis start` — start workspace (download and create if needed).

use anyhow::{Context, Result};
use clap::Args;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::ports::InstanceInspector;
use crate::application::services::vm::lifecycle as vm;
use crate::application::services::workspace_start::{self as service, StartOutcome};
use crate::output::OutputContext;
use owo_colors::OwoColorize as _;

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
    if args.agent.is_some() {
        app.output
            .info("Starting workspace. Agent initialization may take several minutes depending on the selected agent.");
    } else {
        app.output.info("Starting workspace.");
    }

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
        StartOutcome::AlreadyRunning { agent, .. } => {
            print_already_running_message(agent.as_deref(), &app.output);
        }
        StartOutcome::Created {
            agent, onboarding, ..
        }
        | StartOutcome::Restarted {
            agent, onboarding, ..
        } => {
            render_onboarding_steps(&app.output, agent.as_deref(), &onboarding);
        }
    }

    // Show dashboard URL if an agent is active
    show_dashboard_url(&app.output, &app.provisioner).await;

    Ok(ExitCode::SUCCESS)
}

/// Print message when workspace is already running with matching config.
fn print_already_running_message(agent: Option<&str>, ctx: &OutputContext) {
    if ctx.quiet {
        return;
    }
    let label = agent.map_or_else(
        || "workspace running".to_string(),
        |n| format!("workspace running with agent: {n}"),
    );
    ctx.success(&label);
    ctx.blank();
    ctx.kv("Connect", "polis connect");
    ctx.kv("Status", "polis status");
}

fn render_onboarding_steps(
    ctx: &OutputContext,
    agent: Option<&str>,
    agent_steps: &[polis_common::agent::OnboardingStep],
) {
    if ctx.quiet {
        return;
    }

    let default_steps = [
        polis_common::agent::OnboardingStep {
            title: "Set up SSH keys".into(),
            command: "polis connect".into(),
        },
        polis_common::agent::OnboardingStep {
            title: "Connect to workspace".into(),
            command: "ssh workspace".into(),
        },
    ];

    ctx.blank();
    ctx.header("Getting started");
    for (i, step) in default_steps.iter().chain(agent_steps.iter()).enumerate() {
        // Agent-specific commands are written for use inside the workspace.
        // When displayed on the host CLI, prefix with `polis exec <agent>`
        // so the user can run them directly.
        let cmd_str = if i >= default_steps.len() {
            if let Some(name) = agent {
                format!("polis exec {name} {}", step.command)
            } else {
                step.command.clone()
            }
        } else {
            step.command.clone()
        };
        let cmd = cmd_str.style(ctx.styles.bold);
        ctx.info(&format!("{}. {}  {}", i + 1, step.title, cmd));
    }
}

/// Resolve the VM IP and show the dashboard URL (best-effort, no error on failure).
async fn show_dashboard_url(ctx: &OutputContext, mp: &impl InstanceInspector) {
    if ctx.quiet {
        return;
    }
    if let Ok(ip) = vm::resolve_vm_ip(mp).await {
        ctx.blank();
        ctx.kv("Control UI", &format!("http://{ip}:18789/overview"));
    }
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
