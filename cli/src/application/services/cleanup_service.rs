//! Application service — workspace cleanup use-case.

use crate::application::ports::{
    LocalFs, LocalPaths, ProgressReporter, SshConfigurator, VmProvisioner,
};
use crate::application::services::vm::lifecycle as vm;
use anyhow::{Context, Result};

// ── Backup ────────────────────────────────────────────────────────────────────

/// Back up workspace data (Docker volumes + config) before deletion.
///
/// Creates a timestamped `.tar.gz` archive in `~/.polis/backups/`.
/// Best-effort: returns `Ok(None)` if the VM is not running or backup fails
/// non-fatally, so deletion can still proceed.
///
/// # Errors
///
/// Returns an error only if a critical filesystem operation fails on the host.
pub async fn backup_workspace(
    mp: &impl VmProvisioner,
    local_fs: &impl LocalFs,
    reporter: &impl ProgressReporter,
) -> Result<Option<std::path::PathBuf>> {
    // Only attempt backup if VM is running (we need Docker to export volumes).
    let state = vm::state(mp).await?;
    if state != vm::VmState::Running {
        reporter.warn("VM not running — skipping backup (no data to export)");
        return Ok(None);
    }

    reporter.begin_stage("backing up workspace data...");

    let script = r#"
set -e
BACKUP_DIR=/tmp/polis-backup
BACKUP_FILE=/tmp/polis-workspace-backup.tar.gz
rm -rf "$BACKUP_DIR" "$BACKUP_FILE"
mkdir -p "$BACKUP_DIR"

# Backup Docker volumes (agent data, workspace files)
for vol in $(docker volume ls -q 2>/dev/null | grep -E '^polis-' || true); do
    docker run --rm -v "${vol}:/data:ro" -v "$BACKUP_DIR:/backup" alpine \
        tar czf "/backup/${vol}.tar.gz" -C /data . 2>/dev/null || true
done

# Backup .env and agent configs from the VM
cp /opt/polis/.env "$BACKUP_DIR/dot-env" 2>/dev/null || true
tar czf "$BACKUP_DIR/agent-configs.tar.gz" \
    -C /opt/polis agents/ 2>/dev/null || true

# Create final archive
tar czf "$BACKUP_FILE" -C "$BACKUP_DIR" .
rm -rf "$BACKUP_DIR"
echo "BACKUP_OK"
"#;

    let output = mp
        .exec(&["bash", "-c", script])
        .await
        .context("running backup script in VM")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !stdout.contains("BACKUP_OK") {
        reporter.fail_stage();
        reporter.warn("backup script failed — proceeding without backup");
        return Ok(None);
    }

    // Prepare local backup directory
    let backup_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".polis")
        .join("backups");
    local_fs
        .create_dir_all(&backup_dir)
        .context("creating backup directory")?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("workspace-{timestamp}.tar.gz");
    let local_path = backup_dir.join(&filename);

    let transfer_result = mp
        .transfer_from(
            "/tmp/polis-workspace-backup.tar.gz",
            &local_path.to_string_lossy(),
        )
        .await;

    match transfer_result {
        Ok(ref o) if o.status.success() => {
            reporter.complete_stage();
            Ok(Some(local_path))
        }
        _ => {
            reporter.fail_stage();
            reporter.warn("failed to transfer backup from VM — proceeding without backup");
            Ok(None)
        }
    }
}

/// Delete the workspace VM and clear its state.
///
/// When `skip_backup` is false, attempts to back up Docker volumes and config
/// before destroying the VM.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn delete_workspace(
    mp: &impl VmProvisioner,
    state_mgr: &impl crate::application::ports::WorkspaceStateStore,
    local_fs: &impl LocalFs,
    reporter: &impl ProgressReporter,
    skip_backup: bool,
) -> Result<()> {
    // 1. Check if VM exists (fail-fast: prerequisite check)
    let state = vm::state(mp).await?;
    if state == vm::VmState::NotFound {
        reporter.success("workspace not found (already deleted)");
        return Ok(());
    }

    // 2. Backup workspace data before deletion
    if !skip_backup {
        match backup_workspace(mp, local_fs, reporter).await {
            Ok(Some(path)) => {
                reporter.success(&format!("backup saved to {}", path.display()));
            }
            Ok(None) => { /* warning already printed */ }
            Err(e) => {
                reporter.warn(&format!("backup failed: {e} — proceeding with delete"));
            }
        }
    }

    // 3. Stop and delete VM (fail-fast: prerequisite step)
    reporter.begin_stage("removing workspace...");
    vm::delete(mp).await;
    reporter.complete_stage();

    // 4. Accumulate errors for cleanup steps
    reporter.begin_stage("cleaning up...");
    let mut errors: Vec<String> = Vec::new();

    if let Err(e) = state_mgr.clear_async().await {
        errors.push(format!("Failed to clear state: {e}"));
    }

    if errors.is_empty() {
        reporter.complete_stage();
        reporter.success("workspace removed");
        Ok(())
    } else {
        reporter.fail_stage();
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
    reporter: &impl ProgressReporter,
    skip_backup: bool,
) -> Result<()> {
    // 1. Delete VM (fail-fast: prerequisite check)
    let state = vm::state(mp).await?;
    if state != vm::VmState::NotFound {
        // Backup before deletion
        if !skip_backup {
            match backup_workspace(mp, local_fs, reporter).await {
                Ok(Some(path)) => {
                    reporter.success(&format!("backup saved to {}", path.display()));
                }
                Ok(None) => { /* warning already printed */ }
                Err(e) => {
                    reporter.warn(&format!("backup failed: {e} — proceeding with delete"));
                }
            }
        }

        reporter.begin_stage("removing workspace...");
        vm::delete(mp).await;
        reporter.complete_stage();
    }

    // Accumulate errors for all cleanup steps
    reporter.begin_stage("cleaning up...");
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
        reporter.complete_stage();
        reporter.success("all workspace data removed");
        Ok(())
    } else {
        reporter.fail_stage();
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
