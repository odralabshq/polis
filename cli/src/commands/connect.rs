//! `polis connect` — SSH config management.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Args;

use crate::output::OutputContext;
use crate::ssh::{SshConfigManager, ensure_identity_key};
use crate::workspace::CONTAINER_NAME;

/// Timeout for exec calls during connect setup.
/// Prevents `polis connect` from hanging when Docker/containers are unresponsive.
const CONNECT_EXEC_TIMEOUT: Duration = Duration::from_secs(15);

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
pub async fn run(
    ctx: &OutputContext,
    _args: ConnectArgs,
    mp: &impl crate::multipass::Multipass,
) -> Result<()> {
    let ssh_mgr = SshConfigManager::new()?;

    if ssh_mgr.is_configured()? {
        // Refresh polis config to pick up any template changes (idempotent).
        ssh_mgr.create_polis_config()?;
        ssh_mgr.create_sockets_dir()?;
    } else {
        setup_ssh_config(&ssh_mgr)?;
    }

    ssh_mgr.validate_permissions()?;

    // Ensure a passphrase-free identity key exists and is installed in the workspace.
    let pubkey = ensure_identity_key()?;

    // Install pubkey into the VM's ubuntu user so `polis _ssh-proxy` can SSH
    // to the VM directly (bypasses multipass exec stdin bug on Windows).
    install_vm_pubkey(&pubkey, mp).await?;

    // Install pubkey into the workspace container's polis user.
    install_pubkey(&pubkey, mp).await?;

    // Pin the workspace host key so StrictHostKeyChecking can verify it.
    pin_host_key(mp).await;

    show_connection_options(ctx);
    Ok(())
}

fn setup_ssh_config(ssh_mgr: &SshConfigManager) -> Result<()> {
    println!();
    println!("Setting up SSH access...");
    println!();

    let confirmed = dialoguer::Confirm::new()
        .with_prompt("Add SSH configuration to ~/.ssh/config?")
        .default(true)
        .interact()
        .context("reading confirmation")?;

    if !confirmed {
        println!("Skipped. You can set up SSH manually later.");
        return Ok(());
    }

    ssh_mgr.create_polis_config()?;
    ssh_mgr.add_include_directive()?;
    ssh_mgr.create_sockets_dir()?;

    println!("SSH configured");
    println!();
    Ok(())
}

fn show_connection_options(_ctx: &OutputContext) {
    println!();
    println!("Connect with:");
    println!("    ssh workspace");
    println!("    code --remote ssh-remote+workspace /workspace");
    println!("    cursor --remote ssh-remote+workspace /workspace");
    println!();
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
///
/// This enables `polis _ssh-proxy` to SSH directly to the VM (bypassing
/// `multipass exec` which has a stdin pipe bug on Windows).
/// Idempotent — skips if key already present.
async fn install_vm_pubkey(pubkey: &str, _mp: &impl crate::multipass::Multipass) -> Result<()> {
    validate_pubkey(pubkey)?;
    let key = pubkey.trim();

    let script = format!(
        "grep -qxF '{key}' /home/ubuntu/.ssh/authorized_keys 2>/dev/null || \
         printf '%s\\n' '{key}' >> /home/ubuntu/.ssh/authorized_keys"
    );

    let output = crate::multipass::exec_with_timeout(
        &["bash", "-c", &script],
        CONNECT_EXEC_TIMEOUT,
    )
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
///
/// Uses `printf` + shell command instead of stdin piping because
/// `multipass exec ... docker exec -i ...` stdin pipes hang on Windows
/// (the EOF is never propagated through the double-pipe chain).
async fn install_pubkey(pubkey: &str, _mp: &impl crate::multipass::Multipass) -> Result<()> {
    // SEC-001: Validate pubkey format before use to prevent shell injection.
    // validate_pubkey ensures only [a-zA-Z0-9 +/=@.-\n] — no quotes or
    // shell metacharacters — so embedding in single-quoted printf is safe.
    validate_pubkey(pubkey)?;

    let key = pubkey.trim();

    // Single command: create .ssh dir, append key (idempotent via grep guard),
    // fix permissions — all without stdin piping.
    let script = format!(
        "mkdir -p /home/polis/.ssh && \
         chmod 700 /home/polis/.ssh && \
         chown polis:polis /home/polis/.ssh && \
         grep -qxF '{key}' /home/polis/.ssh/authorized_keys 2>/dev/null || \
         printf '%s\\n' '{key}' >> /home/polis/.ssh/authorized_keys && \
         chmod 600 /home/polis/.ssh/authorized_keys && \
         chown polis:polis /home/polis/.ssh/authorized_keys"
    );

    let output = crate::multipass::exec_with_timeout(
        &["docker", "exec", CONTAINER_NAME, "bash", "-c", &script],
        CONNECT_EXEC_TIMEOUT,
    )
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
fn write_host_key(mgr: &crate::ssh::KnownHostsManager, raw_key: &str) -> Result<()> {
    let trimmed = raw_key.trim();
    anyhow::ensure!(!trimmed.is_empty(), "empty host key");
    crate::ssh::validate_host_key(trimmed)?;
    let host_key = format!("workspace {trimmed}");
    mgr.update(&host_key)
}

/// Extracts the workspace SSH host key and writes it to `~/.polis/known_hosts`.
async fn pin_host_key(_mp: &impl crate::multipass::Multipass) {
    let Ok(output) = crate::multipass::exec_with_timeout(
        &[
            "docker",
            "exec",
            CONTAINER_NAME,
            "cat",
            "/etc/ssh/ssh_host_ed25519_key.pub",
        ],
        CONNECT_EXEC_TIMEOUT,
    )
    .await
    else {
        return;
    };

    if output.status.success()
        && let Ok(key) = String::from_utf8(output.stdout)
    {
        let mgr = crate::ssh::KnownHostsManager::new();
        let _ = mgr.and_then(|m| write_host_key(&m, &key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // write_host_key
    // -----------------------------------------------------------------------

    fn known_hosts_in(dir: &tempfile::TempDir) -> crate::ssh::KnownHostsManager {
        crate::ssh::KnownHostsManager::with_path(dir.path().join("known_hosts"))
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
