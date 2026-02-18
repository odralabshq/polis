//! `polis stop` — stop a running workspace, preserving all data.

use anyhow::{Context, Result};

use crate::state::StateManager;
use crate::workspace::WorkspaceDriver;

/// Run `polis stop`.
///
/// # Errors
///
/// Returns an error if no workspace exists or the workspace cannot be stopped.
pub fn run(state_mgr: &StateManager, driver: &dyn WorkspaceDriver) -> Result<()> {
    let state = state_mgr
        .load()
        .context("reading workspace state")?
        .ok_or_else(|| anyhow::anyhow!("No workspace found."))?;

    if driver.is_running(&state.workspace_id)? {
        println!("Stopping workspace...");
        driver.stop(&state.workspace_id)?;
    }

    // Always print final state — accurate after stop completes.
    println!("Workspace is not running");
    println!();
    println!("Your data is preserved. Run: polis start");

    Ok(())
}
