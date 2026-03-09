//! Permission-aware file operations with atomic write support.
//!
//! [`SecureFs`] encapsulates the "create parent dir → write file → set
//! permissions" pattern that was previously duplicated across `ssh.rs`,
//! `config.rs`, `state.rs`, and `fs.rs` with four different implementations.
//!
//! **Dependency rule:** This module depends only on `anyhow` and standard
//! library types — no intra-infra imports outside the allowed set
//! (`polis_dir`, `blocking`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Permission-aware file operations with atomic write support.
///
/// Provides [`write_secure`](SecureFs::write_secure) for atomic writes
/// (temp file + rename), [`ensure_dir`](SecureFs::ensure_dir) for
/// directory creation with permissions, and
/// [`set_permissions`](SecureFs::set_permissions) for cross-platform
/// permission setting.
pub struct SecureFs;

impl Default for SecureFs {
    fn default() -> Self {
        Self::new()
    }
}

impl SecureFs {
    /// Production constructor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Test constructor — kept for API compatibility with test code.
    #[must_use]
    pub fn with_root(_root: PathBuf) -> Self {
        Self
    }

    /// Atomic write: write to temp file, set permissions, rename to target.
    ///
    /// Follows the pattern established in `StateManager::save_sync()`.
    /// If the rename fails the temp file may remain on disk — the next
    /// write attempt will overwrite it.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent directory cannot be created, the temp
    /// file cannot be written, permissions cannot be set, or the rename fails.
    pub fn write_secure(&self, path: &Path, content: &[u8], mode: u32) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        let temp_path = path.with_extension("tmp");
        std::fs::write(&temp_path, content)
            .with_context(|| format!("writing temp file {}", temp_path.display()))?;
        Self::set_permissions_inner(&temp_path, mode)?;
        std::fs::rename(&temp_path, path)
            .with_context(|| format!("finalizing file {}", path.display()))?;
        Ok(())
    }

    /// Create directory with specified permissions.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or permissions
    /// cannot be set.
    pub fn ensure_dir(&self, path: &Path, mode: u32) -> Result<()> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("creating directory {}", path.display()))?;
        Self::set_permissions_inner(path, mode)?;
        Ok(())
    }

    /// Cross-platform permission setting.
    ///
    /// On Unix, applies `from_mode(mode)`. On non-Unix platforms this is
    /// a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if the permissions cannot be set.
    pub fn set_permissions(path: &Path, mode: u32) -> Result<()> {
        Self::set_permissions_inner(path, mode)
    }

    fn set_permissions_inner(path: &Path, mode: u32) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                .with_context(|| format!("setting permissions on {}", path.display()))?;
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_secure_creates_file_with_content() {
        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let file = tmp.path().join("test.txt");
        let content = b"hello world";

        fs.write_secure(&file, content, 0o600).expect("write_secure");

        assert_eq!(std::fs::read(&file).expect("read file"), content);
    }

    #[test]
    fn write_secure_leaves_no_tmp_file() {
        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let file = tmp.path().join("test.txt");

        fs.write_secure(&file, b"data", 0o600).expect("write_secure");

        let tmp_file = file.with_extension("tmp");
        assert!(!tmp_file.exists(), "temp file should be removed after rename");
    }

    #[test]
    fn write_secure_creates_parent_directories() {
        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let file = tmp.path().join("a").join("b").join("c.txt");

        fs.write_secure(&file, b"nested", 0o600).expect("write_secure");

        assert_eq!(std::fs::read(&file).expect("read file"), b"nested");
    }

    #[test]
    fn ensure_dir_creates_directory() {
        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let dir = tmp.path().join("new_dir");

        fs.ensure_dir(&dir, 0o700).expect("ensure_dir");

        assert!(dir.is_dir());
    }

    #[test]
    fn ensure_dir_creates_nested_directories() {
        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let dir = tmp.path().join("a").join("b").join("c");

        fs.ensure_dir(&dir, 0o700).expect("ensure_dir");

        assert!(dir.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn set_permissions_applies_mode_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().expect("create tempdir");
        let file = tmp.path().join("perms.txt");
        std::fs::write(&file, b"test").expect("write file");

        SecureFs::set_permissions(&file, 0o600).expect("set_permissions");

        let perms = std::fs::metadata(&file).expect("metadata").permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[cfg(not(unix))]
    #[test]
    fn set_permissions_is_noop_on_non_unix() {
        let tmp = TempDir::new().expect("create tempdir");
        let file = tmp.path().join("perms.txt");
        std::fs::write(&file, b"test").expect("write file");

        // Should succeed without error on non-Unix platforms.
        SecureFs::set_permissions(&file, 0o600).expect("set_permissions");
    }

    #[cfg(unix)]
    #[test]
    fn write_secure_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let file = tmp.path().join("secure.txt");

        fs.write_secure(&file, b"secret", 0o600).expect("write_secure");

        let perms = std::fs::metadata(&file).expect("metadata").permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let dir = tmp.path().join("secure_dir");

        fs.ensure_dir(&dir, 0o700).expect("ensure_dir");

        let perms = std::fs::metadata(&dir).expect("metadata").permissions();
        assert_eq!(perms.mode() & 0o777, 0o700);
    }

    #[test]
    fn new_creates_usable_instance() {
        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let file = tmp.path().join("test.txt");
        fs.write_secure(&file, b"ok", 0o600).expect("write_secure");
        assert!(file.exists());
    }

    #[test]
    fn with_root_creates_usable_instance() {
        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::with_root(tmp.path().to_path_buf());
        let file = tmp.path().join("test.txt");
        fs.write_secure(&file, b"ok", 0o600).expect("write_secure");
        assert!(file.exists());
    }

    #[test]
    fn write_secure_empty_content() {
        let tmp = TempDir::new().expect("create tempdir");
        let fs = SecureFs::new();
        let file = tmp.path().join("empty.txt");

        fs.write_secure(&file, b"", 0o600).expect("write_secure");

        assert_eq!(std::fs::read(&file).expect("read file"), b"");
    }
}
