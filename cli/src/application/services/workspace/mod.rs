//! Workspace services — use-case orchestration for workspace lifecycle operations.
//!
//! Each submodule implements a single workspace use-case by composing building
//! blocks from `crate::application::vm` and `crate::application::provisioning`.

// Submodules — populated by tasks 3.2–3.8
pub mod delete;
pub mod doctor;
pub mod exec;
pub mod repair;
pub mod start;
pub mod status;
pub mod stop;

pub use delete::{CleanupContext, DeleteOutcome, delete, delete_all};
pub use doctor::diagnose;
pub use exec::exec;
pub use repair::repair;
pub use start::start;
pub use status::{gather, workspace_unknown};
pub use stop::stop;

/// Shared guard ensuring the VM is running before workspace operations.
///
/// Returns `WorkspaceError::NotRunning` if the VM is not in the Running state.
pub(crate) async fn ensure_running(
    provisioner: &impl crate::application::ports::InstanceInspector,
) -> anyhow::Result<()> {
    if !crate::application::vm::lifecycle::is_running(provisioner).await? {
        anyhow::bail!(crate::domain::error::WorkspaceError::NotRunning)
    }
    Ok(())
}
