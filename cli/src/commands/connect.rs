//! `polis connect` — SSH config management.

use anyhow::Result;
use clap::Args;

use crate::app::AppContext;
use crate::application::ports::SshConfigurator;

/// Arguments for the connect command.
#[derive(Args)]
pub struct ConnectArgs {}

/// Run `polis connect`.
///
/// Sets up SSH config on first run, validates permissions, then prints
/// connection instructions.
///
/// # Errors
///
/// Returns an error if SSH config setup fails or permissions are unsafe.
pub async fn run(app: &AppContext, _args: ConnectArgs) -> Result<std::process::ExitCode> {
    let ctx = &app.output;
    let mp = &app.provisioner;
    if SshConfigurator::is_configured(&app.ssh).await? {
        // Refresh polis config to pick up any template changes (idempotent).
        SshConfigurator::setup_config(&app.ssh).await?;
    } else {
        setup_ssh_config(app).await?;
    }

    SshConfigurator::validate_permissions(&app.ssh).await?;

    // Ensure a passphrase-free identity key exists and is installed in the workspace.
    let pubkey = SshConfigurator::ensure_identity(&app.ssh).await?;

    // Install pubkey into the VM's ubuntu user so `polis _ssh-proxy` can SSH
    // to the VM directly (bypasses multipass exec stdin bug on Windows).
    crate::application::services::connect::install_vm_pubkey(mp, &pubkey).await?;

    // Install pubkey into the workspace container's polis user.
    crate::application::services::connect::install_pubkey(mp, &pubkey).await?;

    // Pin the workspace host key so StrictHostKeyChecking can verify it.
    crate::application::services::connect::pin_host_key(mp, &app.ssh).await;

    show_connection_options(ctx);
    Ok(std::process::ExitCode::SUCCESS)
}

/// # Errors
///
/// This function will return an error if the underlying operations fail.
async fn setup_ssh_config(app: &AppContext) -> Result<()> {
    // setup_ssh_config is interactive — uses eprintln for user-facing messages.
    eprintln!();
    eprintln!("Setting up SSH access...");
    eprintln!();

    let confirmed = app.confirm("Add SSH configuration to ~/.ssh/config?", true)?;

    if !confirmed {
        eprintln!("Skipped. You can set up SSH manually later.");
        return Ok(());
    }

    SshConfigurator::setup_config(&app.ssh).await?;

    eprintln!("SSH configured");
    eprintln!();
    Ok(())
}

fn show_connection_options(ctx: &crate::output::OutputContext) {
    ctx.info("Connect with:");
    ctx.info("    ssh workspace");
    ctx.info("    code --remote ssh-remote+workspace /workspace");
    ctx.info("    cursor --remote ssh-remote+workspace /workspace");
}
