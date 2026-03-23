//! Infrastructure implementation of the `ProcessLauncher` port.

use anyhow::{Context, Result};

use crate::application::ports::ProcessLauncher;

/// Launches real OS processes via `tokio::process::Command`.
pub struct OsProcessLauncher;

impl ProcessLauncher for OsProcessLauncher {
    async fn launch(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        tokio::process::Command::new(program)
            .args(args)
            .status()
            .await
            .context("failed to spawn process")
    }
}
