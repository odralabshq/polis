//! Agent service modules — single-responsibility use-case orchestration.
//!
//! Each submodule implements a single agent use case:
//! - `list`: List installed agents
//! - `install`: Install an agent from a local path
//! - `remove`: Remove an installed agent
//! - `artifacts`: Shared artifact writing utilities
//! - `activate`: Activate an agent on the running workspace
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use anyhow::{Result, bail};

use crate::application::ports::InstanceInspector;
use crate::application::services::vm::lifecycle::{self as vm, VmState};
use crate::domain::error::WorkspaceError;

// ── Submodules ────────────────────────────────────────────────────────────────

pub mod activate;
pub mod artifacts;
pub mod install;
pub mod list;
pub mod remove;

// ── Re-exports ────────────────────────────────────────────────────────────────

pub use activate::{
    ActivateOutcome, AgentActivateOptions, AgentOutcome, AgentSwapOptions, activate_agent,
    swap_agent,
};
pub use install::install_agent;
pub use list::list_agents;
pub use remove::remove_agent;

// ── Shared Helpers ────────────────────────────────────────────────────────────

/// Check that the VM is running before attempting any VM operations.
/// Returns a friendly error instead of letting raw Multipass/SSH errors propagate.
///
/// # Errors
///
/// Returns `WorkspaceError::NotRunning` if the VM is not in the Running state.
pub(crate) async fn ensure_vm_running(provisioner: &impl InstanceInspector) -> Result<()> {
    if vm::state(provisioner).await? != VmState::Running {
        bail!(WorkspaceError::NotRunning)
    }
    Ok(())
}
