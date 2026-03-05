//! Agent artifacts service — shared artifact writing utilities.
//!
//! This module provides the `write_artifacts_to_dir` function used by both
//! the install service (local install) and the activate service (VM-based setup).
//!
//! Extracting this into a dedicated module breaks the horizontal coupling where
//! `agent_activate.rs` imported from `agent_crud.rs`.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::{Context, Result};
use std::path::Path;

use crate::application::ports::LocalFs;
use polis_common::agent::AgentManifest;

/// Write generated agent artifacts to `<generated_dir>/`.
///
/// Creates the following files:
/// - `compose.agent.yaml` — Docker Compose overlay for the agent
/// - `<name>.service` — systemd unit file
/// - `<name>.service.sha256` — hash of the systemd unit for change detection
/// - `<name>.env` — filtered environment variables
///
/// Shared by `generate_and_write_artifacts` (local install) and
/// `setup_agent` (VM-based update/start).
///
/// # Arguments
///
/// * `local_fs` - Filesystem abstraction for writing files
/// * `generated_dir` - Directory to write artifacts to
/// * `name` - Agent name (used for file naming)
/// * `manifest` - Agent manifest containing configuration
/// * `env_content` - Filtered environment variable content
///
/// # Errors
///
/// Returns an error if directory creation or any file write fails.
///
/// # Requirements
///
/// - 3.5: Shared artifact writing extracted into dedicated module
pub(crate) fn write_artifacts_to_dir(
    local_fs: &impl LocalFs,
    generated_dir: &Path,
    name: &str,
    manifest: &AgentManifest,
    env_content: String,
) -> Result<()> {
    use crate::domain::agent::artifacts;

    local_fs
        .create_dir_all(generated_dir)
        .with_context(|| format!("creating {}", generated_dir.display()))?;

    let compose = artifacts::compose_overlay(manifest);
    local_fs
        .write(&generated_dir.join("compose.agent.yaml"), compose)
        .context("writing compose.agent.yaml")?;

    let unit = artifacts::systemd_unit(manifest);
    let hash = artifacts::service_hash(&unit);
    local_fs
        .write(&generated_dir.join(format!("{name}.service")), unit)
        .context("writing .service file")?;
    local_fs
        .write(&generated_dir.join(format!("{name}.service.sha256")), hash)
        .context("writing .service.sha256 file")?;

    local_fs
        .write(&generated_dir.join(format!("{name}.env")), env_content)
        .context("writing .env file")?;

    Ok(())
}
