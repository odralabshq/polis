//! Infrastructure implementation of the `WorkspaceStateStore` port.
//!
//! `StateManager` provides async load/save using `tokio::task::spawn_blocking`
//! with atomic write (temp file + rename) to prevent state corruption.

#![allow(dead_code)] // Refactor in progress — defined ahead of callers

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::application::ports::WorkspaceStateStore;
use crate::domain::workspace::{WorkspaceState, validate_workspace_id};

/// State file manager — implements `WorkspaceStateStore` for the infra layer.
pub struct StateManager {
    path: PathBuf,
}

impl StateManager {
    /// Create a state manager using the default path (`~/.polis/state.json`).
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        Ok(Self::with_path(home.join(".polis").join("state.json")))
    }

    /// Create a state manager with an explicit path (used in tests).
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Synchronous load — used internally by `load_async` via `spawn_blocking`.
    fn load_sync(&self) -> Result<Option<WorkspaceState>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading state file {}", self.path.display()))?;
        let state: WorkspaceState = serde_json::from_str(&content)
            .with_context(|| format!("parsing state file {}", self.path.display()))?;
        validate_workspace_id(&state.workspace_id)?;
        Ok(Some(state))
    }

    /// Synchronous save — used internally by `save_async` via `spawn_blocking`.
    fn save_sync(&self, state: &WorkspaceState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(state).context("serializing state")?;

        // Atomic write via temp file then rename (REL-001)
        let temp_path = self.path.with_extension("json.tmp");
        std::fs::write(&temp_path, &content)
            .with_context(|| format!("writing temp file {}", temp_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| format!("setting permissions on {}", temp_path.display()))?;
        }

        std::fs::rename(&temp_path, &self.path)
            .with_context(|| format!("finalizing state file {}", self.path.display()))?;

        Ok(())
    }

    /// Remove the state file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be removed.
    pub fn clear(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .with_context(|| format!("removing state file {}", self.path.display()))?;
        }
        Ok(())
    }
}

impl WorkspaceStateStore for StateManager {
    async fn load_async(&self) -> Result<Option<WorkspaceState>> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let mgr = StateManager::with_path(path);
            mgr.load_sync()
        })
        .await
        .context("state load task panicked")?
    }

    async fn save_async(&self, state: &WorkspaceState) -> Result<()> {
        let path = self.path.clone();
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let mgr = StateManager::with_path(path);
            mgr.save_sync(&state)
        })
        .await
        .context("state save task panicked")?
    }
}
