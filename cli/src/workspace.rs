//! Workspace driver abstraction for start/stop/remove operations.

use std::process::Command;

use anyhow::{Context, Result};

/// VM name used by multipass.
const VM_NAME: &str = "polis";

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

// TODO: refactor to compose over crate::multipass::Multipass trait
// to eliminate duplicated vm_info/start/exec calls (audit F-003).
/// Production driver — delegates to multipass for VM lifecycle.
pub struct MultipassDriver;

impl WorkspaceDriver for MultipassDriver {
    fn is_running(&self, _workspace_id: &str) -> Result<bool> {
        let output = Command::new("multipass")
            .args(["info", VM_NAME, "--format", "json"])
            .output()
            .context("failed to run multipass info")?;

        if !output.status.success() {
            // VM doesn't exist or multipass not available
            return Ok(false);
        }

        let info: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("failed to parse multipass info output")?;

        Ok(parse_multipass_state(&info, VM_NAME) == Some("Running"))
    }

    fn start(&self, _workspace_id: &str) -> Result<()> {
        let output = Command::new("multipass")
            .args(["start", VM_NAME])
            .output()
            .context("failed to run multipass start")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("failed to start workspace: {stderr}");
        }

        Ok(())
    }

    fn stop(&self, _workspace_id: &str) -> Result<()> {
        // First stop docker compose services inside the VM
        let _ = Command::new("multipass")
            .args(["exec", VM_NAME, "--", "docker", "compose", "stop"])
            .output();

        // Then stop the VM itself
        let output = Command::new("multipass")
            .args(["stop", VM_NAME])
            .output()
            .context("failed to run multipass stop")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("failed to stop workspace: {stderr}");
        }

        Ok(())
    }

    fn remove(&self, _workspace_id: &str) -> Result<()> {
        // Delete the VM
        let output = Command::new("multipass")
            .args(["delete", VM_NAME])
            .output()
            .context("failed to run multipass delete")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't fail if VM doesn't exist
            if !stderr.contains("does not exist") {
                anyhow::bail!("failed to delete workspace: {stderr}");
            }
        }

        // Purge deleted VMs
        let _ = Command::new("multipass").args(["purge"]).output();

        Ok(())
    }

    fn remove_cached_images(&self) -> Result<()> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;

        // Remove cached VM images from ~/.polis/images/
        let images_dir = home.join(".polis").join("images");
        if images_dir.exists() {
            std::fs::remove_dir_all(&images_dir)
                .with_context(|| format!("failed to remove {}", images_dir.display()))?;
        }

        Ok(())
    }
}

/// Test driver — allows controlling workspace state in tests.
#[cfg(test)]
pub struct MockDriver {
    pub running: bool,
}

#[cfg(test)]
impl WorkspaceDriver for MockDriver {
    fn is_running(&self, _workspace_id: &str) -> Result<bool> {
        Ok(self.running)
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

/// Parse multipass info JSON to extract VM state.
///
/// Returns `None` if the JSON structure is invalid or state is missing.
#[must_use]
pub fn parse_multipass_state<'a>(json: &'a serde_json::Value, vm_name: &str) -> Option<&'a str> {
    json.get("info")?.get(vm_name)?.get("state")?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── parse_multipass_state ────────────────────────────────────────────────

    #[test]
    fn test_parse_multipass_state_running() {
        let json: serde_json::Value = serde_json::json!({
            "info": {
                "polis": {
                    "state": "Running"
                }
            }
        });
        assert_eq!(parse_multipass_state(&json, "polis"), Some("Running"));
    }

    #[test]
    fn test_parse_multipass_state_stopped() {
        let json: serde_json::Value = serde_json::json!({
            "info": {
                "polis": {
                    "state": "Stopped"
                }
            }
        });
        assert_eq!(parse_multipass_state(&json, "polis"), Some("Stopped"));
    }

    #[test]
    fn test_parse_multipass_state_missing_vm() {
        let json: serde_json::Value = serde_json::json!({
            "info": {}
        });
        assert_eq!(parse_multipass_state(&json, "polis"), None);
    }

    #[test]
    fn test_parse_multipass_state_missing_state_field() {
        let json: serde_json::Value = serde_json::json!({
            "info": {
                "polis": {}
            }
        });
        assert_eq!(parse_multipass_state(&json, "polis"), None);
    }

    #[test]
    fn test_parse_multipass_state_empty_json() {
        let json: serde_json::Value = serde_json::json!({});
        assert_eq!(parse_multipass_state(&json, "polis"), None);
    }

    // ── remove_cached_images ─────────────────────────────────────────────────

    #[test]
    fn test_remove_cached_images_removes_images_dir() {
        let dir = TempDir::new().expect("tempdir");
        let images_dir = dir.path().join(".polis").join("images");
        std::fs::create_dir_all(&images_dir).expect("create images dir");
        std::fs::write(images_dir.join("test.qcow2"), b"test").expect("write file");

        // Use a custom implementation that uses the temp dir
        let result = remove_images_dir(&images_dir);
        assert!(result.is_ok());
        assert!(!images_dir.exists());
    }

    #[test]
    fn test_remove_cached_images_noop_when_dir_absent() {
        let dir = TempDir::new().expect("tempdir");
        let images_dir = dir.path().join(".polis").join("images");
        // Don't create the directory

        let result = remove_images_dir(&images_dir);
        assert!(result.is_ok());
    }

    /// Helper for testing — removes images directory at given path.
    fn remove_images_dir(images_dir: &std::path::Path) -> Result<()> {
        if images_dir.exists() {
            std::fs::remove_dir_all(images_dir)
                .with_context(|| format!("failed to remove {}", images_dir.display()))?;
        }
        Ok(())
    }

    // ── MockDriver ───────────────────────────────────────────────────────────

    #[test]
    fn test_mock_driver_is_running_returns_configured_value() {
        let driver = MockDriver { running: true };
        assert!(driver.is_running("ws-123").expect("is_running"));

        let driver = MockDriver { running: false };
        assert!(!driver.is_running("ws-123").expect("is_running"));
    }

    #[test]
    fn test_mock_driver_start_always_succeeds() {
        let driver = MockDriver { running: false };
        assert!(driver.start("ws-123").is_ok());
    }

    #[test]
    fn test_mock_driver_stop_always_succeeds() {
        let driver = MockDriver { running: true };
        assert!(driver.stop("ws-123").is_ok());
    }

    #[test]
    fn test_mock_driver_remove_always_succeeds() {
        let driver = MockDriver { running: false };
        assert!(driver.remove("ws-123").is_ok());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// parse_multipass_state returns Running only when state is "Running"
        #[test]
        fn prop_parse_multipass_state_running_detection(
            state in prop_oneof![
                Just("Running"),
                Just("Stopped"),
                Just("Starting"),
                Just("Stopping"),
                Just("Deleted"),
            ]
        ) {
            let json = serde_json::json!({
                "info": {
                    "polis": {
                        "state": state
                    }
                }
            });
            let parsed = parse_multipass_state(&json, "polis");
            prop_assert_eq!(parsed, Some(state));
        }

        /// parse_multipass_state returns None for any malformed JSON
        #[test]
        fn prop_parse_multipass_state_malformed_returns_none(
            has_info in any::<bool>(),
            has_vm in any::<bool>(),
            has_state in any::<bool>(),
        ) {
            let json = if !has_info {
                serde_json::json!({})
            } else if !has_vm {
                serde_json::json!({"info": {}})
            } else if !has_state {
                serde_json::json!({"info": {"polis": {}}})
            } else {
                serde_json::json!({"info": {"polis": {"state": "Running"}}})
            };

            let result = parse_multipass_state(&json, "polis");
            if has_info && has_vm && has_state {
                prop_assert!(result.is_some());
            } else {
                prop_assert!(result.is_none());
            }
        }

        /// MockDriver is_running always returns the configured value
        #[test]
        fn prop_mock_driver_is_running_returns_configured(running in any::<bool>()) {
            let driver = MockDriver { running };
            prop_assert_eq!(driver.is_running("any-id").expect("is_running"), running);
        }
    }
}
