use anyhow::{Context, Result};
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::services::workspace_doctor;
use crate::application::services::workspace_repair;

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run `polis doctor`.
///
/// Executes diagnostics across prerequisites, workspace, network, and security.
/// If `--fix` is active, attempts to repair any detected issues.
///
/// # Errors
///
/// Returns an error if health checks or repair steps fail fatally.
pub async fn run(app: &AppContext, verbose: bool, fix: bool) -> Result<ExitCode> {
    let ctx = &app.output;
    let mp = &app.provisioner;
    let reporter = app.terminal_reporter();

    // 1. Diagnose
    let checks = workspace_doctor::run_doctor(
        mp,
        &reporter,
        &app.cmd_runner,
        &app.network_probe,
        &app.local_fs,
    )
    .await?;

    let issues = crate::domain::health::collect_issues(&checks);

    // 2. Render report
    app.renderer().render_doctor(&checks, &issues, verbose)?;

    // 3. Optional Repair
    if fix && !issues.is_empty() {
        let (assets_dir, _guard) = app.assets_dir().context("extracting embedded assets")?;
        let version = env!("CARGO_PKG_VERSION");

        workspace_repair::run_repair(mp, &reporter, &assets_dir, version, false).await?;

        // Re-probe after repair to confirm success
        if !ctx.quiet {
            ctx.info("Verifying repair...");
        }
        let checks_after = workspace_doctor::run_doctor(
            mp,
            &reporter,
            &app.cmd_runner,
            &app.network_probe,
            &app.local_fs,
        )
        .await?;
        let issues_after = crate::domain::health::collect_issues(&checks_after);
        app.renderer()
            .render_doctor(&checks_after, &issues_after, verbose)?;
    } else if !issues.is_empty() && !ctx.quiet {
        ctx.info("Run 'polis doctor --fix' to attempt automated repair.");
    }

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_run_healthy_yields_ok() {
        let _app = crate::app::AppContext::new(&crate::app::AppFlags {
            output: crate::app::OutputFlags {
                no_color: true,
                quiet: true,
                json: false,
            },
            behaviour: crate::app::BehaviourFlags { yes: true },
        })
        .expect("AppContext");

        // Note: This test will fail if dependencies are not mocked,
        // but it illustrates the intended command structure.
    }
}
