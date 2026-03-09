//! SSH identity key generation — ED25519 keypair provisioning.
//!
//! Provides the [`IdentityKeyProvider`] trait for dependency injection and
//! [`OsIdentityKeyProvider`] as the production implementation that shells
//! out to `ssh-keygen`.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::infra::polis_dir::PolisDir;
use crate::infra::secure_fs::SecureFs;

/// Trait for identity key provisioning — enables test injection.
pub trait IdentityKeyProvider: Send + Sync {
    /// Ensure an ED25519 keypair exists, returning the public key.
    ///
    /// # Errors
    ///
    /// Returns an error if key generation fails or the public key cannot be read.
    fn ensure_identity_key(&self) -> Result<String>;
}

/// Production implementation that shells out to `ssh-keygen`.
pub struct OsIdentityKeyProvider {
    key_path: PathBuf,
    pub_path: PathBuf,
}

impl OsIdentityKeyProvider {
    /// Production constructor — derives paths from a [`PolisDir`].
    #[must_use]
    pub fn new(polis_dir: &PolisDir) -> Self {
        Self {
            key_path: polis_dir.identity_key_path(),
            pub_path: polis_dir.identity_pub_path(),
        }
    }

    /// Test constructor — injects explicit key and public-key paths.
    #[must_use]
    pub fn with_paths(key_path: PathBuf, pub_path: PathBuf) -> Self {
        Self { key_path, pub_path }
    }
}

impl IdentityKeyProvider for OsIdentityKeyProvider {
    fn ensure_identity_key(&self) -> Result<String> {
        if !self.key_path.exists() {
            if let Some(parent) = self.key_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create dir {}", parent.display()))?;
                SecureFs::set_permissions(parent, 0o700)?;
            }
            let status = std::process::Command::new("ssh-keygen")
                .args([
                    "-t",
                    "ed25519",
                    "-N",
                    "", // no passphrase
                    "-f",
                    self.key_path
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("non-UTF8 path"))?,
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .context("ssh-keygen not found")?;
            anyhow::ensure!(status.success(), "ssh-keygen failed");
            SecureFs::set_permissions(&self.key_path, 0o600)?;
        }

        let pubkey = std::fs::read_to_string(&self.pub_path)
            .with_context(|| format!("read {}", self.pub_path.display()))?;
        Ok(pubkey.trim().to_string())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_paths_stores_key_path() {
        let provider = OsIdentityKeyProvider::with_paths(
            PathBuf::from("/tmp/test-key"),
            PathBuf::from("/tmp/test-key.pub"),
        );
        assert_eq!(provider.key_path, PathBuf::from("/tmp/test-key"));
    }

    #[test]
    fn with_paths_stores_pub_path() {
        let provider = OsIdentityKeyProvider::with_paths(
            PathBuf::from("/tmp/test-key"),
            PathBuf::from("/tmp/test-key.pub"),
        );
        assert_eq!(provider.pub_path, PathBuf::from("/tmp/test-key.pub"));
    }

    #[test]
    fn new_derives_paths_from_polis_dir() {
        let polis_dir = PolisDir::with_root(PathBuf::from("/home/user/.polis"));
        let provider = OsIdentityKeyProvider::new(&polis_dir);
        assert_eq!(
            provider.key_path,
            PathBuf::from("/home/user/.polis/id_ed25519")
        );
        assert_eq!(
            provider.pub_path,
            PathBuf::from("/home/user/.polis/id_ed25519.pub")
        );
    }

    #[test]
    fn ensure_identity_key_reads_existing_pubkey() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let key_path = dir.path().join("id_ed25519");
        let pub_path = dir.path().join("id_ed25519.pub");

        // Simulate an existing keypair by writing dummy files.
        std::fs::write(&key_path, "dummy-private-key").expect("write key");
        std::fs::write(&pub_path, "ssh-ed25519 AAAA test@host\n").expect("write pub");

        let provider = OsIdentityKeyProvider::with_paths(key_path, pub_path);
        let result = provider.ensure_identity_key().expect("should succeed");
        assert_eq!(result, "ssh-ed25519 AAAA test@host");
    }

    #[test]
    fn ensure_identity_key_trims_trailing_whitespace() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let key_path = dir.path().join("id_ed25519");
        let pub_path = dir.path().join("id_ed25519.pub");

        std::fs::write(&key_path, "dummy-private-key").expect("write key");
        std::fs::write(&pub_path, "  ssh-ed25519 AAAA test@host  \n").expect("write pub");

        let provider = OsIdentityKeyProvider::with_paths(key_path, pub_path);
        let result = provider.ensure_identity_key().expect("should succeed");
        assert_eq!(result, "ssh-ed25519 AAAA test@host");
    }
}
