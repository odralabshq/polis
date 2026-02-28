//! Filesystem infrastructure — implements `LocalArtifactWriter` and raw file ops.

#![allow(dead_code)] // Refactor in progress — defined ahead of callers

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::application::ports::LocalArtifactWriter;

/// Writes agent artifact files to the local filesystem under `.generated/`.
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

/// Compute the SHA256 hex digest of a file.
///
/// Delegates to [`crate::workspace::image::sha256_file`].
///
/// # Errors
///
/// Returns an error if the file cannot be opened or read.
pub fn sha256_file(path: &std::path::Path) -> Result<String> {
    crate::workspace::image::sha256_file(path)
}
