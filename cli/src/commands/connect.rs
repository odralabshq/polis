//! `polis connect` — SSH config management.

use anyhow::{Context, Result};
use clap::Args;

use crate::app::AppContext;
use crate::domain::workspace::CONTAINER_NAME;
use crate::infra::ssh::{SshConfigManager, ensure_identity_key};

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
    let ssh_mgr = SshConfigManager::new()?;

    if ssh_mgr.is_configured()? {
        // Refresh polis config to pick up any template changes (idempotent).
        ssh_mgr.create_polis_config()?;
        ssh_mgr.create_sockets_dir()?;
    } else {
        setup_ssh_config(&ssh_mgr, app)?;
    }

    ssh_mgr.validate_permissions()?;

    // Ensure a passphrase-free identity key exists and is installed in the workspace.
    let pubkey = ensure_identity_key()?;

    // Install pubkey into the VM's ubuntu user so `polis _ssh-proxy` can SSH
    // to the VM directly (bypasses multipass exec stdin bug on Windows).
    install_vm_pubkey(mp, &pubkey).await?;

    // Install pubkey into the workspace container's polis user.
    install_pubkey(mp, &pubkey).await?;

    // Pin the workspace host key so StrictHostKeyChecking can verify it.
    pin_host_key(mp).await;

    show_connection_options(ctx);
    Ok(std::process::ExitCode::SUCCESS)
}

fn setup_ssh_config(ssh_mgr: &SshConfigManager, app: &AppContext) -> Result<()> {
    // setup_ssh_config is interactive — uses eprintln for user-facing messages.
    eprintln!();
    eprintln!("Setting up SSH access...");
    eprintln!();

    let confirmed = app.confirm("Add SSH configuration to ~/.ssh/config?", true)?;

    if !confirmed {
        eprintln!("Skipped. You can set up SSH manually later.");
        return Ok(());
    }

    ssh_mgr.create_polis_config()?;
    ssh_mgr.add_include_directive()?;
    ssh_mgr.create_sockets_dir()?;

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
fn write_host_key(mgr: &crate::infra::ssh::KnownHostsManager, raw_key: &str) -> Result<()> {
    let trimmed = raw_key.trim();
    anyhow::ensure!(!trimmed.is_empty(), "empty host key");
    crate::infra::ssh::validate_host_key(trimmed)?;
    let host_key = format!("workspace {trimmed}");
    mgr.update(&host_key)
}

/// Extracts the workspace SSH host key and writes it to `~/.polis/known_hosts`.
async fn pin_host_key(mp: &impl crate::application::ports::ShellExecutor) {
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
        let mgr = crate::infra::ssh::KnownHostsManager::new();
        let _ = mgr.and_then(|m| write_host_key(&m, &key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // write_host_key
    // -----------------------------------------------------------------------

    fn known_hosts_in(dir: &tempfile::TempDir) -> crate::infra::ssh::KnownHostsManager {
        crate::infra::ssh::KnownHostsManager::with_path(dir.path().join("known_hosts"))
    }

    #[test]
    fn test_write_host_key_writes_known_hosts_with_workspace_prefix() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = known_hosts_in(&dir);
        write_host_key(&mgr, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey")
            .expect("should succeed");
        let content = std::fs::read_to_string(dir.path().join("known_hosts")).expect("read");
        assert_eq!(
            content,
            "workspace ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey"
        );
    }

    #[test]
    fn test_write_host_key_trims_whitespace() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = known_hosts_in(&dir);
        write_host_key(&mgr, "  ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey\n")
            .expect("should succeed");
        let content = std::fs::read_to_string(dir.path().join("known_hosts")).expect("read");
        assert_eq!(
            content,
            "workspace ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey"
        );
    }

    #[test]
    fn test_write_host_key_rejects_empty_key() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = known_hosts_in(&dir);
        assert!(write_host_key(&mgr, "").is_err());
        assert!(write_host_key(&mgr, "   \n").is_err());
    }

    #[test]
    fn test_write_host_key_rejects_non_ed25519_key() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = known_hosts_in(&dir);
        assert!(write_host_key(&mgr, "ssh-rsa AAAAB3NzaC1yc2EAAA").is_err());
    }

    #[test]
    fn test_write_host_key_overwrites_previous_key() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = known_hosts_in(&dir);
        write_host_key(&mgr, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOldKey").expect("first");
        write_host_key(&mgr, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINewKey").expect("second");
        let content = std::fs::read_to_string(dir.path().join("known_hosts")).expect("read");
        assert_eq!(
            content,
            "workspace ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINewKey"
        );
    }
}
