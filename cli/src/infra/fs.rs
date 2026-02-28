//! Filesystem infrastructure — implements `LocalArtifactWriter` and raw file ops.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::application::ports::LocalArtifactWriter;
use crate::domain::workspace::hex_encode;

/// Writes agent artifact files to the local filesystem under `.generated/`.
/// Production filesystem implementation of `LocalArtifactWriter`.
#[allow(dead_code)] // Not yet wired from command handlers
pub struct LocalFs;

impl LocalArtifactWriter for LocalFs {
    async fn write_agent_artifacts(
        &self,
        agent_name: &str,
        files: HashMap<String, String>,
    ) -> Result<PathBuf> {
        let dir = PathBuf::from("agents").join(agent_name).join(".generated");
        let dir_clone = dir.clone();
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&dir_clone)
                .with_context(|| format!("creating artifact dir {}", dir_clone.display()))?;
            for (filename, content) in &files {
                let path = dir_clone.join(filename);
                std::fs::write(&path, content)
                    .with_context(|| format!("writing artifact {}", path.display()))?;
            }
            Ok::<PathBuf, anyhow::Error>(dir_clone)
        })
        .await
        .context("spawn_blocking for write_agent_artifacts")??;
        Ok(dir)
    }
}

impl crate::application::ports::FileHasher for LocalFs {
    fn sha256_file(&self, path: &Path) -> Result<String> {
        sha256_file(path)
    }
}

impl crate::application::ports::LocalPaths for LocalFs {
    fn images_dir(&self) -> PathBuf {
        images_dir().unwrap_or_else(|_| PathBuf::from("images"))
    }

    fn polis_dir(&self) -> Result<PathBuf> {
        dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))
            .map(|h| h.join(".polis"))
    }
}

impl crate::application::ports::LocalFs for LocalFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("creating directory {}", path.display()))
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("removing directory {}", path.display()))
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path).with_context(|| format!("removing file {}", path.display()))
    }

    fn write(&self, path: &Path, content: String) -> Result<()> {
        std::fs::write(path, content).with_context(|| format!("writing file {}", path.display()))
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).with_context(|| format!("reading file {}", path.display()))
    }

    fn set_permissions(&self, path: &Path, _mode: u32) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(_mode))
                .with_context(|| format!("setting permissions on {}", path.display()))?;
        }
        Ok(())
    }
}

/// Compute the SHA256 hex digest of a file.
///
/// Reads the file in 64 KB chunks to avoid loading large files into memory.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or read.
pub fn sha256_file(path: &Path) -> Result<String> {
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
pub fn images_dir() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    return Ok(dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join("polis")
        .join("images"));
    #[cfg(not(target_os = "linux"))]
    Ok(dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".polis")
        .join("images"))
}
