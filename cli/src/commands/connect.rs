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

#[cfg(test)]
mod proptests {
    use super::resolve_ide;
    use proptest::prelude::*;

    proptest! {
        /// Every recognised IDE name (in any case) resolves successfully.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_resolve_ide_known_names_always_ok(
            name in proptest::sample::select(vec![
                "vscode", "VSCODE", "VsCode", "vSCODE",
                "code",   "CODE",   "Code",
                "cursor", "CURSOR", "Cursor",
            ])
        ) {
            prop_assert!(resolve_ide(name).is_ok());
        }

        /// Any lowercase string that is not a known IDE name always returns Err.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_resolve_ide_unknown_names_always_err(name in "[a-z]{1,20}") {
            prop_assume!(!matches!(name.as_str(), "vscode" | "code" | "cursor"));
            prop_assert!(resolve_ide(&name).is_err());
        }

        /// A successful resolution always yields a non-empty command string.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_resolve_ide_ok_cmd_is_non_empty(
            name in proptest::sample::select(vec!["vscode", "code", "cursor"])
        ) {
            let (cmd, _) = resolve_ide(name).expect("known name resolves");
            prop_assert!(!cmd.is_empty());
        }

        /// A successful resolution always yields at least one argument.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_resolve_ide_ok_args_are_non_empty(
            name in proptest::sample::select(vec!["vscode", "code", "cursor"])
        ) {
            let (_, args) = resolve_ide(name).expect("known name resolves");
            prop_assert!(!args.is_empty());
        }
    }
}
