//! Application service — workspace cleanup use-case.

use crate::application::ports::{LocalFs, LocalPaths, ProgressReporter, SshConfigurator, VmProvisioner};
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
        Err(anyhow::anyhow!("Cleanup completed with errors:\n{}", errors.join("\n")))
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
            let config_path = polis_dir.join("config.yaml");
            if local_fs.exists(&config_path) {
                if let Err(e) = local_fs.remove_file(&config_path) {
                    errors.push(format!("Failed to remove config.yaml: {e}"));
                }
            }

            // 4. Remove certificates
            let certs_dir = polis_dir.join("certs");
            if local_fs.exists(&certs_dir) {
                if let Err(e) = local_fs.remove_dir_all(&certs_dir) {
                    errors.push(format!("Failed to remove certs dir: {e}"));
                }
            }

            // 5. Remove agents
            let agents_dir = polis_dir.join("agents");
            if local_fs.exists(&agents_dir) {
                if let Err(e) = local_fs.remove_dir_all(&agents_dir) {
                    errors.push(format!("Failed to remove agents dir: {e}"));
                }
            }

            // 7. Remove SSH known_hosts
            let known_hosts = polis_dir.join("known_hosts");
            if local_fs.exists(&known_hosts) {
                if let Err(e) = local_fs.remove_file(&known_hosts) {
                    errors.push(format!("Failed to remove known_hosts: {e}"));
                }
            }

            // 8. Remove SSH identity keys
            let id_ed25519 = polis_dir.join("id_ed25519");
            if local_fs.exists(&id_ed25519) {
                if let Err(e) = local_fs.remove_file(&id_ed25519) {
                    errors.push(format!("Failed to remove id_ed25519: {e}"));
                }
            }
            let id_ed25519_pub = polis_dir.join("id_ed25519.pub");
            if local_fs.exists(&id_ed25519_pub) {
                if let Err(e) = local_fs.remove_file(&id_ed25519_pub) {
                    errors.push(format!("Failed to remove id_ed25519.pub: {e}"));
                }
            }
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
    let images_dir = paths.images_dir();
    if local_fs.exists(&images_dir) {
        if let Err(e) = local_fs.remove_dir_all(&images_dir) {
            errors.push(format!("Failed to remove images dir: {e}"));
        }
    }

    if errors.is_empty() {
        println!("All Polis data removed.");
        Ok(())
    } else {
        Err(anyhow::anyhow!("Cleanup completed with errors:\n{}", errors.join("\n")))
    }
}
