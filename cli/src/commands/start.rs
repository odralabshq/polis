//! `polis start` — start workspace (download and create if needed).

use anyhow::{Context, Result};
use clap::Args;

use crate::app::AppContext;
use crate::application::services::workspace_start::{self as service, StartOutcome};
use crate::output::OutputContext;

/// Arguments for the start command.
#[derive(Args, Default)]
pub struct StartArgs {
    /// Agent to activate (must match agents/<name>/ directory inside the VM)
    #[arg(long)]
    pub agent: Option<String>,
}

/// Run `polis start`.
///
/// # Errors
///
/// Returns an error if image acquisition, VM creation, or health check fails.
pub async fn run(args: &StartArgs, app: &AppContext) -> Result<()> {
    let (assets_dir, _assets_guard) = app.assets_dir().context("extracting assets")?;
    let version = env!("CARGO_PKG_VERSION");
    let reporter = app.terminal_reporter();

    let outcome = service::start_workspace(
        &app.provisioner,
        &app.state_mgr,
        &app.assets,
        &app.ssh,
        &crate::infra::fs::LocalFs,
        &reporter,
        args.agent.as_deref(),
        &assets_dir,
        version,
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

    Ok(())
}

/// Print message when workspace is already running with matching config.
fn print_already_running_message(agent: Option<&str>, ctx: &OutputContext) {
    if ctx.quiet {
        return;
    }
    ctx.info("Workspace is running.");
    if let Some(name) = agent {
        ctx.kv("Agent", name);
    }
    print_guarantees(ctx);
    ctx.kv("Connect", "polis connect");
    ctx.kv("Status", "polis status");
}

/// Print success message after workspace is ready.
fn print_success_message(agent: Option<&str>, ctx: &OutputContext) {
    if ctx.quiet {
        return;
    }
    print_guarantees(ctx);
    if let Some(name) = agent {
        ctx.success(&format!("Workspace ready. Agent: {name}"));
        ctx.kv("Agent shell", "polis agent shell");
        ctx.kv("Agent commands", "polis agent cmd help");
    } else {
        ctx.success("Workspace ready.");
    }
    ctx.kv("Connect", "polis connect");
    ctx.kv("Status", "polis status");
}

fn print_guarantees(ctx: &OutputContext) {
    pub use owo_colors::{OwoColorize, Stream::Stdout, Style};
    if ctx.quiet {
        return;
    }
    let gov = Style::new().truecolor(37, 56, 144);
    let sec = Style::new().truecolor(26, 107, 160);
    let obs = Style::new().truecolor(26, 151, 179);
    println!(
        "✓ {}  policy engine active · audit trail recording",
        "[governance]   ".if_supports_color(Stdout, |t| t.style(gov))
    );
    println!(
        "✓ {}  workspace isolated · traffic inspection enabled",
        "[security]     ".if_supports_color(Stdout, |t| t.style(sec))
    );
    println!(
        "✓ {}  action tracing live · trust scoring active",
        "[observability]".if_supports_color(Stdout, |t| t.style(obs))
    );
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
