//! `polis update` — self-update with checksum and signature verification.

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args;

use crate::app::App;
use crate::application::ports::{ProgressReporter, UpdateChecker, UpdateInfo};
use crate::application::services::update::{
    PostUpdateOutcome, UpdateVmConfigOutcome, download_and_verify_cli_update, install_cli_update,
    run_post_update, run_vm_config_update_service,
};
use crate::application::vm::lifecycle::is_running;

/// Arguments for the update command.
#[derive(Args)]
pub struct UpdateArgs {
    /// Check for updates without applying them
    #[arg(long)]
    pub check: bool,
}

/// Run `polis update [--check]`.
/// # Errors
/// Returns an error if the version check, signature verification, download, or
/// user prompt fails.
pub async fn run<A: App, C>(app: &A, args: &UpdateArgs, checker: C) -> Result<ExitCode>
where
    C: UpdateChecker + Clone + Send + 'static,
{
    let ctx = app.output();
    let current = env!("CARGO_PKG_VERSION");
    let reporter = app.terminal_reporter();

    reporter.begin_stage("checking for updates...");
    let checker_clone = checker.clone();
    let current_owned = current.to_string();
    let cli_update = tokio::task::spawn_blocking(move || checker_clone.check(&current_owned))
        .await
        .context("spawn_blocking panicked")??;
    reporter.complete_stage();
    app.renderer().render_update_info(current, &cli_update)?;

    if args.check {
        if matches!(cli_update, UpdateInfo::Available { .. }) {
            ctx.info("Run 'polis update' to apply the update.");
        }
        return Ok(ExitCode::SUCCESS);
    }

    let did_update = apply_cli_update(app, checker, cli_update).await?;
    let vm_running = is_running(app.provisioner()).await?;

    // After CLI self-update, delegate VM config update to the NEW binary
    if did_update && vm_running {
        match run_post_update(&crate::infra::process::OsProcessLauncher).await? {
            PostUpdateOutcome::Success => ctx.success("VM config updated via new binary"),
            PostUpdateOutcome::NonZeroExit => {
                ctx.warn("VM config update returned non-zero — check with: polis status");
            }
        }
    } else if !did_update && vm_running {
        run_vm_config_update_with_output(app).await?;
    }

    Ok(ExitCode::SUCCESS)
}

/// Verify, confirm, and perform the CLI binary update. Returns `true` if updated.
async fn apply_cli_update<A: App, C>(app: &A, checker: C, cli_update: UpdateInfo) -> Result<bool>
where
    C: UpdateChecker + Clone + Send + 'static,
{
    let ctx = app.output();
    let reporter = app.terminal_reporter();
    let UpdateInfo::Available {
        version,
        download_url,
        ..
    } = cli_update
    else {
        return Ok(false);
    };

    let asset = download_and_verify_cli_update(&checker, &download_url, &reporter).await?;
    let sha_preview = asset.sha256.get(..12).unwrap_or(&asset.sha256);
    ctx.success(&format!("SHA-256: {sha_preview}..."));

    let confirmed = app
        .confirm("Update CLI now?", true)
        .context("reading confirmation")?;
    if confirmed {
        install_cli_update(checker, asset, &reporter).await?;
        ctx.success(&format!("CLI updated to v{version}"));
        ctx.info(if cfg!(windows) {
            "Restart your terminal to use the new version."
        } else {
            "Restart your terminal or run: exec polis"
        });
        return Ok(true);
    }
    Ok(false)
}

/// Run the VM config update cycle with output rendering.
async fn run_vm_config_update_with_output(app: &impl App) -> Result<()> {
    let (assets_dir, _guard) = app.assets_dir().context("extracting embedded assets")?;
    let reporter = app.terminal_reporter();

    match run_vm_config_update_service(
        app.provisioner(),
        app.assets(),
        app.fs(),
        &reporter,
        &assets_dir,
        env!("CARGO_PKG_VERSION"),
    )
    .await?
    {
        UpdateVmConfigOutcome::UpToDate => app.output().success("Config is up to date"),
        UpdateVmConfigOutcome::Updated => app.output().success("Config updated successfully"),
    }
    Ok(())
}

/// Run the VM config update cycle (used by `_post-update` hidden command).
///
/// # Errors
///
/// Returns an error if the VM config update fails.
pub async fn post_update(app: &impl App) -> Result<()> {
    run_vm_config_update_with_output(app).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
mod tests {
    use super::*;
    use crate::application::ports::VerifiedAsset;

    #[tokio::test]
    async fn test_run_up_to_date_returns_ok() {
        #[derive(Clone, Copy)]
        struct AlwaysUpToDate;
        impl UpdateChecker for AlwaysUpToDate {
            fn check(&self, _current: &str) -> anyhow::Result<UpdateInfo> {
                Ok(UpdateInfo::UpToDate)
            }
            fn download_and_verify(&self, _url: &str) -> anyhow::Result<VerifiedAsset> {
                anyhow::bail!("not expected")
            }
            fn install(&self, _asset: VerifiedAsset) -> anyhow::Result<()> {
                anyhow::bail!("not expected")
            }
        }

        let args = UpdateArgs { check: true };
        let app = crate::app::AppContext::new(&crate::app::AppFlags {
            output: crate::app::OutputFlags {
                no_color: true,
                quiet: true,
                json: false,
            },
            behaviour: crate::app::BehaviourFlags { yes: true },
        })
        .expect("AppContext");
        let result = run(&app, &args, AlwaysUpToDate).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_invalid_signature_returns_err() {
        #[derive(Clone, Copy)]
        struct BadSignature;
        impl UpdateChecker for BadSignature {
            fn check(&self, _current: &str) -> anyhow::Result<UpdateInfo> {
                Ok(UpdateInfo::Available {
                    version: "9.9.9".to_string(),
                    release_notes: vec![],
                    download_url: "https://example.com/polis.tar.gz".to_string(),
                })
            }
            fn download_and_verify(&self, _url: &str) -> anyhow::Result<VerifiedAsset> {
                Err(anyhow::anyhow!("checksum verification failed"))
            }
            fn install(&self, _asset: VerifiedAsset) -> anyhow::Result<()> {
                anyhow::bail!("not expected")
            }
        }

        let args = UpdateArgs { check: false };
        let app = crate::app::AppContext::new(&crate::app::AppFlags {
            output: crate::app::OutputFlags {
                no_color: true,
                quiet: true,
                json: false,
            },
            behaviour: crate::app::BehaviourFlags { yes: true },
        })
        .expect("AppContext");
        let result = run(&app, &args, BadSignature).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("checksum") || err_msg.contains("verification"),
            "Expected error to contain 'checksum' or 'verification', got: {err_msg}"
        );
    }
}
