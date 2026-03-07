//! Standalone SSH provisioning service.
//!
//! Ensures SSH connectivity to the workspace by generating keys, configuring
//! `~/.ssh/config`, and installing pubkeys. All operations are idempotent.
//!
//! This module contains no dialoguer imports and no references to
//! `non_interactive` — consent is passed as a boolean by the presentation layer.

use crate::application::ports::{ProgressReporter, ShellExecutor, SshConfigurator};
use crate::domain::ssh::{validate_host_key, validate_pubkey};
use crate::domain::workspace::CONTAINER_NAME;
use anyhow::{Context, Result};

/// Options for SSH provisioning.
pub struct SshProvisionOptions {
    /// Whether the user has consented to SSH configuration changes.
    /// Decided by the presentation layer — never read from env or prompts here.
    pub consent_given: bool,
}

/// Outcome of an SSH provisioning call.
pub struct SshProvisionOutcome {
    /// `true` when SSH was not previously configured and was set up during this
    /// call. `false` when SSH was already configured or consent was not given.
    pub was_first_setup: bool,
}

/// Provision SSH connectivity to the workspace.
///
/// # Behaviour
///
/// - **Already configured**: refreshes the SSH config template (idempotent) and
///   installs pubkeys.
/// - **Not configured + `consent_given` is `false`**: returns immediately with
///   `was_first_setup: false` and performs no side effects.
/// - **Not configured + `consent_given` is `true`**: runs full setup — config,
///   identity key, pubkey installation, and host key pinning.
///
/// All operations are idempotent and safe to call multiple times.
///
/// # Errors
///
/// Returns an error if any underlying SSH or VM operation fails.
pub async fn provision_ssh(
    provisioner: &impl ShellExecutor,
    ssh: &impl SshConfigurator,
    opts: SshProvisionOptions,
    reporter: &impl ProgressReporter,
) -> Result<SshProvisionOutcome> {
    let already_configured = ssh.is_configured().await?;

    if already_configured {
        // Refresh the config template idempotently.
        ssh.setup_config().await?;
    } else {
        if !opts.consent_given {
            return Ok(SshProvisionOutcome {
                was_first_setup: false,
            });
        }
        reporter.step("configuring SSH...");
        ssh.setup_config().await?;
    }

    ssh.validate_permissions().await?;

    // Generate or reuse the identity key.
    let pubkey = ssh.ensure_identity().await?;

    // Install into VM and workspace container.
    install_vm_pubkey(provisioner, &pubkey).await?;
    install_pubkey(provisioner, &pubkey).await?;

    // Pin the host key (best-effort — failure is logged but not fatal).
    pin_host_key(provisioner, ssh).await;

    Ok(SshProvisionOutcome {
        was_first_setup: !already_configured,
    })
}

/// Installs `pubkey` into `~/.ssh/authorized_keys` of the VM's `ubuntu` user.
///
/// # Errors
///
/// Returns an error if the underlying VM operation fails.
pub(crate) async fn install_vm_pubkey(mp: &impl ShellExecutor, pubkey: &str) -> Result<()> {
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
/// Returns an error if the underlying VM operation fails.
pub(crate) async fn install_pubkey(mp: &impl ShellExecutor, pubkey: &str) -> Result<()> {
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
/// given SSH configurator. Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns an error if the key is empty, invalid, or the write fails.
pub(crate) async fn write_host_key(ssh: &impl SshConfigurator, raw_key: &str) -> Result<()> {
    let trimmed = raw_key.trim();
    anyhow::ensure!(!trimmed.is_empty(), "empty host key");
    validate_host_key(trimmed)?;
    let host_key = format!("workspace {trimmed}");
    ssh.update_host_key(&host_key).await
}

/// Extracts the workspace SSH host key and writes it to `~/.polis/known_hosts`.
///
/// This is best-effort — failures are silently ignored so that SSH provisioning
/// is not blocked by a missing or unreadable host key.
pub(crate) async fn pin_host_key(mp: &impl ShellExecutor, ssh: &impl SshConfigurator) {
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::process::Output;
    use anyhow::Result;
    use super::*;
    use crate::application::ports::ShellExecutor;
    use crate::application::vm::test_support::{
        impl_shell_executor_stubs, ok_output, fail_output, NoopReporter, SshConfiguratorStub,
    };

    struct ShellStub(bool); // true = success
    impl ShellExecutor for ShellStub {
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            if self.0 { Ok(ok_output(b"")) } else { Ok(fail_output()) }
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    #[tokio::test]
    async fn provision_ssh_already_configured_skips_first_setup() {
        let mp = ShellStub(true);
        let ssh = SshConfiguratorStub::configured();
        let out = provision_ssh(&mp, &ssh, SshProvisionOptions { consent_given: true }, &NoopReporter).await.unwrap();
        assert!(!out.was_first_setup);
    }

    #[tokio::test]
    async fn provision_ssh_consent_false_returns_early() {
        let mp = ShellStub(true);
        let ssh = SshConfiguratorStub::unconfigured();
        let out = provision_ssh(&mp, &ssh, SshProvisionOptions { consent_given: false }, &NoopReporter).await.unwrap();
        assert!(!out.was_first_setup);
    }

    #[tokio::test]
    async fn provision_ssh_full_setup_when_unconfigured_and_consent_given() {
        let mp = ShellStub(true);
        let ssh = SshConfiguratorStub::unconfigured();
        let out = provision_ssh(&mp, &ssh, SshProvisionOptions { consent_given: true }, &NoopReporter).await.unwrap();
        assert!(out.was_first_setup);
    }

    #[tokio::test]
    async fn install_vm_pubkey_rejects_invalid_pubkey() {
        let mp = ShellStub(true);
        assert!(install_vm_pubkey(&mp, "not-a-key").await.is_err());
    }

    #[tokio::test]
    async fn install_vm_pubkey_accepts_valid_key() {
        let mp = ShellStub(true);
        assert!(install_vm_pubkey(&mp, "ssh-ed25519 AAAA test@host").await.is_ok());
    }

    #[tokio::test]
    async fn write_host_key_rejects_empty() {
        let ssh = SshConfiguratorStub::configured();
        assert!(write_host_key(&ssh, "").await.is_err());
        assert!(write_host_key(&ssh, "   ").await.is_err());
    }

    #[tokio::test]
    async fn write_host_key_accepts_valid_key() {
        let ssh = SshConfiguratorStub::configured();
        assert!(write_host_key(&ssh, "ssh-ed25519 AAAA test@host").await.is_ok());
    }
}
