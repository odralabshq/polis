//! `polis start` — start an existing stopped workspace.

use anyhow::{Context, Result};

use crate::state::StateManager;
use crate::workspace::WorkspaceDriver;

/// Run `polis start`.
///
/// # Errors
///
/// Returns an error if no workspace exists or the workspace cannot be started.
pub fn run(state_mgr: &StateManager, driver: &dyn WorkspaceDriver) -> Result<()> {
    let state = state_mgr
        .load()
        .context("reading workspace state")?
        .ok_or_else(|| anyhow::anyhow!("No workspace found. Run: polis run <agent>"))?;

    if driver.is_running(&state.workspace_id)? {
        println!("Workspace is already running");
        println!();
        println!("Run: polis status");
        return Ok(());
    }

    println!("Starting workspace...");
    driver.start(&state.workspace_id)?;
    println!("Workspace started");
    println!();
    println!("Run: polis status");

    Ok(())
}
