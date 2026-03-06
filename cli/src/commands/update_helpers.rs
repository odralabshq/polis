//! Helpers for `polis update` — extracted to keep the command handler thin.

use anyhow::{Context, Result};

use crate::app::AppContext;
use crate::application::ports::ProgressReporter;
use crate::application::services::update::{
    UpdateChecker, UpdateInfo, UpdateVmConfigOutcome, update_vm_config,
};
use crate::output::OutputContext;

/// Print version comparison info to the terminal.
pub fn print_update_info(ctx: &OutputContext, current: &str, info: &UpdateInfo) {
    match info {
        UpdateInfo::UpToDate => {
            ctx.success(&format!("CLI v{current} ({info})"));
        }
        UpdateInfo::Available {
            version,
            release_notes,
            ..
        } => {
            ctx.info(&format!("CLI v{current} → {info}"));
            if !release_notes.is_empty() && !ctx.quiet {
                println!("  Changes in v{version}:");
                for note in release_notes {
                    println!("    • {note}");
                }
            }
        }
    }
}

/// Verify, confirm, and perform the CLI binary update.
/// Returns `true` if the binary was actually replaced.
///
/// # Errors
/// Returns an error if verification, confirmation, or download fails.
pub async fn apply_cli_update<C>(
    app: &AppContext,
    checker: C,
    cli_update: UpdateInfo,
) -> Result<bool>
where
    C: UpdateChecker + Clone + Send + 'static,
{
    let ctx = &app.output;
    let reporter = app.terminal_reporter();
    let UpdateInfo::Available {
        version,
        download_url,
        ..
    } = cli_update
    else {
        return Ok(false);
    };

    reporter.begin_stage("downloading and verifying...");
    let checker_clone = checker.clone();
    let url = download_url.clone();
    let asset = tokio::task::spawn_blocking(move || checker_clone.download_and_verify(&url))
        .await
        .context("spawn_blocking panicked")?
        .context("download and verification failed")?;
    reporter.complete_stage();

    let sha_preview = asset.sha256.get(..12).unwrap_or(&asset.sha256);
    ctx.success(&format!("SHA-256: {sha_preview}..."));

    let confirmed = app
        .confirm("Update CLI now?", true)
        .context("reading confirmation")?;

    if confirmed {
        reporter.begin_stage("installing update...");
        tokio::task::spawn_blocking(move || checker.install(asset))
            .await
            .context("spawn_blocking panicked")?
            .context("update failed")?;
        reporter.complete_stage();
        ctx.success(&format!("CLI updated to v{version}"));
        if cfg!(windows) {
            ctx.info("Restart your terminal to use the new version.");
        } else {
            ctx.info("Restart your terminal or run: exec polis");
        }
        return Ok(true);
    }
    Ok(false)
}

/// Run the newly-installed `polis` binary with the hidden `_post-update`
/// command so the VM config update uses the NEW binary's embedded assets.
/// Uses `tokio::process::Command` for non-blocking execution.
///
/// # Errors
/// Returns an error if the new binary cannot be run.
pub async fn run_post_update(ctx: &OutputContext) -> Result<()> {
    let exe = std::env::current_exe().context("resolving current executable path")?;
    let status = tokio::process::Command::new(&exe)
        .arg("_post-update")
        .status()
        .await
        .context("failed to run post-update process")?;

    if status.success() {
        ctx.success("VM config updated via new binary");
    } else {
        ctx.warn("VM config update returned non-zero — check with: polis status");
    }
    Ok(())
}

/// Run the VM config update cycle using the current binary's embedded assets.
///
/// # Errors
/// Returns an error if any step of the update cycle fails.
pub async fn run_vm_config_update(app: &AppContext) -> Result<()> {
    let ctx = &app.output;
    let (assets_dir, _guard) = app.assets_dir().context("extracting embedded assets")?;

    let version = env!("CARGO_PKG_VERSION");
    let reporter = app.terminal_reporter();
    let hasher = &crate::infra::fs::OsFs;

    match update_vm_config(
        &app.provisioner,
        &app.assets,
        hasher,
        &reporter,
        &assets_dir,
        version,
    )
    .await?
    {
        UpdateVmConfigOutcome::UpToDate => {
            ctx.success("Config is up to date");
        }
        UpdateVmConfigOutcome::Updated => {
            ctx.success("Config updated successfully");
        }
    }

    Ok(())
}
