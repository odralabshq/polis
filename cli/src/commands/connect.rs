//! `polis connect` — SSH config management and IDE integration.

use anyhow::{Context, Result};
use clap::Args;

use crate::output::OutputContext;
use crate::ssh::{SshConfigManager, ensure_identity_key};
use crate::workspace::CONTAINER_NAME;

/// Arguments for the connect command.
#[derive(Args)]
pub struct ConnectArgs {
    /// Open in IDE: vscode, cursor
    #[arg(long)]
    pub ide: Option<String>,
}

/// Run `polis connect [--ide <name>]`.
///
/// Sets up SSH config on first run, validates permissions, then either opens
/// an IDE or prints connection instructions.
///
/// # Errors
///
/// Returns an error if SSH config setup fails, permissions are unsafe, or the
/// IDE cannot be launched.
pub async fn run(
    ctx: &OutputContext,
    args: ConnectArgs,
    mp: &impl crate::multipass::Multipass,
) -> Result<()> {
    // Validate IDE name early — fail fast before any interactive prompts.
    if let Some(ref ide) = args.ide {
        resolve_ide(ide)?;
    }

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
    install_pubkey(&pubkey, mp).await?;

    // Pin the workspace host key so StrictHostKeyChecking can verify it.
    pin_host_key(mp).await;

    if let Some(ref ide) = args.ide {
        open_ide(ide).await
    } else {
        show_connection_options(ctx);
        Ok(())
    }
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

/// Resolves an IDE name to its binary and arguments.
///
/// # Errors
///
/// Returns an error if the IDE name is not recognised.
pub fn resolve_ide(name: &str) -> Result<(&'static str, &'static [&'static str])> {
    match name.to_lowercase().as_str() {
        "vscode" | "code" => Ok(("code", &["--remote", "ssh-remote+workspace", "/workspace"])),
        "cursor" => Ok((
            "cursor",
            &["--remote", "ssh-remote+workspace", "/workspace"],
        )),
        _ => anyhow::bail!("Unknown IDE: {name}. Supported: vscode, cursor"),
    }
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

/// Installs `pubkey` into `~/.ssh/authorized_keys` of the `polis` user inside
/// the workspace container. Idempotent.
async fn install_pubkey(pubkey: &str, mp: &impl crate::multipass::Multipass) -> Result<()> {
    // SEC-001: Validate pubkey format before use to prevent shell injection
    validate_pubkey(pubkey)?;

    // SEC-001: Use stdin to pass pubkey instead of shell interpolation
    let setup_script = "mkdir -p /home/polis/.ssh && chmod 700 /home/polis/.ssh && chown polis:polis /home/polis/.ssh";
    let install_script = "cat >> /home/polis/.ssh/authorized_keys && \
         chmod 600 /home/polis/.ssh/authorized_keys && \
         chown polis:polis /home/polis/.ssh/authorized_keys";

    // First ensure .ssh directory exists
    let setup_output = mp
        .exec(&["docker", "exec", CONTAINER_NAME, "bash", "-c", setup_script])
        .await
        .context("multipass exec failed")?;
    anyhow::ensure!(
        setup_output.status.success(),
        "failed to setup .ssh directory"
    );

    // Add newline if not present
    let key_line = if pubkey.ends_with('\n') {
        pubkey.as_bytes().to_vec()
    } else {
        format!("{pubkey}\n").into_bytes()
    };

    // Install pubkey via stdin (no shell interpolation)
    let output = mp
        .exec_with_stdin(
            &[
                "docker",
                "exec",
                "-i",
                CONTAINER_NAME,
                "bash",
                "-c",
                install_script,
            ],
            &key_line,
        )
        .await
        .context("multipass exec failed")?;

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
async fn pin_host_key(mp: &impl crate::multipass::Multipass) {
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
        let mgr = crate::ssh::KnownHostsManager::new();
        let _ = mgr.and_then(|m| write_host_key(&m, &key));
    }
}

async fn open_ide(ide: &str) -> Result<()> {
    let (cmd, args) = resolve_ide(ide)?;
    let status = tokio::process::Command::new(cmd)
        .args(args)
        .status()
        .await
        .with_context(|| format!("{cmd} is not installed or not in PATH"))?;
    anyhow::ensure!(status.success(), "{cmd} exited with failure");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_ide_vscode() {
        let (cmd, args) = resolve_ide("vscode").expect("should resolve");
        assert_eq!(cmd, "code");
        assert_eq!(args, &["--remote", "ssh-remote+workspace", "/workspace"]);
    }

    #[test]
    fn test_resolve_ide_code() {
        let (cmd, args) = resolve_ide("code").expect("should resolve");
        assert_eq!(cmd, "code");
        assert_eq!(args, &["--remote", "ssh-remote+workspace", "/workspace"]);
    }

    #[test]
    fn test_resolve_ide_cursor() {
        let (cmd, args) = resolve_ide("cursor").expect("should resolve");
        assert_eq!(cmd, "cursor");
        assert_eq!(args, &["--remote", "ssh-remote+workspace", "/workspace"]);
    }

    #[test]
    fn test_resolve_ide_unknown() {
        let result = resolve_ide("unknown-ide");
        assert!(result.is_err());
        assert!(
            result
                .expect_err("should fail")
                .to_string()
                .contains("Unknown IDE")
        );
    }

    #[test]
    fn test_resolve_ide_case_insensitive() {
        let (cmd, _) = resolve_ide("VsCode").expect("should resolve");
        assert_eq!(cmd, "code");
        let (cmd, _) = resolve_ide("CURSOR").expect("should resolve");
        assert_eq!(cmd, "cursor");
    }

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
