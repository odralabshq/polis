//! `polis start` — start workspace (download and create if needed).

use anyhow::{Context, Result};
use clap::Args;
use std::process::ExitCode;
use std::time::Duration;

use crate::app::AppContext;
use crate::application::services::ssh_provision::{self, SshProvisionOptions};
use crate::application::services::workspace_start::{self as service, StartOutcome};
use crate::output::OutputContext;
use owo_colors::OwoColorize as _;
/// Arguments for the start command.
#[derive(Args, Default)]
pub struct StartArgs {}

/// # Errors
///
/// This function will return an error if the underlying operations fail.
/// Run `polis start`.
pub async fn run(_args: &StartArgs, app: &AppContext) -> Result<ExitCode> {
    let (assets_dir, _assets_guard) = app.assets_dir().context("extracting assets")?;
    let version = env!("CARGO_PKG_VERSION");
    let reporter = app.terminal_reporter();
    app.output.info("Starting workspace.");

    // Read start timeout from env var in the presentation layer (Req 8.6, 10.5).
    let start_timeout = Duration::from_secs(
        std::env::var("POLIS_VM_START_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(180u64),
    );

    // Phase 1: Workspace lifecycle (Req 8.1).
    let opts = service::StartOptions {
        reporter: &reporter,
        assets_dir: &assets_dir,
        version,
        start_timeout,
    };
    let outcome = service::start_workspace(
        &app.provisioner,
        &app.state_mgr,
        &app.assets,
        // LocalFs implements both LocalFs and FileHasher — passed as `hasher`
        // because start_workspace only needs SHA256 file hashing from it.
        &app.local_fs,
        opts,
    )
    .await?;

    // Phase 2: SSH provisioning — consent decided here in presentation layer (Req 8.7, 10.4).
    let ssh_configured = app.ssh.is_configured()?;
    let consent = if ssh_configured {
        true
    } else {
        app.confirm("Add SSH configuration to ~/.ssh/config?", true)?
    };
    ssh_provision::provision_ssh(
        &app.provisioner,
        &app.ssh,
        SshProvisionOptions {
            consent_given: consent,
        },
        &reporter,
    )
    .await?;

    // Phase 3: Render outcome.
    match &outcome {
        StartOutcome::AlreadyRunning { active_agent } => {
            print_already_running_message(active_agent.as_deref(), &app.output);
        }
        StartOutcome::Created { .. } | StartOutcome::Restarted { .. } => {
            render_onboarding_steps(&app.output, &[]);
        }
    }

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
        let cmd = step.command.style(ctx.styles.bold);
        ctx.info(&format!("{}. {}  {}", i + 1, step.title, cmd));
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
