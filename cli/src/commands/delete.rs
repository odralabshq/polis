//! `polis delete [--all]` — remove workspace.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::commands::DeleteArgs;
use crate::multipass::Multipass;
use crate::state::StateManager;
use crate::workspace::{image, vm};

/// Run `polis delete [--all]`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be removed.
pub fn run(args: &DeleteArgs, mp: &impl Multipass, quiet: bool) -> Result<()> {
    if args.all {
        delete_all(args, mp, quiet)
    } else {
        delete_workspace(args, mp, quiet)
    }
}

fn delete_workspace(args: &DeleteArgs, mp: &impl Multipass, quiet: bool) -> Result<()> {
    if !quiet {
        println!();
        println!("This will remove your workspace.");
        println!("Configuration, certificates, and cached downloads will be preserved.");
        println!();
    }

    if !args.yes && !confirm("Continue?")? {
        println!("Cancelled.");
        return Ok(());
    }

    // Stop and delete VM
    if vm::exists(mp)? {
        if !quiet {
            println!("Removing workspace...");
        }
        vm::delete(mp)?;
    }

    // Clear state file
    let state_mgr = StateManager::new()?;
    state_mgr.clear()?;

    if !quiet {
        println!();
        println!("Workspace removed.");
        println!();
        println!("Create new: polis start");
    }

    Ok(())
}

fn delete_all(args: &DeleteArgs, mp: &impl Multipass, quiet: bool) -> Result<()> {
    if !quiet {
        println!();
        println!("This will permanently remove:");
        println!("  • Your workspace");
        println!("  • Generated certificates");
        println!("  • Configuration");
        println!("  • Cached workspace image (~3.5 GB)");
        println!();
    }

    if !args.yes && !confirm("Continue?")? {
        println!("Cancelled.");
        return Ok(());
    }

    // Stop and delete VM
    if vm::exists(mp)? {
        if !quiet {
            println!("Removing workspace...");
        }
        vm::delete(mp)?;
    }

    // Clear state file
    let state_mgr = StateManager::new()?;
    state_mgr.clear()?;

    // Remove certificates
    if !quiet {
        println!("Removing certificates...");
    }
    remove_certificates()?;

    // Remove SSH config and known_hosts
    remove_ssh_config()?;
    remove_known_hosts()?;

    // Remove configuration
    if !quiet {
        println!("Removing configuration...");
    }
    remove_config()?;

    // Remove cached images
    if !quiet {
        println!("Removing cached data...");
    }
    remove_cached_images()?;

    if !quiet {
        println!();
        println!("All Polis data removed.");
        println!();
        println!("Start fresh: polis start");
    }

    Ok(())
}

// --- Helpers ---

fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{BufRead, Write};
    print!("{prompt} [y/N]: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    let n = std::io::stdin().lock().read_line(&mut line)?;
    anyhow::ensure!(n > 0, "no input provided");
    Ok(line.trim().eq_ignore_ascii_case("y"))
}

fn get_polis_dir() -> Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    Ok(home.join(".polis"))
}

fn remove_certificates() -> Result<()> {
    let certs_dir = get_polis_dir()?.join("certs");
    if certs_dir.exists() {
        std::fs::remove_dir_all(&certs_dir)
            .with_context(|| format!("removing {}", certs_dir.display()))?;
    }
    Ok(())
}

fn remove_ssh_config() -> Result<()> {
    let ssh_config = get_polis_dir()?.join("ssh_config");
    if ssh_config.exists() {
        std::fs::remove_file(&ssh_config)?;
    }
    let sockets_dir = get_polis_dir()?.join("sockets");
    if sockets_dir.exists() {
        let _ = std::fs::remove_dir_all(&sockets_dir);
    }
    Ok(())
}

fn remove_known_hosts() -> Result<()> {
    crate::ssh::KnownHostsManager::new()?.remove()
}

fn remove_config() -> Result<()> {
    let config_path = get_polis_dir()?.join("config.yaml");
    if config_path.exists() {
        std::fs::remove_file(&config_path)
            .with_context(|| format!("removing {}", config_path.display()))?;
    }
    Ok(())
}

fn remove_cached_images() -> Result<()> {
    let images_dir = image::images_dir()?;
    if images_dir.exists() {
        std::fs::remove_dir_all(&images_dir)
            .with_context(|| format!("removing {}", images_dir.display()))?;
    }
    Ok(())
}
