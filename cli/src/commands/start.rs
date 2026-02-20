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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::state::test_helpers::state_mgr_with_state;
    use crate::workspace::MockDriver;
    use tempfile::TempDir;

    #[test]
    fn test_start_already_running_shows_already_running_and_exits_ok() {
        let dir = TempDir::new().expect("tempdir");
        let mgr = state_mgr_with_state(&dir);
        let driver = MockDriver { running: true };

        let result = run(&mgr, &driver);
        assert!(result.is_ok());
    }

    #[test]
    fn test_start_stopped_workspace_exits_ok() {
        let dir = TempDir::new().expect("tempdir");
        let mgr = state_mgr_with_state(&dir);
        let driver = MockDriver { running: false };

        let result = run(&mgr, &driver);
        assert!(result.is_ok());
    }
}
