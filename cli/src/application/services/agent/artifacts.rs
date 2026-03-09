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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::application::vm::test_support::LocalFsStub;
    use std::path::PathBuf;

    fn minimal_manifest() -> polis_common::agent::AgentManifest {
        serde_yaml_ng::from_str(
            r#"
apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: test-agent
  displayName: "Test Agent"
  version: "1.0.0"
  description: "Test"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: "/usr/bin/node dist/index.js"
    workdir: /app
    user: polis
"#,
        )
        .unwrap()
    }

    #[test]
    fn write_artifacts_creates_all_four_files() {
        let fs = LocalFsStub::new(vec![]);
        let dir = PathBuf::from("/tmp/generated");
        let manifest = minimal_manifest();
        write_artifacts_to_dir(&fs, &dir, "test-agent", &manifest, String::new()).unwrap();
        let written = fs.written.lock().unwrap();
        assert!(written.contains_key(&dir.join("compose.agent.yaml")));
        assert!(written.contains_key(&dir.join("test-agent.service")));
        assert!(written.contains_key(&dir.join("test-agent.service.sha256")));
        assert!(written.contains_key(&dir.join("test-agent.env")));
    }

    #[test]
    fn write_artifacts_write_failure_propagates() {
        let mut fs = LocalFsStub::new(vec![]);
        fs.write_fails = true;
        let dir = PathBuf::from("/tmp/generated");
        let manifest = minimal_manifest();
        assert!(write_artifacts_to_dir(&fs, &dir, "test-agent", &manifest, String::new()).is_err());
    }
}
