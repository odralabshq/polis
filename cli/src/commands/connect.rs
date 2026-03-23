//! `polis connect` — open an SSH session to the workspace with self-healing.

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args;
use std::process::Stdio;

use crate::app::App;
use crate::application::services::ssh::{self, SshProvisionOptions};
use crate::application::vm::lifecycle::{self as vm, VmState};
use crate::domain::error::WorkspaceError;
use crate::domain::process::exit_code_from_status;
use crate::output::models::ConnectionInfo;

/// Arguments for the connect command.
#[derive(Args)]
pub struct ConnectArgs {
    /// Display IDE connection strings without opening an SSH session.
    #[arg(long)]
    pub info: bool,
}

/// Run `polis connect`.
///
/// Checks that the workspace is running, runs self-healing SSH provisioning,
/// then either prints connection info (`--info`) or opens an interactive SSH
/// session.
///
/// # Errors
///
/// Returns an error if the VM is not running, SSH provisioning fails, or the
/// SSH process cannot be spawned.
pub async fn run(app: &impl App, args: &ConnectArgs) -> Result<ExitCode> {
    // Req 8.5 — return WorkspaceError::NotRunning when VM is not Running.
    let vm_state = vm::state(app.provisioner()).await?;
    if vm_state != VmState::Running {
        return Err(WorkspaceError::NotRunning.into());
    }

    // Req 8.3 — --info flag: display connection strings and return.
    if args.info {
        app.renderer()
            .render_connection_info(&ConnectionInfo::default())?;
        return Ok(ExitCode::SUCCESS);
    }

    // Req 8.2 — self-healing SSH provisioning (consent always given for connect).
    let reporter = app.terminal_reporter();
    ssh::provision_ssh(
        app.provisioner(),
        app.ssh(),
        SshProvisionOptions {
            consent_given: true,
        },
        &reporter,
    )
    .await?;

    // Open interactive SSH session.
    open_ssh_session().await
}

/// Open an interactive SSH session to the workspace (async).
///
/// Uses `ssh workspace` which resolves via the `~/.ssh/config` entry written
/// by `provision_ssh`. Inherits stdin/stdout/stderr for a fully interactive
/// terminal.
///
/// # Errors
///
/// Returns an error if the `ssh` process cannot be spawned.
async fn open_ssh_session() -> Result<ExitCode> {
    let status = tokio::process::Command::new("ssh")
        .arg("workspace")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("failed to spawn ssh")?;

    Ok(exit_code_from_status(status))
}
