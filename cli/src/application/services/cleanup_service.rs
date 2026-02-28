//! Application service — workspace cleanup use-case.

use crate::application::ports::{LocalFs, LocalPaths, ProgressReporter, VmProvisioner};
use crate::application::services::vm::lifecycle as vm;
use anyhow::Result;

/// Delete the workspace VM and clear its state.
pub async fn delete_workspace(
    mp: &impl VmProvisioner,
    state_mgr: &impl crate::application::ports::WorkspaceStateStore,
    reporter: &impl ProgressReporter,
) -> Result<()> {
    // 1. Check if VM exists
    let state = vm::state(mp).await?;
    if state == vm::VmState::NotFound {
        return Ok(());
    }

    // 2. Stop and delete VM
    reporter.step("Stopping and removing workspace VM...");
    vm::delete(mp).await?;

    // 3. Clear workspace state
    state_mgr.clear_async().await?;

    Ok(())
}

/// Delete all workspace data (images, state, agents, config, certs).
pub async fn delete_all(
    mp: &impl VmProvisioner,
    state_mgr: &impl crate::application::ports::WorkspaceStateStore,
    local_fs: &impl LocalFs,
    paths: &impl LocalPaths,
) -> Result<()> {
    // 1. Delete VM and state
    let state = vm::state(mp).await?;
    if state != vm::VmState::NotFound {
        vm::delete(mp).await?;
    }
    state_mgr.clear_async().await?;

    // 2. Remove configuration
    let config_path = paths.config_dir().join("config.yaml");
    if local_fs.exists(&config_path) {
        local_fs.remove(&config_path)?;
    }

    // 3. Remove certificates
    let certs_dir = paths.config_dir().join("certs");
    if local_fs.exists(&certs_dir) {
        local_fs.remove_dir_all(&certs_dir)?;
    }

    // 4. Remove agents
    let agents_dir = paths.config_dir().join("agents");
    if local_fs.exists(&agents_dir) {
        local_fs.remove_dir_all(&agents_dir)?;
    }

    Ok(())
}
