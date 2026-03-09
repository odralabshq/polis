//! Infrastructure implementation of the `WorkspaceStateStore` port.
//!
//! `StateManager` provides async load/save using `spawn_blocking_io`
//! with atomic write (temp file + rename) to prevent state corruption.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::application::ports::WorkspaceStateStore;
use crate::domain::workspace::WorkspaceState;
use crate::infra::blocking::spawn_blocking_io;
use crate::infra::secure_fs::SecureFs;

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
        let polis_dir = crate::infra::polis_dir::PolisDir::new()?;
        Ok(Self::with_path(polis_dir.state_path()))
    }

    /// Create a state manager with an explicit path (used in tests).
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load existing state, if any.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(&self) -> Result<Option<WorkspaceState>> {
        self.load_sync()
    }

    /// Save state to disk with mode 600 using atomic write.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or the file cannot be written.
    pub fn save(&self, state: &WorkspaceState) -> Result<()> {
        self.save_sync(state)
    }

    /// Synchronous load — used internally by `load_async` via `spawn_blocking`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    fn load_sync(&self) -> Result<Option<WorkspaceState>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading state file {}", self.path.display()))?;
        let state: WorkspaceState = serde_json::from_str(&content)
            .with_context(|| format!("parsing state file {}", self.path.display()))?;
        Ok(Some(state))
    }

    /// Synchronous save — used internally by `save_async` via `spawn_blocking`.
    ///
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
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

        SecureFs::set_permissions(&temp_path, 0o600)?;

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
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    async fn load_async(&self) -> Result<Option<WorkspaceState>> {
        let path = self.path.clone();
        spawn_blocking_io("state load", move || {
            let mgr = StateManager::with_path(path);
            mgr.load_sync()
        })
        .await
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    async fn save_async(&self, state: &WorkspaceState) -> Result<()> {
        let path = self.path.clone();
        let state = state.clone();
        spawn_blocking_io("state save", move || {
            let mgr = StateManager::with_path(path);
            mgr.save_sync(&state)
        })
        .await
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    async fn clear_async(&self) -> Result<()> {
        let path = self.path.clone();
        spawn_blocking_io("state clear", move || {
            let mgr = StateManager::with_path(path);
            mgr.clear()
        })
        .await
    }
}
