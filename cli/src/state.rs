//! Run state persistence for checkpoint/resume.

use anyhow::{Context, Result};
use polis_common::types::{RunStage, RunState};
use std::path::PathBuf;

/// State file manager for checkpoint/resume.
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
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        Ok(Self::with_path(home.join(".polis").join("state.json")))
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
    pub fn load(&self) -> Result<Option<RunState>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading state file {}", self.path.display()))?;
        let state: RunState = serde_json::from_str(&content)
            .with_context(|| format!("parsing state file {}", self.path.display()))?;
        Ok(Some(state))
    }

    /// Save state to disk with mode 600.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or the file cannot be written.
    pub fn save(&self, state: &RunState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(state).context("serializing state")?;
        std::fs::write(&self.path, &content)
            .with_context(|| format!("writing state file {}", self.path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| format!("setting permissions on {}", self.path.display()))?;
        }
        Ok(())
    }

    /// Remove the state file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be removed.
    #[allow(dead_code)] // used in tests; will be called by `polis stop`/`polis delete`
    pub fn clear(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .with_context(|| format!("removing state file {}", self.path.display()))?;
        }
        Ok(())
    }

    /// Update the stage in `state`, then persist.
    ///
    /// # Errors
    ///
    /// Returns an error if the state cannot be saved.
    pub fn advance(&self, run_state: &mut RunState, next_stage: RunStage) -> Result<()> {
        run_state.stage = next_stage;
        self.save(run_state)
    }
}

/// Shared test helpers — available to all modules via `crate::state::test_helpers`.
#[cfg(test)]
pub mod test_helpers {
    use super::StateManager;
    use tempfile::TempDir;

    /// Creates a `StateManager` pre-loaded with a minimal `agent_ready` state fixture.
    pub fn state_mgr_with_state(dir: &TempDir) -> StateManager {
        let polis_dir = dir.path().join(".polis");
        std::fs::create_dir_all(&polis_dir).expect("create .polis dir");
        let state_path = polis_dir.join("state.json");
        std::fs::write(
            &state_path,
            r#"{"stage":"agent_ready","agent":"claude-dev","workspace_id":"ws-test01","started_at":"2026-02-17T14:30:00Z"}"#,
        )
        .expect("write state");
        StateManager::with_path(state_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use polis_common::types::{RunStage, RunState};
    use tempfile::TempDir;

    fn make_state() -> RunState {
        RunState {
            stage: RunStage::Provisioned,
            agent: "claude-dev".to_string(),
            workspace_id: "ws-abc123".to_string(),
            started_at: Utc::now(),
            image_sha256: None,
        }
    }

    fn mgr(dir: &TempDir) -> StateManager {
        StateManager::with_path(dir.path().join("state.json"))
    }

    #[test]
    fn test_state_manager_load_returns_none_when_no_file() {
        let dir = TempDir::new().expect("tempdir");
        let result = mgr(&dir)
            .load()
            .expect("load should not error on missing file");
        assert!(result.is_none());
    }

    #[test]
    fn test_state_manager_load_returns_state_when_file_exists() {
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        m.save(&make_state()).expect("save");
        let loaded = m.load().expect("load").expect("state should be present");
        assert_eq!(loaded.stage, RunStage::Provisioned);
        assert_eq!(loaded.agent, "claude-dev");
        assert_eq!(loaded.workspace_id, "ws-abc123");
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
    fn test_state_manager_load_image_ready_stage_deserializes_as_workspace_created() {
        // Backward-compat migration: state.json files written before issue 06
        // used "image_ready" as the stage value. The #[serde(alias)] on
        // WorkspaceCreated must make these load without error.
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            br#"{"stage":"image_ready","agent":"claude-dev","workspace_id":"ws-abc123","started_at":"2026-02-17T14:30:00Z"}"#,
        )
        .expect("write legacy state");
        let loaded = StateManager::with_path(path)
            .load()
            .expect("load must not error")
            .expect("state must be present");
        assert_eq!(
            loaded.stage,
            RunStage::WorkspaceCreated,
            "image_ready must deserialize as WorkspaceCreated"
        );
    }

    #[test]
    fn test_state_manager_save_creates_parent_directory() {
        let dir = TempDir::new().expect("tempdir");
        let nested = dir.path().join("a").join("b").join("state.json");
        StateManager::with_path(nested.clone())
            .save(&make_state())
            .expect("save should create missing parent dirs");
        assert!(nested.exists());
    }

    #[test]
    fn test_state_manager_save_persists_all_fields() {
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        let state = RunState {
            stage: RunStage::CredentialsSet,
            agent: "gpt-dev".to_string(),
            workspace_id: "ws-xyz999".to_string(),
            started_at: Utc::now(),
            image_sha256: Some("abc123def456".to_string()),
        };
        m.save(&state).expect("save");
        let loaded = m.load().expect("load").expect("state present");
        assert_eq!(loaded.stage, RunStage::CredentialsSet);
        assert_eq!(loaded.agent, "gpt-dev");
        assert_eq!(loaded.workspace_id, "ws-xyz999");
        assert_eq!(loaded.image_sha256.as_deref(), Some("abc123def456"));
    }

    #[test]
    fn test_state_manager_clear_removes_existing_file() {
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        m.save(&make_state()).expect("save");
        m.clear().expect("clear");
        assert!(!dir.path().join("state.json").exists());
    }

    #[test]
    fn test_state_manager_clear_is_noop_when_no_file() {
        let dir = TempDir::new().expect("tempdir");
        let result = mgr(&dir).clear();
        assert!(result.is_ok(), "clear with no file must not error");
    }

    #[test]
    fn test_state_manager_advance_updates_stage_in_memory_and_on_disk() {
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        let mut state = make_state(); // stage = Provisioned
        m.advance(&mut state, RunStage::AgentReady)
            .expect("advance");
        assert_eq!(
            state.stage,
            RunStage::AgentReady,
            "in-memory stage must update"
        );
        let on_disk = m.load().expect("load").expect("state present");
        assert_eq!(
            on_disk.stage,
            RunStage::AgentReady,
            "disk stage must update"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_state_manager_save_sets_600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().expect("tempdir");
        let m = mgr(&dir);
        m.save(&make_state()).expect("save");
        let perms = std::fs::metadata(dir.path().join("state.json"))
            .expect("metadata")
            .permissions();
        assert_eq!(perms.mode() & 0o777, 0o600, "state file must be mode 600");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use chrono::Utc;
    use polis_common::types::{RunStage, RunState};
    use proptest::prelude::*;
    use tempfile::TempDir;

    fn arb_run_stage() -> impl Strategy<Value = RunStage> {
        prop_oneof![
            Just(RunStage::WorkspaceCreated),
            Just(RunStage::CredentialsSet),
            Just(RunStage::Provisioned),
            Just(RunStage::AgentReady),
        ]
    }

    fn arb_run_state() -> impl Strategy<Value = RunState> {
        (
            arb_run_stage(),
            "[a-z][a-z0-9-]{1,20}",
            "[a-z]{2}-[a-z0-9]{6}",
            proptest::option::of("[a-f0-9]{64}"),
        )
            .prop_map(|(stage, agent, workspace_id, image_sha256)| RunState {
                stage,
                agent,
                workspace_id,
                started_at: Utc::now(),
                image_sha256,
            })
    }

    proptest! {
        /// save then load is identity for all RunState fields
        #[test]
        fn prop_save_load_roundtrip(run_state in arb_run_state()) {
            let dir = TempDir::new().expect("tempdir");
            let m = StateManager::with_path(dir.path().join("state.json"));
            m.save(&run_state).expect("save");
            let loaded = m.load().expect("load").expect("state present");
            prop_assert_eq!(loaded.stage, run_state.stage);
            prop_assert_eq!(loaded.agent, run_state.agent);
            prop_assert_eq!(loaded.workspace_id, run_state.workspace_id);
            prop_assert_eq!(loaded.image_sha256, run_state.image_sha256);
        }

        /// advance always sets the stage to the requested value
        #[test]
        fn prop_advance_sets_requested_stage(
            initial in arb_run_state(),
            target in arb_run_stage(),
        ) {
            let dir = TempDir::new().expect("tempdir");
            let m = StateManager::with_path(dir.path().join("state.json"));
            let mut run_state = initial;
            m.advance(&mut run_state, target).expect("advance");
            prop_assert_eq!(run_state.stage, target);
            let on_disk = m.load().expect("load").expect("state present");
            prop_assert_eq!(on_disk.stage, target);
        }

        /// save is idempotent — overwriting with the same state yields the same result
        #[test]
        fn prop_save_is_idempotent(run_state in arb_run_state()) {
            let dir = TempDir::new().expect("tempdir");
            let m = StateManager::with_path(dir.path().join("state.json"));
            m.save(&run_state).expect("first save");
            m.save(&run_state).expect("second save");
            let loaded = m.load().expect("load").expect("state present");
            prop_assert_eq!(loaded.stage, run_state.stage);
            prop_assert_eq!(loaded.agent, run_state.agent);
        }

        /// load after clear always returns None
        #[test]
        fn prop_load_after_clear_returns_none(run_state in arb_run_state()) {
            let dir = TempDir::new().expect("tempdir");
            let m = StateManager::with_path(dir.path().join("state.json"));
            m.save(&run_state).expect("save");
            m.clear().expect("clear");
            let result = m.load().expect("load after clear must not error");
            prop_assert!(result.is_none());
        }

        /// advance preserves all fields except stage
        #[test]
        fn prop_advance_preserves_non_stage_fields(
            initial in arb_run_state(),
            target in arb_run_stage(),
        ) {
            let dir = TempDir::new().expect("tempdir");
            let m = StateManager::with_path(dir.path().join("state.json"));
            let mut run_state = initial.clone();
            m.advance(&mut run_state, target).expect("advance");
            prop_assert_eq!(&run_state.agent, &initial.agent);
            prop_assert_eq!(&run_state.workspace_id, &initial.workspace_id);
            prop_assert_eq!(&run_state.image_sha256, &initial.image_sha256);
        }
    }
}
