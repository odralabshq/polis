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
    paths: &impl LocalPaths,
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
    let backup_dir = paths
        .polis_dir()
        .context("resolving polis directory")?
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
    paths: &impl LocalPaths,
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
        match backup_workspace(mp, local_fs, paths, reporter).await {
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
            match backup_workspace(mp, local_fs, paths, reporter).await {
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::process::Output;

    use anyhow::Result;

    use crate::application::ports::{
        FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec, LocalFs, LocalPaths,
        ProgressReporter, ShellExecutor,
    };
    use crate::application::services::vm::test_support::{
        fail_output, impl_shell_executor_stubs, ok_output,
    };

    // ── Mocks ─────────────────────────────────────────────────────────────

    /// Stub reporter that records warnings.
    struct TestReporter {
        warnings: std::cell::RefCell<Vec<String>>,
    }
    impl TestReporter {
        fn new() -> Self {
            Self {
                warnings: std::cell::RefCell::new(Vec::new()),
            }
        }
        fn warnings(&self) -> Vec<String> {
            self.warnings.borrow().clone()
        }
    }
    impl ProgressReporter for TestReporter {
        fn step(&self, _: &str) {}
        fn success(&self, _: &str) {}
        fn warn(&self, msg: &str) {
            self.warnings.borrow_mut().push(msg.to_string());
        }
        fn begin_stage(&self, _: &str) {}
        fn complete_stage(&self) {}
        fn fail_stage(&self) {}
    }

    /// Mock provisioner for backup tests.
    #[allow(clippy::struct_field_names)]
    struct BackupMock {
        info_output: Output,
        exec_output: Output,
        transfer_from_output: std::cell::RefCell<Result<Output>>,
    }
    impl BackupMock {
        fn running_ok() -> Self {
            Self {
                info_output: ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#),
                exec_output: ok_output(b"BACKUP_OK\n"),
                transfer_from_output: std::cell::RefCell::new(Ok(ok_output(b""))),
            }
        }
        fn stopped() -> Self {
            Self {
                info_output: ok_output(br#"{"info":{"polis":{"state":"Stopped"}}}"#),
                exec_output: ok_output(b""),
                transfer_from_output: std::cell::RefCell::new(Ok(ok_output(b""))),
            }
        }
        fn script_fails() -> Self {
            Self {
                info_output: ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#),
                exec_output: fail_output(),
                transfer_from_output: std::cell::RefCell::new(Ok(ok_output(b""))),
            }
        }
        fn transfer_fails() -> Self {
            Self {
                info_output: ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#),
                exec_output: ok_output(b"BACKUP_OK\n"),
                transfer_from_output: std::cell::RefCell::new(Ok(fail_output())),
            }
        }
    }
    impl InstanceInspector for BackupMock {
        async fn info(&self) -> Result<Output> {
            Ok(Output {
                status: self.info_output.status,
                stdout: self.info_output.stdout.clone(),
                stderr: self.info_output.stderr.clone(),
            })
        }
        async fn version(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }
    impl InstanceLifecycle for BackupMock {
        async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn start(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn stop(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn delete(&self) -> Result<Output> {
            Ok(ok_output(b""))
        }
        async fn purge(&self) -> Result<Output> {
            Ok(ok_output(b""))
        }
    }
    impl FileTransfer for BackupMock {
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn transfer_from(&self, _: &str, _: &str) -> Result<Output> {
            // Take the value and replace with a bail so double-calls are caught.
            self.transfer_from_output
                .replace(Err(anyhow::anyhow!("already consumed")))
        }
    }
    impl ShellExecutor for BackupMock {
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            Ok(Output {
                status: self.exec_output.status,
                stdout: self.exec_output.stdout.clone(),
                stderr: self.exec_output.stderr.clone(),
            })
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    /// Mock filesystem that creates real temp dirs.
    struct TestFs {
        base: tempfile::TempDir,
    }
    impl TestFs {
        fn new() -> Self {
            Self {
                base: tempfile::tempdir().expect("tempdir"),
            }
        }
    }
    impl LocalFs for TestFs {
        fn exists(&self, path: &std::path::Path) -> bool {
            path.exists()
        }
        fn create_dir_all(&self, path: &std::path::Path) -> Result<()> {
            std::fs::create_dir_all(path)?;
            Ok(())
        }
        fn remove_dir_all(&self, _: &std::path::Path) -> Result<()> {
            Ok(())
        }
        fn remove_file(&self, _: &std::path::Path) -> Result<()> {
            Ok(())
        }
        fn write(&self, _: &std::path::Path, _: String) -> Result<()> {
            Ok(())
        }
        fn read_to_string(&self, _: &std::path::Path) -> Result<String> {
            Ok(String::new())
        }
        fn set_permissions(&self, _: &std::path::Path, _: u32) -> Result<()> {
            Ok(())
        }
    }
    impl LocalPaths for TestFs {
        fn images_dir(&self) -> std::path::PathBuf {
            self.base.path().join("images")
        }
        fn polis_dir(&self) -> Result<std::path::PathBuf> {
            Ok(self.base.path().to_path_buf())
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn backup_skips_when_vm_not_running() {
        let mp = BackupMock::stopped();
        let fs = TestFs::new();
        let reporter = TestReporter::new();
        let result = super::backup_workspace(&mp, &fs, &fs, &reporter)
            .await
            .expect("should not error");
        assert!(result.is_none(), "should return None when VM is stopped");
        assert!(
            reporter
                .warnings()
                .iter()
                .any(|w| w.contains("not running")),
            "should warn about VM not running"
        );
    }

    #[tokio::test]
    async fn backup_returns_none_when_script_fails() {
        let mp = BackupMock::script_fails();
        let fs = TestFs::new();
        let reporter = TestReporter::new();
        let result = super::backup_workspace(&mp, &fs, &fs, &reporter)
            .await
            .expect("should not error");
        assert!(
            result.is_none(),
            "should return None when backup script fails"
        );
        assert!(
            reporter
                .warnings()
                .iter()
                .any(|w| w.contains("backup script failed")),
            "should warn about script failure"
        );
    }

    #[tokio::test]
    async fn backup_returns_none_when_transfer_fails() {
        let mp = BackupMock::transfer_fails();
        let fs = TestFs::new();
        let reporter = TestReporter::new();
        let result = super::backup_workspace(&mp, &fs, &fs, &reporter)
            .await
            .expect("should not error");
        assert!(result.is_none(), "should return None when transfer fails");
        assert!(
            reporter
                .warnings()
                .iter()
                .any(|w| w.contains("failed to transfer")),
            "should warn about transfer failure"
        );
    }

    #[tokio::test]
    async fn backup_returns_path_on_success() {
        let mp = BackupMock::running_ok();
        let fs = TestFs::new();
        let reporter = TestReporter::new();
        let result = super::backup_workspace(&mp, &fs, &fs, &reporter)
            .await
            .expect("should not error");
        assert!(result.is_some(), "should return Some(path) on success");
        let path = result.unwrap();
        assert!(
            path.to_string_lossy().contains("backups"),
            "path should contain 'backups': {}",
            path.display()
        );
        assert!(
            path.to_string_lossy().contains("workspace-"),
            "path should contain 'workspace-': {}",
            path.display()
        );
        assert!(
            path.to_string_lossy().ends_with(".tar.gz"),
            "path should end with .tar.gz: {}",
            path.display()
        );
    }

    #[tokio::test]
    async fn backup_creates_backup_directory() {
        let mp = BackupMock::running_ok();
        let fs = TestFs::new();
        let reporter = TestReporter::new();
        let _ = super::backup_workspace(&mp, &fs, &fs, &reporter).await;
        let backup_dir = fs.base.path().join("backups");
        assert!(
            backup_dir.exists(),
            "backup directory should be created at {backup_dir:?}",
        );
    }
}
