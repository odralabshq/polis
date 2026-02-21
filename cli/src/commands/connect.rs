//! `polis connect` — SSH config management and IDE integration.

use anyhow::{Context, Result};
use clap::Args;

use crate::output::OutputContext;
use crate::ssh::SshConfigManager;

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
pub async fn run(ctx: &OutputContext, args: ConnectArgs) -> Result<()> {
    // Validate IDE name early — fail fast before any interactive prompts.
    if let Some(ref ide) = args.ide {
        resolve_ide(ide)?;
    }

    let ssh_mgr = SshConfigManager::new()?;

    if !ssh_mgr.is_configured()? {
        setup_ssh_config(&ssh_mgr)?;
    }

    ssh_mgr.validate_permissions()?;

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
