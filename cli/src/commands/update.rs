//! `polis update` — self-update with checksum and signature verification.

use anyhow::Result;
use clap::Args;

use crate::app::AppContext;
use crate::application::ports::ProgressReporter;
use crate::application::services::update::{UpdateChecker, UpdateInfo};
use crate::application::services::workspace_stop::is_vm_running;
use crate::commands::update_helpers;

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
#[allow(clippy::unused_async)]
pub async fn run(
    args: &UpdateArgs,
    app: &AppContext,
    checker: &impl UpdateChecker,
) -> Result<std::process::ExitCode> {
    let ctx = &app.output;
    let mp = &app.provisioner;
    let current = env!("CARGO_PKG_VERSION");
    let reporter = app.terminal_reporter();

    reporter.begin_stage("checking for updates...");
    let cli_update = checker.check(current)?;
    reporter.complete_stage();
    update_helpers::print_update_info(ctx, current, &cli_update);

    if args.check {
        if matches!(cli_update, UpdateInfo::Available { .. }) {
            ctx.info("Run 'polis update' to apply the update.");
        }
        return Ok(std::process::ExitCode::SUCCESS);
    }

    let did_update = update_helpers::apply_cli_update(app, checker, cli_update)?;

    // After CLI self-update, delegate VM config update to the NEW binary so
    // its embedded assets are used instead of the stale ones from the old binary.
    if did_update && is_vm_running(mp).await? {
        update_helpers::spawn_post_update(ctx)?;
    } else if !did_update && is_vm_running(mp).await? {
        update_helpers::run_vm_config_update(app).await?;
    }

    Ok(std::process::ExitCode::SUCCESS)
}

/// Run the VM config update cycle (used by `_post-update` hidden command).
/// # Errors
/// Returns an error if any step of the update cycle fails.
pub async fn post_update(app: &AppContext) -> Result<()> {
    update_helpers::run_vm_config_update(app).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::wildcard_imports)]
mod tests {
    use super::*;
    use crate::application::services::update::SignatureInfo;
    use crate::domain::util::hex_encode;

    #[tokio::test]
    async fn test_run_up_to_date_returns_ok() {
        struct AlwaysUpToDate;
        impl UpdateChecker for AlwaysUpToDate {
            fn check(&self, _current: &str) -> anyhow::Result<UpdateInfo> {
                Ok(UpdateInfo::UpToDate)
            }
            fn verify_signature(&self, _url: &str) -> anyhow::Result<SignatureInfo> {
                anyhow::bail!("not expected")
            }
            fn perform_update(&self, _version: &str) -> anyhow::Result<()> {
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
        let result = run(&args, &app, &BadSignature).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("checksum"));
    }

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
