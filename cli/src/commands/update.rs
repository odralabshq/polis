//! `polis update` — self-update with checksum and signature verification.


use anyhow::{Context, Result};
use clap::Args;

use crate::app::AppContext;
use crate::application::services::update::{
    SignatureInfo, UpdateChecker, UpdateInfo, UpdateVmConfigOutcome, update_vm_config,
};
use crate::application::services::vm::lifecycle::{self as vm, VmState};

/// Arguments for the update command.
#[derive(Args)]
pub struct UpdateArgs {
    /// Check for updates without applying them
    #[arg(long)]
    pub check: bool,
}

/// Embedded ed25519 public key (base64) for verifying signed CLI release archives.
///
/// The corresponding private key is stored as `POLIS_SIGNING_KEY` in GitHub
/// Actions secrets and used by the release workflow to sign `.tar.gz` / `.zip`
/// archives via `zipsign`.

/// Production implementation using GitHub releases.
// ── Entry point ───────────────────────────────────────────────────────────────

/// Run `polis update [--check]`.
///
/// Checks GitHub for a newer release, verifies its signature, prompts the user,
/// then downloads and replaces the current binary. If the VM is running, also
/// updates the VM config.
///
/// # Errors
///
/// Returns an error if the version check, signature verification, download, or
/// user prompt fails.
#[allow(clippy::unused_async)] // async contract: will gain awaits when download is made async
pub async fn run(args: &UpdateArgs, app: &AppContext, checker: &impl UpdateChecker) -> Result<std::process::ExitCode> {
    let ctx = &app.output;
    let mp = &app.provisioner;
    let current = env!("CARGO_PKG_VERSION");

    if !ctx.quiet {
        ctx.info("Checking for updates...");
    }

    let cli_update = checker.check(current)?;

    match &cli_update {
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

    if args.check {
        ctx.info("Run 'polis update' to apply the update.");
        return Ok(std::process::ExitCode::SUCCESS);
    }

    if matches!(cli_update, UpdateInfo::Available { .. }) {
        apply_cli_update(app, checker, cli_update)?;
    }

    // After CLI self-update, update VM config if the VM is running
    let vm_state = vm::state(mp).await?;
    if vm_state == VmState::Running {
        if !ctx.quiet {
            ctx.info("Updating VM config...");
        }
        update_config(app).await?;
    }

    Ok(std::process::ExitCode::SUCCESS)
}

/// Update the VM config when the CLI has been updated to a new version.
///
/// Extracts embedded assets, computes the SHA256 of the new config tarball,
/// and compares it against the hash stored in the VM. If they differ, stops
/// services, transfers the new config, pulls images, verifies digests,
/// restarts services, and writes the new hash.
///
/// # Errors
///
/// Returns an error if any step of the update cycle fails.
pub async fn update_config(app: &AppContext) -> Result<()> {
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

fn apply_cli_update(
    app: &AppContext,
    checker: &impl UpdateChecker,
    cli_update: UpdateInfo,
) -> Result<()> {
    let ctx = &app.output;
    let UpdateInfo::Available {
        version,
        download_url,
        ..
    } = cli_update
    else {
        return Ok(());
    };

    if !ctx.quiet {
        ctx.info("Verifying checksum...");
    }
    let sig = checker
        .verify_signature(&download_url)
        .context("checksum verification failed")?;

    let sha_preview = sig.sha256.get(..12).unwrap_or(&sig.sha256);
    ctx.success(&format!("SHA-256: {sha_preview}..."));

    let confirmed = app
        .confirm("Update CLI now?", true)
        .context("reading confirmation")?;

    if confirmed {
        if !ctx.quiet {
            ctx.info("Downloading...");
        }
        checker.perform_update(&version).context("update failed")?;
        ctx.success(&format!("CLI updated to v{version}"));
        ctx.info("Restart your terminal or run: exec polis");
    }
    Ok(())
}


// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // run() via UpdateChecker trait mock — unit
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_run_up_to_date_returns_ok() {
        struct AlwaysUpToDate;
        impl UpdateChecker for AlwaysUpToDate {
            fn check(&self, _current: &str) -> anyhow::Result<UpdateInfo> {
                Ok(UpdateInfo::UpToDate)
            }
            fn verify_signature(&self, _url: &str) -> anyhow::Result<SignatureInfo> {
                anyhow::bail!("not expected: should not verify when up to date")
            }
            fn perform_update(&self, _version: &str) -> anyhow::Result<()> {
                anyhow::bail!("not expected: should not update when up to date")
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
        let result = run(&args, &app, &AlwaysUpToDate).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_invalid_signature_returns_err() {
        struct BadSignature;
        impl UpdateChecker for BadSignature {
            fn check(&self, _current: &str) -> anyhow::Result<UpdateInfo> {
                Ok(UpdateInfo::Available {
                    version: "9.9.9".to_string(),
                    release_notes: vec![],
                    download_url: "https://example.com/polis.tar.gz".to_string(),
                })
            }
            fn verify_signature(&self, _url: &str) -> anyhow::Result<SignatureInfo> {
                Err(anyhow::anyhow!("checksum verification failed"))
            }
            fn perform_update(&self, _version: &str) -> anyhow::Result<()> {
                anyhow::bail!("not expected: should not update when checksum is invalid")
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
        let result = run(&args, &app, &BadSignature).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("checksum"),
            "error should mention checksum"
        );
    }

    // -----------------------------------------------------------------------
    // hex_encode — unit
    // -----------------------------------------------------------------------

    #[test]
    fn test_hex_encode_empty_returns_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_single_byte() {
        assert_eq!(hex_encode(&[0x00]), "00");
        assert_eq!(hex_encode(&[0xff]), "ff");
        assert_eq!(hex_encode(&[0xab]), "ab");
    }

    #[test]
    fn test_hex_encode_multiple_bytes() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }
}
