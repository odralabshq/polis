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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::state::test_helpers::state_mgr_with_state;
    use crate::workspace::MockDriver;
    use tempfile::TempDir;

    #[test]
    fn test_stop_already_stopped_exits_ok() {
        let dir = TempDir::new().expect("tempdir");
        let mgr = state_mgr_with_state(&dir);
        let driver = MockDriver { running: false };

        let result = run(&mgr, &driver);
        assert!(result.is_ok());
    }

    #[test]
    fn test_stop_running_workspace_exits_ok() {
        let dir = TempDir::new().expect("tempdir");
        let mgr = state_mgr_with_state(&dir);
        let driver = MockDriver { running: true };

        let result = run(&mgr, &driver);
        assert!(result.is_ok());
    }

    #[test]
    fn test_stop_preserves_state_file() {
        let dir = TempDir::new().expect("tempdir");
        let mgr = state_mgr_with_state(&dir);
        let driver = MockDriver { running: true };

        run(&mgr, &driver).expect("stop should succeed");

        // State file must still exist — stop preserves data
        let reloaded = mgr.load().expect("load").expect("state should exist");
        assert_eq!(reloaded.workspace_id, "ws-test01");
    }
}
