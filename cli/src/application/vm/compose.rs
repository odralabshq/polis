//! Shared compose helpers used by `workspace_start` and `agent_activate`.
//!
//! These functions manage the active overlay symlink and the ready marker
//! that gates `polis.service` auto-start. They are pure I/O wrappers with
//! no domain logic.

use anyhow::{Context, Result};

use crate::application::ports::ShellExecutor;
use crate::domain::workspace::{ACTIVE_OVERLAY_PATH, READY_MARKER_PATH};

/// Set or remove the active compose overlay symlink.
///
/// # Errors
///
/// Returns an error if the symlink operation fails inside the VM.
pub async fn set_active_overlay(
    provisioner: &impl ShellExecutor,
    overlay_path: Option<&str>,
) -> Result<()> {
    match overlay_path {
        Some(path) => {
            provisioner
                .exec(&["ln", "-sf", path, ACTIVE_OVERLAY_PATH])
                .await
                .context("creating overlay symlink")?;
        }
        None => {
            provisioner
                .exec(&["rm", "-f", ACTIVE_OVERLAY_PATH])
                .await
                .context("removing overlay symlink")?;
        }
    }
    Ok(())
}

/// Set or clear the ready marker that gates `polis.service` auto-start.
///
/// # Errors
///
/// Returns an error if the marker file operation fails inside the VM.
pub async fn set_ready_marker(provisioner: &impl ShellExecutor, enabled: bool) -> Result<()> {
    if enabled {
        provisioner
            .exec(&["touch", READY_MARKER_PATH])
            .await
            .context("creating ready marker")?;
    } else {
        provisioner
            .exec(&["rm", "-f", READY_MARKER_PATH])
            .await
            .context("removing ready marker")?;
    }
    Ok(())
}
