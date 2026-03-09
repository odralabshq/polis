//! SSH known hosts management — host key pinning file management.
//!
//! Provides [`KnownHostsManager`] for managing `~/.polis/known_hosts` and
//! the [`KnownHostsOps`] trait for dependency injection into [`super::SshConfigManager`].

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::infra::secure_fs::SecureFs;

/// Abstracts known hosts operations for dependency injection.
///
/// Enables [`super::SshConfigManager`] to accept a mock or stub in tests
/// instead of a real [`KnownHostsManager`].
pub trait KnownHostsOps: Send + Sync {
    /// Writes `host_key_line` to the known hosts file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written or permissions cannot be set.
    fn update(&self, host_key_line: &str) -> Result<()>;
    /// Removes the known hosts file if it exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be removed.
    fn remove(&self) -> Result<()>;
}

/// Manages `~/.polis/known_hosts` for SSH host key pinning.
pub struct KnownHostsManager {
    path: PathBuf,
}

impl KnownHostsManager {
    /// Creates a manager pointing at `~/.polis/known_hosts`.
    /// # Errors
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let polis_dir = crate::infra::polis_dir::PolisDir::new()?;
        Ok(Self::with_path(polis_dir.known_hosts_path()))
    }

    /// Creates a manager pointing at an arbitrary path (for testing).
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Writes `host_key_line` to the `known_hosts` file, creating parent dirs as needed.
    /// Sets file permissions to 600 and parent directory to 700 on Unix.
    /// # Errors
    /// Returns an error if the file cannot be written or permissions cannot be set.
    pub fn update(&self, host_key_line: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
            SecureFs::set_permissions(parent, 0o700)?;
        }
        std::fs::write(&self.path, host_key_line)
            .with_context(|| format!("write {}", self.path.display()))?;
        SecureFs::set_permissions(&self.path, 0o600)?;
        Ok(())
    }

    /// Removes the `known_hosts` file if it exists.
    /// # Errors
    /// Returns an error if the file exists but cannot be removed.
    pub fn remove(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .with_context(|| format!("remove {}", self.path.display()))?;
        }
        Ok(())
    }
}

impl KnownHostsOps for KnownHostsManager {
    fn update(&self, host_key_line: &str) -> Result<()> {
        self.update(host_key_line)
    }

    fn remove(&self) -> Result<()> {
        self.remove()
    }
}

#[cfg(test)]
mod tests {
    // ⚠️  Testability requirement: `KnownHostsManager` must expose a
    // `with_path(path: PathBuf) -> Self` constructor so tests can inject a
    // temp directory instead of relying on `$HOME`.  The production `new()`
    // delegates to `with_path(home.join(".polis").join("known_hosts"))`.
    //
    // ⚠️  Testability requirement: extract a `pub fn validate_host_key(key: &str)
    // -> Result<()>` from the inline check in `extract_from_multipass` /
    // `extract_from_docker` so the validation logic can be unit-tested directly.
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn manager_in(dir: &tempfile::TempDir) -> KnownHostsManager {
        KnownHostsManager::with_path(dir.path().join("known_hosts"))
    }

    const VALID_KEY: &str = "workspace ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKeyMaterialHere";

    // -----------------------------------------------------------------------
    // KnownHostsManager::update
    // -----------------------------------------------------------------------

    #[test]
    fn test_known_hosts_manager_update_creates_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("known_hosts");
        assert!(!path.exists());
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        assert!(path.exists());
    }

    #[test]
    fn test_known_hosts_manager_update_creates_parent_directory() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let nested = dir.path().join("a").join("b");
        let mgr = KnownHostsManager::with_path(nested.join("known_hosts"));
        mgr.update(VALID_KEY)
            .expect("update should create parent dirs");
        assert!(nested.exists());
    }

    #[test]
    fn test_known_hosts_manager_update_overwrites_existing_content() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.update("workspace ssh-ed25519 OldKey")
            .expect("first update");
        mgr.update(VALID_KEY).expect("second update");
        let content =
            std::fs::read_to_string(dir.path().join("known_hosts")).expect("file should exist");
        assert_eq!(content, VALID_KEY);
    }

    #[cfg(unix)]
    #[test]
    fn test_known_hosts_manager_update_sets_file_permissions_600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        let mode = std::fs::metadata(dir.path().join("known_hosts"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "file must be 600");
    }

    #[cfg(unix)]
    #[test]
    fn test_known_hosts_manager_update_sets_parent_dir_permissions_700() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().expect("tempdir");
        let parent = dir.path().join("polis_dir");
        let mgr = KnownHostsManager::with_path(parent.join("known_hosts"));
        mgr.update(VALID_KEY).expect("update should succeed");
        let mode = std::fs::metadata(&parent)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "directory must be 700");
    }

    // -----------------------------------------------------------------------
    // KnownHostsManager::remove
    // -----------------------------------------------------------------------

    #[test]
    fn test_known_hosts_manager_remove_deletes_existing_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("known_hosts");
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        mgr.remove().expect("remove should succeed");
        assert!(!path.exists());
    }

    #[test]
    fn test_known_hosts_manager_remove_is_idempotent_when_file_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        // File never created — remove must not error.
        let result = mgr.remove();
        assert!(result.is_ok());
    }
}
