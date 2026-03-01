//! Application service — workspace cleanup use-case.

use crate::application::ports::{
    LocalFs, LocalPaths, ProgressReporter, SshConfigurator, VmProvisioner,
};
use crate::application::services::vm::lifecycle as vm;
use anyhow::Result;

/// Delete the workspace VM and clear its state.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn delete_workspace(
    mp: &impl VmProvisioner,
    state_mgr: &impl crate::application::ports::WorkspaceStateStore,
    reporter: &impl ProgressReporter,
) -> Result<()> {
    // 1. Check if VM exists (fail-fast: prerequisite check)
    let state = vm::state(mp).await?;
    if state == vm::VmState::NotFound {
        return Ok(());
    }

    // 2. Stop and delete VM (fail-fast: prerequisite step)
    reporter.step("Stopping and removing workspace VM...");
    vm::delete(mp).await;

    // 3. Accumulate errors for cleanup steps
    let mut errors: Vec<String> = Vec::new();

    if let Err(e) = state_mgr.clear_async().await {
        errors.push(format!("Failed to clear state: {e}"));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Cleanup completed with errors:\n{}",
            errors.join("\n")
        ))
    }
}

/// Delete all workspace data (images, state, agents, config, certs, SSH artifacts).
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn delete_all(
    mp: &impl VmProvisioner,
    state_mgr: &impl crate::application::ports::WorkspaceStateStore,
    local_fs: &impl LocalFs,
    paths: &impl LocalPaths,
    ssh: &impl SshConfigurator,
) -> Result<()> {
    // 1. Delete VM (fail-fast: prerequisite check)
    let state = vm::state(mp).await?;
    if state != vm::VmState::NotFound {
        vm::delete(mp).await;
    }

    // Accumulate errors for all cleanup steps
    let mut errors: Vec<String> = Vec::new();

    // 2. Clear workspace state
    if let Err(e) = state_mgr.clear_async().await {
        errors.push(format!("Failed to clear state: {e}"));
    }

    // 3. Remove configuration
    match paths.polis_dir() {
        Err(e) => errors.push(format!("Failed to resolve polis dir: {e}")),
        Ok(polis_dir) => {
            remove_if_exists(
                local_fs,
                &polis_dir.join("config.yaml"),
                "config.yaml",
                &mut errors,
            );
            remove_if_exists(local_fs, &polis_dir.join("certs"), "certs dir", &mut errors);
            remove_if_exists(
                local_fs,
                &polis_dir.join("agents"),
                "agents dir",
                &mut errors,
            );
            remove_if_exists(
                local_fs,
                &polis_dir.join("known_hosts"),
                "known_hosts",
                &mut errors,
            );
            remove_if_exists(
                local_fs,
                &polis_dir.join("id_ed25519"),
                "id_ed25519",
                &mut errors,
            );
            remove_if_exists(
                local_fs,
                &polis_dir.join("id_ed25519.pub"),
                "id_ed25519.pub",
                &mut errors,
            );
        }
    }

    // 6. Remove SSH config file
    if let Err(e) = ssh.remove_config().await {
        errors.push(format!("Failed to remove SSH config: {e}"));
    }

    // 9. Remove Include directive from ~/.ssh/config
    if let Err(e) = ssh.remove_include_directive().await {
        errors.push(format!("Failed to remove SSH include directive: {e}"));
    }

    // 10. Remove cached images directory
    remove_if_exists(local_fs, &paths.images_dir(), "images dir", &mut errors);

    if errors.is_empty() {
        println!("All Polis data removed.");
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Cleanup completed with errors:\n{}",
            errors.join("\n")
        ))
    }
}

/// Try to remove a path (file or directory) if it exists, collecting errors.
fn remove_if_exists(
    fs: &impl LocalFs,
    path: &std::path::Path,
    label: &str,
    errors: &mut Vec<String>,
) {
    if !fs.exists(path) {
        return;
    }
    let result = if path.is_dir() {
        fs.remove_dir_all(path)
    } else {
        fs.remove_file(path)
    };
    if let Err(e) = result {
        errors.push(format!("Failed to remove {label}: {e}"));
    }
}
