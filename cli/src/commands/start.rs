//! `polis start` — start workspace (download and create if needed).

use anyhow::{Context, Result};
use std::process::ExitCode;
use std::time::Duration;

use crate::app::AppContext;
use crate::application::services::ssh::{self, SshProvisionOptions};
use crate::application::services::workspace::start as service;

/// Run `polis start`.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn run(app: &AppContext) -> Result<ExitCode> {
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
    let outcome = service::start(
        &app.provisioner,
        &app.state_mgr,
        &app.assets,
        // LocalFs implements both LocalFs and FileHasher — passed as `hasher`
        // because start only needs SHA256 file hashing from it.
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
    ssh::provision_ssh(
        &app.provisioner,
        &app.ssh,
        SshProvisionOptions {
            consent_given: consent,
        },
        &reporter,
    )
    .await?;

    // Phase 3: Render outcome.
    app.renderer().render_start_outcome(&outcome, &[])?;

    Ok(ExitCode::SUCCESS)
}
