//! `polis delete [--all]` — remove workspace (and optionally cached images).

use anyhow::{Context, Result};

use crate::commands::DeleteArgs;
use crate::state::StateManager;
use crate::workspace::WorkspaceDriver;

/// Run `polis delete [--all]`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be removed or state cannot be cleared.
pub fn run(args: &DeleteArgs, state_mgr: &StateManager, driver: &dyn WorkspaceDriver) -> Result<()> {
    if args.all {
        delete_all(state_mgr, driver)
    } else {
        delete_workspace(state_mgr, driver)
    }
}

/// Prompt for confirmation, reading from stdin (works with both TTY and piped input).
///
/// # Errors
///
/// Returns an error if stdin cannot be read or is closed (EOF).
fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{BufRead, Write};
    print!("{prompt} [y/N]: ");
    std::io::stdout().flush().context("flushing stdout")?;
    let mut line = String::new();
    let n = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading confirmation")?;
    anyhow::ensure!(n > 0, "no input provided (stdin closed)");
    Ok(line.trim().eq_ignore_ascii_case("y"))
}

fn delete_workspace(state_mgr: &StateManager, driver: &dyn WorkspaceDriver) -> Result<()> {
    println!();
    println!("  This will remove the workspace and all agent data.");
    println!("  Configuration and cached images are preserved.");
    println!();

    if !confirm("Continue?")? {
        return Ok(());
    }

    if let Some(state) = state_mgr.load().context("reading workspace state")? {
        if driver.is_running(&state.workspace_id)? {
            driver.stop(&state.workspace_id)?;
        }
        driver.remove(&state.workspace_id)?;
    }

    state_mgr.clear().context("clearing state file")?;
    println!("Workspace removed");
    println!();
    println!("Run: polis run <agent>  to create a new workspace");

    Ok(())
}

fn delete_all(state_mgr: &StateManager, driver: &dyn WorkspaceDriver) -> Result<()> {
    println!();
    println!("  This will remove everything including cached images (~3.5 GB).");
    println!("  Only configuration is preserved.");
    println!();

    if !confirm("Continue?")? {
        return Ok(());
    }

    if let Some(state) = state_mgr.load().context("reading workspace state")? {
        if driver.is_running(&state.workspace_id)? {
            driver.stop(&state.workspace_id)?;
        }
        driver.remove(&state.workspace_id)?;
    }

    driver.remove_cached_images()?;
    state_mgr.clear().context("clearing state file")?;
    println!("All data removed");

    Ok(())
}
