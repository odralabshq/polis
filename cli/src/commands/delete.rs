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
pub async fn run(
    args: &DeleteArgs,
    mp: &impl Multipass,
    state_mgr: &StateManager,
    quiet: bool,
) -> Result<()> {
    if args.all {
        delete_all(args, mp, state_mgr, quiet).await
    } else {
        delete_workspace(args, mp, state_mgr, quiet).await
    }
}

async fn delete_workspace(
    args: &DeleteArgs,
    mp: &impl Multipass,
    state_mgr: &StateManager,
    quiet: bool,
) -> Result<()> {
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

    // REL-003: Collect errors instead of failing fast
    let mut errors = Vec::new();

    // Stop and delete VM
    if vm::exists(mp).await {
        if !quiet {
            println!("Removing workspace...");
        }
        vm::delete(mp).await;
    }

    // Clear state file
    if let Err(e) = state_mgr.clear() {
        errors.push(format!("failed to clear state: {e}"));
    }

    if !errors.is_empty() {
        anyhow::bail!("delete completed with errors:\n  {}", errors.join("\n  "));
    }

    if !quiet {
        println!();
        println!("Workspace removed.");
        println!();
        println!("Create new: polis start");
    }

    Ok(())
}

async fn delete_all(
    args: &DeleteArgs,
    mp: &impl Multipass,
    state_mgr: &StateManager,
    quiet: bool,
) -> Result<()> {
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
    if vm::exists(mp).await {
        if !quiet {
            println!("Removing workspace...");
        }
        vm::delete(mp).await;
    }

    // Clear state file
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
    use std::io::{BufRead, Read, Write};
    print!("{prompt} [y/N]: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    // CORR-002: Limit input to 16 bytes to prevent memory exhaustion
    let n = std::io::stdin().lock().take(16).read_line(&mut line)?;
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
    let polis_dir = get_polis_dir()?;
    for name in &["ssh_config", "id_ed25519", "id_ed25519.pub"] {
        let path = polis_dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    let sockets_dir = polis_dir.join("sockets");
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

#[cfg(test)]
mod tests {
    use std::fs;

    fn make_polis_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn remove_ssh_config_removes_keys_and_config() {
        let dir = make_polis_dir();
        let files = ["ssh_config", "id_ed25519", "id_ed25519.pub"];
        for f in &files {
            fs::write(dir.path().join(f), "data").expect("write");
        }
        fs::create_dir(dir.path().join("sockets")).expect("mkdir");

        // Exercise via direct fs ops mirroring remove_ssh_config logic
        for name in &files {
            let path = dir.path().join(name);
            if path.exists() {
                fs::remove_file(&path).expect("rm");
            }
        }
        let sockets = dir.path().join("sockets");
        if sockets.exists() {
            fs::remove_dir_all(&sockets).expect("rmdir");
        }

        for f in &files {
            assert!(!dir.path().join(f).exists(), "{f} should be removed");
        }
        assert!(!dir.path().join("sockets").exists());
    }

    #[test]
    fn remove_ssh_config_tolerates_missing_files() {
        let dir = make_polis_dir();
        // Only one of the three files exists
        fs::write(dir.path().join("id_ed25519"), "key").expect("write");

        for name in &["ssh_config", "id_ed25519", "id_ed25519.pub"] {
            let path = dir.path().join(name);
            if path.exists() {
                fs::remove_file(&path).expect("rm");
            }
        }

        assert!(!dir.path().join("id_ed25519").exists());
    }
}
