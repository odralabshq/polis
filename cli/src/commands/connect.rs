//! `polis connect` — SSH config management.

use anyhow::{Context, Result};
use clap::Args;

use crate::app::AppContext;
use crate::domain::workspace::CONTAINER_NAME;
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
    install_vm_pubkey(mp, &pubkey).await?;

    // Install pubkey into the workspace container's polis user.
    install_pubkey(mp, &pubkey).await?;

    // Pin the workspace host key so StrictHostKeyChecking can verify it.
    pin_host_key(mp, &app.ssh).await;

    show_connection_options(ctx);
    Ok(std::process::ExitCode::SUCCESS)
}

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

/// Validates that a public key has a safe format for use in shell commands.
///
/// # Errors
///
/// Returns an error if the key format is invalid or contains unsafe characters.
fn validate_pubkey(key: &str) -> Result<()> {
    anyhow::ensure!(
        key.starts_with("ssh-ed25519 ") || key.starts_with("ssh-rsa "),
        "invalid public key format"
    );
    anyhow::ensure!(
        key.chars()
            .all(|c| c.is_ascii_alphanumeric() || " +/=@.-\n".contains(c)),
        "public key contains invalid characters"
    );
    Ok(())
}

/// Installs `pubkey` into `~/.ssh/authorized_keys` of the VM's `ubuntu` user.
async fn install_vm_pubkey(
    mp: &impl crate::application::ports::ShellExecutor,
    pubkey: &str,
) -> Result<()> {
    validate_pubkey(pubkey)?;
    let key = pubkey.trim();

    let script = format!(
        "grep -qxF '{key}' /home/ubuntu/.ssh/authorized_keys 2>/dev/null || \
         printf '%s\\n' '{key}' >> /home/ubuntu/.ssh/authorized_keys"
    );

    let output = mp
        .exec(&["bash", "-c", &script])
        .await
        .context("installing public key in VM")?;

    anyhow::ensure!(
        output.status.success(),
        "failed to install public key in VM"
    );
    Ok(())
}

/// Installs `pubkey` into `~/.ssh/authorized_keys` of the `polis` user inside
/// the workspace container. Idempotent.
async fn install_pubkey(
    mp: &impl crate::application::ports::ShellExecutor,
    pubkey: &str,
) -> Result<()> {
    validate_pubkey(pubkey)?;

    let key = pubkey.trim();

    let script = format!(
        "mkdir -p /home/polis/.ssh && \
         chmod 700 /home/polis/.ssh && \
         chown polis:polis /home/polis/.ssh && \
         grep -qxF '{key}' /home/polis/.ssh/authorized_keys 2>/dev/null || \
         printf '%s\\n' '{key}' >> /home/polis/.ssh/authorized_keys && \
         chmod 600 /home/polis/.ssh/authorized_keys && \
         chown polis:polis /home/polis/.ssh/authorized_keys"
    );

    let output = mp
        .exec(&["docker", "exec", CONTAINER_NAME, "bash", "-c", &script])
        .await
        .context("installing public key in workspace")?;

    anyhow::ensure!(
        output.status.success(),
        "failed to install public key in workspace"
    );
    Ok(())
}

/// Formats a raw public key as a `known_hosts` line and writes it via the
/// given manager. Returns `Ok(())` on success.
async fn write_host_key(ssh: &impl crate::application::ports::SshConfigurator, raw_key: &str) -> Result<()> {
    let trimmed = raw_key.trim();
    anyhow::ensure!(!trimmed.is_empty(), "empty host key");
    crate::domain::ssh::validate_host_key(trimmed)?;
    let host_key = format!("workspace {trimmed}");
    ssh.update_host_key(&host_key).await
}

/// Extracts the workspace SSH host key and writes it to `~/.polis/known_hosts`.
async fn pin_host_key(mp: &impl crate::application::ports::ShellExecutor, ssh: &impl crate::application::ports::SshConfigurator) {
    let Ok(output) = mp
        .exec(&[
            "docker",
            "exec",
            CONTAINER_NAME,
            "cat",
            "/etc/ssh/ssh_host_ed25519_key.pub",
        ])
        .await
    else {
        return;
    };

    if output.status.success()
        && let Ok(key) = String::from_utf8(output.stdout)
    {
        let _ = write_host_key(ssh, &key).await;
    }
}


