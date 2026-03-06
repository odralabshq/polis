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
    let polis_image_override = std::env::var("POLIS_IMAGE").ok();
    let checks = workspace_doctor::diagnose(
        mp,
        &reporter,
        &app.cmd_runner,
        &app.network_probe,
        &app.local_fs,
        polis_image_override.clone(),
        &workspace_doctor::NetworkTargets::default(),
    )
    .await?;

    let issues = crate::domain::health::collect_issues(&checks);

    // 2. Render report
    app.renderer()
        .render_diagnostics(&checks, &issues, verbose)?;

    // 3. Optional Repair
    if fix && !issues.is_empty() {
        let (assets_dir, _guard) = app.assets_dir().context("extracting embedded assets")?;
        let version = env!("CARGO_PKG_VERSION");

        workspace_repair::repair(mp, &reporter, &assets_dir, version, false).await?;

        // Re-probe after repair to confirm success
        if !ctx.quiet {
            ctx.info("Verifying repair...");
        }
        let checks_after = workspace_doctor::diagnose(
            mp,
            &reporter,
            &app.cmd_runner,
            &app.network_probe,
            &app.local_fs,
            polis_image_override,
            &workspace_doctor::NetworkTargets::default(),
        )
        .await?;
        let issues_after = crate::domain::health::collect_issues(&checks_after);
        app.renderer()
            .render_diagnostics(&checks_after, &issues_after, verbose)?;

        // Return success only if no issues remain after repair
        if issues_after.is_empty() {
            Ok(ExitCode::SUCCESS)
        } else {
            Ok(ExitCode::FAILURE)
        }
    } else if issues.is_empty() {
        // No issues detected - success
        Ok(ExitCode::SUCCESS)
    } else {
        // Issues detected and --fix not active - failure
        if !ctx.quiet {
            ctx.info("Run 'polis doctor --fix' to attempt automated repair.");
        }
        Ok(ExitCode::FAILURE)
    }
}
