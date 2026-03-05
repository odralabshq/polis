//! `polis connect` — open an SSH session to the workspace with self-healing.

use anyhow::{Context, Result};
use clap::Args;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::services::ssh_provision::{self, SshProvisionOptions};
use crate::application::services::vm::lifecycle::{self as vm, VmState};
use crate::domain::error::WorkspaceError;

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
pub async fn run(app: &AppContext, args: ConnectArgs) -> Result<ExitCode> {
    // Req 8.5 — return WorkspaceError::NotRunning when VM is not Running.
    let vm_state = vm::state(&app.provisioner).await?;
    if vm_state != VmState::Running {
        return Err(WorkspaceError::NotRunning.into());
    }

    // Req 8.3 — --info flag: display connection strings and return.
    if args.info {
        show_connection_info(&app.output);
        return Ok(ExitCode::SUCCESS);
    }

    // Req 8.2 — self-healing SSH provisioning (consent always given for connect).
    let reporter = app.terminal_reporter();
    ssh_provision::provision_ssh(
        &app.provisioner,
        &app.ssh,
        SshProvisionOptions {
            consent_given: true,
        },
        &reporter,
    )
    .await?;

    // Open interactive SSH session.
    open_ssh_session()
}

/// Open an interactive SSH session to the workspace.
///
/// Uses `ssh workspace` which resolves via the `~/.ssh/config` entry written
/// by `provision_ssh`. Inherits stdin/stdout/stderr for a fully interactive
/// terminal.
///
/// # Errors
///
/// Returns an error if the `ssh` process cannot be spawned.
fn open_ssh_session() -> Result<ExitCode> {
    let status = std::process::Command::new("ssh")
        .arg("workspace")
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("failed to spawn ssh")?;

    let code = status.code().unwrap_or(255);
    #[allow(clippy::cast_possible_truncation)]
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(255)))
}

/// Print IDE connection strings.
fn show_connection_info(ctx: &crate::output::OutputContext) {
    ctx.blank();
    ctx.kv("SSH     ", "ssh workspace");
    ctx.kv("VS Code ", "code --remote ssh-remote+workspace /workspace");
    ctx.kv(
        "Cursor  ",
        "cursor --remote ssh-remote+workspace /workspace",
    );
}
