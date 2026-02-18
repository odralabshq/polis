//! Workspace driver abstraction for start/stop/remove operations.

use anyhow::Result;

/// Abstracts workspace lifecycle operations so commands are testable.
pub trait WorkspaceDriver {
    /// Returns `true` if the workspace is currently running.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace state cannot be determined.
    fn is_running(&self, workspace_id: &str) -> Result<bool>;

    /// Start the workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace cannot be started.
    fn start(&self, workspace_id: &str) -> Result<()>;

    /// Stop the workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace cannot be stopped.
    fn stop(&self, workspace_id: &str) -> Result<()>;

    /// Remove the workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace cannot be removed.
    fn remove(&self, workspace_id: &str) -> Result<()>;

    /// Remove cached images (~3.5 GB).
    ///
    /// # Errors
    ///
    /// Returns an error if the cache cannot be removed.
    fn remove_cached_images(&self) -> Result<()>;
}

/// Production driver — delegates to `docker compose`.
///
/// All mutating operations are stubs until the provisioning layer is wired.
/// `is_running` conservatively returns `true` when the workspace state cannot
/// be determined (e.g. Docker unavailable), so the CLI never silently no-ops.
pub struct DockerDriver;

impl WorkspaceDriver for DockerDriver {
    fn is_running(&self, _workspace_id: &str) -> Result<bool> {
        // Stub: treat workspace as running until real docker compose check is wired.
        Ok(true)
    }

    fn start(&self, _workspace_id: &str) -> Result<()> {
        Ok(())
    }

    fn stop(&self, _workspace_id: &str) -> Result<()> {
        Ok(())
    }

    fn remove(&self, _workspace_id: &str) -> Result<()> {
        Ok(())
    }

    fn remove_cached_images(&self) -> Result<()> {
        Ok(())
    }
}
