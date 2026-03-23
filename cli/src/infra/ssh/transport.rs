use anyhow::{Context, Result};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// SshTransport
// ---------------------------------------------------------------------------

/// Encapsulates SSH command construction for VM access.
///
/// Handles:
/// - Identity key path resolution
/// - Host key checking configuration
/// - Known hosts file path
/// - Log level suppression
/// - Batch mode for non-interactive use
pub struct SshTransport {
    identity_key: PathBuf,
}

impl SshTransport {
    /// Create a new SSH transport using the default identity key location.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let polis_dir = crate::infra::polis_dir::PolisDir::new()?;
        Ok(Self {
            identity_key: polis_dir.identity_key_path(),
        })
    }

    /// Spawn an SSH process to the VM with inherited STDIO.
    ///
    /// Uses `tokio::process::Command` for async-safe process spawning.
    ///
    /// # Arguments
    ///
    /// * `vm_ip` - IP address of the VM
    /// * `remote_command` - Command to execute on the VM
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The identity key path is not valid UTF-8
    /// - The SSH process cannot be spawned
    pub async fn spawn_inherited(
        &self,
        vm_ip: &str,
        remote_command: &str,
    ) -> Result<std::process::ExitStatus> {
        let identity_key_str = self
            .identity_key
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("SSH identity path is not valid UTF-8"))?;

        #[cfg(windows)]
        let devnull = "NUL";
        #[cfg(not(windows))]
        let devnull = "/dev/null";

        let user_known_hosts = format!("UserKnownHostsFile={devnull}");
        let user_host = format!("ubuntu@{vm_ip}");

        tokio::process::Command::new("ssh")
            .args([
                "-i",
                identity_key_str,
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                &user_known_hosts,
                "-o",
                "LogLevel=ERROR",
                "-o",
                "BatchMode=yes",
                &user_host,
                remote_command,
            ])
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .await
            .context("failed to spawn ssh")
    }
}
