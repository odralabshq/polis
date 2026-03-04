use crate::application::ports::{ShellExecutor, SshConfigurator};
use crate::domain::workspace::CONTAINER_NAME;
use anyhow::{Context, Result};

/// Validates that a public key has a safe format for use in shell commands.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub fn validate_pubkey(key: &str) -> Result<()> {
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
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn install_vm_pubkey(mp: &impl ShellExecutor, pubkey: &str) -> Result<()> {
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
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn install_pubkey(mp: &impl ShellExecutor, pubkey: &str) -> Result<()> {
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
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn write_host_key(ssh: &impl SshConfigurator, raw_key: &str) -> Result<()> {
    let trimmed = raw_key.trim();
    anyhow::ensure!(!trimmed.is_empty(), "empty host key");
    crate::domain::ssh::validate_host_key(trimmed)?;
    let host_key = format!("workspace {trimmed}");
    ssh.update_host_key(&host_key).await
}

/// Extracts the workspace SSH host key and writes it to `~/.polis/known_hosts`.
pub async fn pin_host_key(mp: &impl ShellExecutor, ssh: &impl SshConfigurator) {
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
