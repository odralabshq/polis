//! Workspace state persistence.
//!
//! Domain types have moved to `crate::domain::workspace`. This module
//! re-exports them for backward compatibility and provides the I/O layer
//! (`StateManager`) for reading and writing state to disk.

use crate::domain::workspace::{WorkspaceState, validate_workspace_id};

use anyhow::{Context, Result};
use std::path::PathBuf;

/// State file manager.
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

    /// Load existing state, if any.
    ///
    /// Handles migration from old state format by ignoring unknown fields
    /// and aliasing `started_at` to `created_at`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed,
    /// or if the workspace ID fails validation.
    pub fn load(&self) -> Result<Option<WorkspaceState>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading state file {}", self.path.display()))?;
        let state: WorkspaceState = serde_json::from_str(&content)
            .with_context(|| format!("parsing state file {}", self.path.display()))?;
        // SEC-004: Validate workspace ID from external source
        validate_workspace_id(&state.workspace_id)?;
        Ok(Some(state))
    }

    /// Save state to disk with mode 600 using atomic write.
    ///
    /// REL-001: Uses temp-file-then-rename pattern to prevent corruption.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or the file cannot be written.
    pub fn save(&self, state: &WorkspaceState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(state).context("serializing state")?;

        // REL-001: Atomic write via temp file then rename
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    /// Valid 22-character workspace ID for tests (polis- + 16 hex chars).
    const TEST_WORKSPACE_ID: &str = "polis-0123456789abcdef";

    fn mgr(dir: &TempDir) -> StateManager {
        StateManager::with_path(dir.path().join("state.json"))
    }

    #[test]
    fn test_workspace_state_deserialize_new_format() {
        let json = r#"{
            "workspace_id": "polis-0123456789abcdef",
            "created_at": "2026-02-17T14:30:00Z",
            "image_sha256": "abc123"
        }"#;
        let state: WorkspaceState = serde_json::from_str(json).expect("deserialize");
        assert_eq!(state.workspace_id, TEST_WORKSPACE_ID);
        assert_eq!(state.image_sha256.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_workspace_state_deserialize_legacy_format() {
        // Old format with stage, agent, started_at
        let json = r#"{
            "stage": "agent_ready",
            "agent": "claude-dev",
            "workspace_id": "polis-fedcba9876543210",
            "started_at": "2026-02-17T14:30:00Z",
            "image_sha256": "abc123"
        }"#;
        let state: WorkspaceState = serde_json::from_str(json).expect("deserialize");
        assert_eq!(state.workspace_id, "polis-fedcba9876543210");
        // started_at should be aliased to created_at
        assert!(state.created_at.to_rfc3339().contains("2026-02-17"));
    }

    #[test]
    fn test_state_manager_load_returns_none_when_no_file() {
        let dir = TempDir::new().expect("tempdir");
        assert!(mgr(&dir).load().expect("load").is_none());
    }

    #[test]
    fn test_state_manager_save_load_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        let state = WorkspaceState {
            workspace_id: TEST_WORKSPACE_ID.to_string(),
            created_at: Utc::now(),
            image_sha256: Some("abc123".to_string()),
            image_source: None,
            active_agent: None,
        };
        m.save(&state).expect("save");
        let loaded = m.load().expect("load").expect("state present");
        assert_eq!(loaded.workspace_id, state.workspace_id);
    }

    #[test]
    fn test_state_manager_clear_removes_file() {
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        let state = WorkspaceState {
            workspace_id: TEST_WORKSPACE_ID.to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: None,
        };
        m.save(&state).expect("save");
        m.clear().expect("clear");
        assert!(m.load().expect("load").is_none());
    }

    #[test]
    fn test_workspace_state_active_agent_defaults_to_none() {
        let json = r#"{"workspace_id":"polis-abc","created_at":"2026-02-17T14:30:00Z"}"#;
        let state: WorkspaceState = serde_json::from_str(json).expect("deserialize");
        assert!(state.active_agent.is_none());
    }

    #[test]
    fn test_workspace_state_active_agent_omitted_when_none() {
        let state = WorkspaceState {
            workspace_id: "polis-test".to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: None,
        };
        let json = serde_json::to_string(&state).expect("serialize");
        assert!(!json.contains("active_agent"));
    }

    #[test]
    fn test_workspace_state_active_agent_roundtrip() {
        let state = WorkspaceState {
            workspace_id: "polis-test".to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: Some("openclaw".to_string()),
        };
        let json = serde_json::to_string(&state).expect("serialize");
        assert!(json.contains("active_agent"));
        let loaded: WorkspaceState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(loaded.active_agent.as_deref(), Some("openclaw"));
    }

    #[test]
    fn test_state_manager_load_returns_error_on_corrupted_json() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"not valid json").expect("write corrupt file");
        let result = StateManager::with_path(path).load();
        assert!(result.is_err(), "corrupted JSON must return Err");
    }

    #[test]
    fn test_state_manager_save_creates_parent_directory() {
        let dir = TempDir::new().expect("tempdir");
        let nested = dir.path().join("a").join("b").join("state.json");
        let state = WorkspaceState {
            workspace_id: TEST_WORKSPACE_ID.to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: None,
        };
        StateManager::with_path(nested.clone())
            .save(&state)
            .expect("save should create missing parent dirs");
        assert!(nested.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_state_manager_save_sets_600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        let state = WorkspaceState {
            workspace_id: TEST_WORKSPACE_ID.to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: None,
            active_agent: None,
        };
        m.save(&state).expect("save");
        let perms = std::fs::metadata(dir.path().join("state.json"))
            .expect("metadata")
            .permissions();
        assert_eq!(perms.mode() & 0o777, 0o600, "state file must be mode 600");
    }
}
