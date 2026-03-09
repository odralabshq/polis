//! Polis home directory resolution and path accessors.
//!
//! [`PolisDir`] resolves `~/.polis/` as the base directory for all
//! Polis-managed files and provides typed accessors for well-known
//! subdirectory paths.
//!
//! **Dependency rule:** This module depends only on `dirs`, `anyhow`,
//! and standard library types — no intra-infra imports.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Resolves the Polis home directory (`~/.polis/`) and provides
/// typed accessors for all well-known subdirectory paths.
pub struct PolisDir {
    root: PathBuf,
}

impl PolisDir {
    /// Production constructor — resolves via `dirs::home_dir()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        Ok(Self {
            root: home.join(".polis"),
        })
    }

    /// Test constructor — injects an explicit root path.
    #[must_use]
    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// The resolved root directory (e.g., `/home/user/.polis`).
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path to the YAML configuration file (`<root>/config.yaml`).
    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.yaml")
    }

    /// Path to the JSON state file (`<root>/state.json`).
    #[must_use]
    pub fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }

    /// Path to the SSH known-hosts file (`<root>/known_hosts`).
    #[must_use]
    pub fn known_hosts_path(&self) -> PathBuf {
        self.root.join("known_hosts")
    }

    /// Path to the ED25519 private key (`<root>/id_ed25519`).
    #[must_use]
    pub fn identity_key_path(&self) -> PathBuf {
        self.root.join("id_ed25519")
    }

    /// Path to the ED25519 public key (`<root>/id_ed25519.pub`).
    #[must_use]
    pub fn identity_pub_path(&self) -> PathBuf {
        self.root.join("id_ed25519.pub")
    }

    /// Platform-specific images directory.
    ///
    /// - Linux: `~/polis/images/` (legacy path — parent of root + `polis/images`)
    /// - Others: `~/.polis/images/`
    #[must_use]
    pub fn images_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.root
                .parent()
                .unwrap_or(&self.root)
                .join("polis")
                .join("images")
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.root.join("images")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_root_sets_root() {
        let dir = PolisDir::with_root(PathBuf::from("/tmp/test-polis"));
        assert_eq!(dir.root(), Path::new("/tmp/test-polis"));
    }

    #[test]
    fn config_path_is_under_root() {
        let dir = PolisDir::with_root(PathBuf::from("/tmp/test-polis"));
        assert_eq!(
            dir.config_path(),
            PathBuf::from("/tmp/test-polis/config.yaml")
        );
    }

    #[test]
    fn state_path_is_under_root() {
        let dir = PolisDir::with_root(PathBuf::from("/tmp/test-polis"));
        assert_eq!(
            dir.state_path(),
            PathBuf::from("/tmp/test-polis/state.json")
        );
    }

    #[test]
    fn known_hosts_path_is_under_root() {
        let dir = PolisDir::with_root(PathBuf::from("/tmp/test-polis"));
        assert_eq!(
            dir.known_hosts_path(),
            PathBuf::from("/tmp/test-polis/known_hosts")
        );
    }

    #[test]
    fn identity_key_path_is_under_root() {
        let dir = PolisDir::with_root(PathBuf::from("/tmp/test-polis"));
        assert_eq!(
            dir.identity_key_path(),
            PathBuf::from("/tmp/test-polis/id_ed25519")
        );
    }

    #[test]
    fn identity_pub_path_is_under_root() {
        let dir = PolisDir::with_root(PathBuf::from("/tmp/test-polis"));
        assert_eq!(
            dir.identity_pub_path(),
            PathBuf::from("/tmp/test-polis/id_ed25519.pub")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn images_dir_linux_uses_parent() {
        let dir = PolisDir::with_root(PathBuf::from("/home/user/.polis"));
        assert_eq!(dir.images_dir(), PathBuf::from("/home/user/polis/images"));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn images_dir_non_linux_is_under_root() {
        let dir = PolisDir::with_root(PathBuf::from("/home/user/.polis"));
        assert_eq!(dir.images_dir(), PathBuf::from("/home/user/.polis/images"));
    }

    #[test]
    fn new_resolves_home_dir() {
        // This test verifies that `new()` succeeds on systems with a home directory.
        // It may fail in unusual CI environments without a home dir set.
        if dirs::home_dir().is_some() {
            let polis = PolisDir::new().expect("should resolve home directory");
            assert!(polis.root().ends_with(".polis"));
        }
    }
}
