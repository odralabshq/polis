//! `polis connect` — SSH config management and connection options.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;

use crate::app::App;
use crate::application::ports::SshConfigurator;
use crate::application::services::ssh::{self, SshProvisionOptions};
use crate::application::vm::lifecycle::{self as vm, VmState};
use crate::domain::error::WorkspaceError;
use crate::output::models::ConnectionInfo;

/// Arguments for the connect command.
#[derive(Args)]
pub struct ConnectArgs {
    /// Display connection strings as JSON.
    #[arg(long)]
    pub info: bool,
}

/// Run `polis connect`.
///
/// Ensures SSH is configured, validates permissions, provisions keys,
/// then prints connection options (SSH, VS Code, Cursor).
///
/// # Errors
///
/// Returns an error if the VM is not running, SSH config setup fails,
/// or permissions are unsafe.
pub async fn run(app: &impl App, args: &ConnectArgs) -> Result<ExitCode> {
    // Ensure the workspace is running.
    let vm_state = vm::state(app.provisioner()).await?;
    if vm_state != VmState::Running {
        return Err(WorkspaceError::NotRunning.into());
    }

    // Self-healing SSH provisioning.
    let reporter = app.terminal_reporter();
    let ssh_configured = app.ssh().is_configured().await?;

    let consent = if ssh_configured {
        true
    } else {
        app.confirm("Add SSH configuration to ~/.ssh/config?", true)?
    };

    ssh::provision_ssh(
        app.provisioner(),
        app.ssh(),
        SshProvisionOptions {
            consent_given: consent,
        },
        &reporter,
    )
    .await?;

    // Show connection options.
    let info = ConnectionInfo::default();

    if args.info {
        app.renderer().render_connection_info(&info)?;
    } else {
        let ctx = app.output();
        if ssh_configured {
            ctx.success("workspace ready to connect");
        } else {
            ctx.success("workspace connected");
        }
        ctx.blank();
        ctx.kv("SSH     ", &info.ssh);
        ctx.kv("VS Code ", &info.vscode);
        ctx.kv("Cursor  ", &info.cursor);
    }

    Ok(ExitCode::SUCCESS)
}
