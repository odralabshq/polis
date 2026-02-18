//! SSH utilities — host key pinning (`KnownHostsManager`).

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Validates that `key` is an ed25519 public key with non-empty key material.
///
/// Accepts the raw public key format: `ssh-ed25519 <base64-material>`.
///
/// # Errors
///
/// Returns an error if the key does not start with `ssh-ed25519 ` or has no
/// key material after the prefix.
pub fn validate_host_key(key: &str) -> Result<()> {
    let material = key
        .strip_prefix("ssh-ed25519 ")
        .ok_or_else(|| anyhow::anyhow!("host key must be an ed25519 key (got: {key:?})"))?;
    anyhow::ensure!(!material.trim().is_empty(), "host key has no key material");
    Ok(())
}

/// Manages `~/.polis/known_hosts` for SSH host key pinning.
pub struct KnownHostsManager {
    path: PathBuf,
}

impl KnownHostsManager {
    /// Creates a manager pointing at `~/.polis/known_hosts`.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        Ok(Self::with_path(home.join(".polis").join("known_hosts")))
    }

    /// Creates a manager pointing at an arbitrary path (for testing).
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Writes `host_key_line` to the `known_hosts` file, creating parent dirs as needed.
    ///
    /// Sets file permissions to 600 and parent directory to 700 on Unix.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written or permissions cannot be set.
    pub fn update(&self, host_key_line: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
            set_permissions(parent, 0o700)?;
        }
        std::fs::write(&self.path, host_key_line)
            .with_context(|| format!("write {}", self.path.display()))?;
        set_permissions(&self.path, 0o600)?;
        Ok(())
    }

    /// Returns `true` if the `known_hosts` file exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Removes the `known_hosts` file if it exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be removed.
    pub fn remove(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .with_context(|| format!("remove {}", self.path.display()))?;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn set_permissions(path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_permissions(_path: &std::path::Path, _mode: u32) -> Result<()> {
    Ok(())
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

    const VALID_KEY: &str =
        "workspace ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKeyMaterialHere";

    // -----------------------------------------------------------------------
    // KnownHostsManager::exists
    // -----------------------------------------------------------------------

    #[test]
    fn test_known_hosts_manager_exists_returns_false_when_file_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        assert!(!mgr.exists());
    }

    #[test]
    fn test_known_hosts_manager_exists_returns_true_after_update() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        assert!(mgr.exists());
    }

    // -----------------------------------------------------------------------
    // KnownHostsManager::update
    // -----------------------------------------------------------------------

    #[test]
    fn test_known_hosts_manager_update_writes_content_to_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        let content = std::fs::read_to_string(dir.path().join("known_hosts"))
            .expect("file should exist");
        assert_eq!(content, VALID_KEY);
    }

    #[test]
    fn test_known_hosts_manager_update_creates_parent_directory() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let nested = dir.path().join("a").join("b");
        let mgr = KnownHostsManager::with_path(nested.join("known_hosts"));
        mgr.update(VALID_KEY).expect("update should create parent dirs");
        assert!(nested.exists());
    }

    #[test]
    fn test_known_hosts_manager_update_overwrites_existing_content() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        mgr.update("workspace ssh-ed25519 OldKey").expect("first update");
        mgr.update(VALID_KEY).expect("second update");
        let content = std::fs::read_to_string(dir.path().join("known_hosts"))
            .expect("file should exist");
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
        let mgr = manager_in(&dir);
        mgr.update(VALID_KEY).expect("update should succeed");
        mgr.remove().expect("remove should succeed");
        assert!(!mgr.exists());
    }

    #[test]
    fn test_known_hosts_manager_remove_is_idempotent_when_file_absent() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let mgr = manager_in(&dir);
        // File never created — remove must not error.
        let result = mgr.remove();
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // validate_host_key
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_host_key_accepts_valid_ed25519_key() {
        let result = validate_host_key("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_host_key_rejects_rsa_key() {
        let result = validate_host_key("ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC...");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_host_key_rejects_ecdsa_key() {
        let result = validate_host_key("ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY...");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_host_key_rejects_empty_string() {
        let result = validate_host_key("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_host_key_rejects_key_with_only_prefix() {
        // "ssh-ed25519 " with no key material after the space
        let result = validate_host_key("ssh-ed25519 ");
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod proptests {
    use super::{validate_host_key, KnownHostsManager};
    use proptest::prelude::*;

    proptest! {
        /// Any "ssh-ed25519 <non-whitespace-material>" is accepted.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_validate_host_key_accepts_ed25519_with_material(
            material in "[A-Za-z0-9+/]{10,100}"
        ) {
            let key = format!("ssh-ed25519 {material}");
            prop_assert!(validate_host_key(&key).is_ok());
        }

        /// Any non-ed25519 key type prefix is rejected.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_validate_host_key_rejects_non_ed25519_prefix(
            prefix in "(ssh-rsa|ecdsa-sha2-nistp256|sk-ssh-ed25519|ssh-dss)",
            material in "[A-Za-z0-9+/]{10,100}",
        ) {
            let key = format!("{prefix} {material}");
            prop_assert!(validate_host_key(&key).is_err());
        }

        /// update then read always returns the exact content written.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_update_content_roundtrip(content in "[a-zA-Z0-9 ]{1,200}") {
            let dir = tempfile::TempDir::new().expect("tempdir");
            let mgr = KnownHostsManager::with_path(dir.path().join("known_hosts"));
            mgr.update(&content).expect("update");
            let read = std::fs::read_to_string(dir.path().join("known_hosts")).expect("read");
            prop_assert_eq!(read, content);
        }

        /// update always makes exists() return true.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_update_makes_exists_true(content in "[a-zA-Z0-9 ]{1,200}") {
            let dir = tempfile::TempDir::new().expect("tempdir");
            let mgr = KnownHostsManager::with_path(dir.path().join("known_hosts"));
            mgr.update(&content).expect("update");
            prop_assert!(mgr.exists());
        }

        /// remove after update always makes exists() return false.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_remove_after_update_makes_exists_false(content in "[a-zA-Z0-9 ]{1,200}") {
            let dir = tempfile::TempDir::new().expect("tempdir");
            let mgr = KnownHostsManager::with_path(dir.path().join("known_hosts"));
            mgr.update(&content).expect("update");
            mgr.remove().expect("remove");
            prop_assert!(!mgr.exists());
        }

        /// Second update always overwrites the first — last write wins.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_update_last_write_wins(
            first in "[a-zA-Z0-9]{1,100}",
            second in "[a-zA-Z0-9]{1,100}",
        ) {
            let dir = tempfile::TempDir::new().expect("tempdir");
            let mgr = KnownHostsManager::with_path(dir.path().join("known_hosts"));
            mgr.update(&first).expect("first update");
            mgr.update(&second).expect("second update");
            let read = std::fs::read_to_string(dir.path().join("known_hosts")).expect("read");
            prop_assert_eq!(read, second);
        }

        /// remove is always Ok whether or not the file exists.
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_remove_is_always_ok(content in proptest::option::of("[a-zA-Z0-9 ]{1,100}")) {
            let dir = tempfile::TempDir::new().expect("tempdir");
            let mgr = KnownHostsManager::with_path(dir.path().join("known_hosts"));
            if let Some(c) = &content {
                mgr.update(c).expect("update");
            }
            prop_assert!(mgr.remove().is_ok());
        }
    }
}
