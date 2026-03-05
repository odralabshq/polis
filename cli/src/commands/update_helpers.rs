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
            ctx.success(&format!("CLI v{current} (latest)"));
        }
        UpdateInfo::Available {
            version,
            release_notes,
            ..
        } => {
            ctx.info(&format!("CLI v{current} → v{version} available"));
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
pub fn apply_cli_update(
    app: &AppContext,
    checker: &impl UpdateChecker,
    cli_update: UpdateInfo,
) -> Result<bool> {
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

    reporter.begin_stage("verifying checksum...");
    let sig = checker
        .verify_signature(&download_url)
        .context("checksum verification failed")?;
    reporter.complete_stage();

    let sha_preview = sig.sha256.get(..12).unwrap_or(&sig.sha256);
    ctx.success(&format!("SHA-256: {sha_preview}..."));

    let confirmed = app
        .confirm("Update CLI now?", true)
        .context("reading confirmation")?;

    if confirmed {
        reporter.begin_stage("downloading update...");
        checker.perform_update(&version).context("update failed")?;
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


/// Spawn the newly-installed `polis` binary with the hidden `_post-update`
/// command so the VM config update uses the NEW binary's embedded assets.
///
/// # Errors
/// Returns an error if the new binary cannot be spawned.
pub fn spawn_post_update(ctx: &OutputContext) -> Result<()> {
    let exe = std::env::current_exe().context("resolving current executable path")?;
    let status = std::process::Command::new(&exe)
        .arg("_post-update")
        .status()
        .context("failed to spawn post-update process")?;

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
    let hasher = &crate::infra::fs::LocalFs;

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
