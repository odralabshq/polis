//! Filesystem infrastructure — implements `LocalArtifactWriter` and raw file ops.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::application::ports::LocalArtifactWriter;
use crate::domain::util::hex_encode;
use crate::infra::blocking::spawn_blocking_io;
use crate::infra::secure_fs::SecureFs;

/// Production filesystem implementation combining `LocalArtifactWriter`, `FileHasher`,
/// `LocalPaths`, and `LocalFs` in a single struct.
///
/// # Cohesion rationale
///
/// These four traits are unified in `OsFs` because the `App` trait requires
/// `type Fs: LocalFs + LocalPaths + FileHasher`, and call sites such as
/// `commands/delete.rs` pass the same `app.fs()` reference for both the
/// `local_fs: &F` (`LocalFs`) and `paths: &L` (`LocalPaths`) slots of
/// `CleanupContext`. Splitting into separate structs would force callers to
/// construct and manage two independent instances that share no state, adding
/// complexity without benefit. All four traits operate on the OS filesystem
/// with no internal state, so a single zero-sized unit struct is the natural
/// implementation.
pub struct OsFs;

impl LocalArtifactWriter for OsFs {
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    async fn write_agent_artifacts(
        &self,
        agent_name: &str,
        files: HashMap<String, String>,
    ) -> Result<PathBuf> {
        let dir = PathBuf::from("agents").join(agent_name).join(".generated");
        let dir_clone = dir.clone();
        spawn_blocking_io("write agent artifacts", move || {
            std::fs::create_dir_all(&dir_clone)
                .with_context(|| format!("creating artifact dir {}", dir_clone.display()))?;
            for (filename, content) in &files {
                let path = dir_clone.join(filename);
                std::fs::write(&path, content)
                    .with_context(|| format!("writing artifact {}", path.display()))?;
            }
            Ok::<PathBuf, anyhow::Error>(dir_clone)
        })
        .await?;
        Ok(dir)
    }
}

impl crate::application::ports::FileHasher for OsFs {
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn sha256_file(&self, path: &Path) -> Result<String> {
        sha256_file(path)
    }
}

impl crate::application::ports::LocalPaths for OsFs {
    fn images_dir(&self) -> PathBuf {
        images_dir().unwrap_or_else(|_| PathBuf::from("images"))
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn polis_dir(&self) -> Result<PathBuf> {
        crate::infra::polis_dir::PolisDir::new().map(|pd| pd.root().to_path_buf())
    }
}

impl crate::application::ports::LocalFs for OsFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("creating directory {}", path.display()))
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("removing directory {}", path.display()))
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn remove_file(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path).with_context(|| format!("removing file {}", path.display()))
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn write(&self, path: &Path, content: String) -> Result<()> {
        std::fs::write(path, content).with_context(|| format!("writing file {}", path.display()))
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn read_to_string(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).with_context(|| format!("reading file {}", path.display()))
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn set_permissions(&self, path: &Path, mode: u32) -> Result<()> {
        SecureFs::set_permissions(path, mode)
    }
}

/// Compute the SHA256 hex digest of a file.
///
/// Reads the file in 64 KB chunks to avoid loading large files into memory.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or read.
pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = file.read(&mut buf).context("reading file")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

/// Returns the image cache directory (legacy — used by `polis delete --all`).
///
/// Linux: `~/polis/images/`
/// Windows/macOS: `~/.polis/images/`
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub(crate) fn images_dir() -> Result<PathBuf> {
    let polis_dir = crate::infra::polis_dir::PolisDir::new()?;
    Ok(polis_dir.images_dir())
}
