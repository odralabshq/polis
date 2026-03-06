//! Application service — workspace cleanup use-case.

use crate::application::ports::{
    InstanceInspector, InstanceLifecycle, LocalFs, LocalPaths, ProgressReporter, ShellExecutor,
    SshConfigurator,
};
use crate::application::services::vm::lifecycle as vm;
use anyhow::Result;

/// Outcome of the `delete_workspace` use-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// Workspace VM was deleted successfully.
    Deleted,
    /// No workspace VM found (already deleted or never created).
    NotFound,
}

/// Delete the workspace VM and clear its state.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn delete_workspace(
    provisioner: &(impl InstanceInspector + InstanceLifecycle + ShellExecutor),
    state_mgr: &impl crate::application::ports::WorkspaceStateStore,
    reporter: &impl ProgressReporter,
) -> Result<DeleteOutcome> {
    // 1. Check if VM exists (fail-fast: prerequisite check)
    let state = vm::state(provisioner).await?;
    if state == vm::VmState::NotFound {
        return Ok(DeleteOutcome::NotFound);
    }

    // 2. Stop containers before deletion (best-effort)
    if matches!(state, vm::VmState::Running | vm::VmState::Starting) {
        let _ = provisioner
            .exec(&[
                "bash",
                "-c",
                "docker ps -q --filter name=polis- | xargs -r docker stop",
            ])
            .await;
    }

    // 3. Stop and delete VM (fail-fast: prerequisite step)
    reporter.begin_stage("removing workspace...");
    vm::delete(provisioner).await?;
    reporter.complete_stage();

    // 3. Accumulate errors for cleanup steps
    reporter.begin_stage("cleaning up...");
    let mut errors: Vec<String> = Vec::new();

    if let Err(e) = state_mgr.clear_async().await {
        errors.push(format!("Failed to clear state: {e}"));
    }

    if errors.is_empty() {
        reporter.complete_stage();
        Ok(DeleteOutcome::Deleted)
    } else {
        reporter.fail_stage();
        Err(anyhow::anyhow!(
            "Cleanup completed with errors:\n{}",
            errors.join("\n")
        ))
    }
}

/// Groups the dependencies required by `delete_all`, keeping the function
/// signature stable as new dependencies are added.
pub struct CleanupContext<'a, P, S, F, L, C, R> {
    pub provisioner: &'a P,
    pub state_store: &'a S,
    pub local_fs: &'a F,
    pub paths: &'a L,
    pub ssh: &'a C,
    pub reporter: &'a R,
}

/// Delete all workspace data (images, state, agents, config, certs, SSH artifacts).
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn delete_all<P, S, F, L, C, R>(ctx: &CleanupContext<'_, P, S, F, L, C, R>) -> Result<()>
where
    P: InstanceInspector + InstanceLifecycle + ShellExecutor,
    S: crate::application::ports::WorkspaceStateStore,
    F: LocalFs,
    L: LocalPaths,
    C: SshConfigurator,
    R: ProgressReporter,
{
    // 1. Check VM state
    let state = vm::state(ctx.provisioner).await?;
    if state != vm::VmState::NotFound {
        // 2. Stop containers before deletion (best-effort)
        if matches!(state, vm::VmState::Running | vm::VmState::Starting) {
            let _ = ctx
                .provisioner
                .exec(&[
                    "bash",
                    "-c",
                    "docker ps -q --filter name=polis- | xargs -r docker stop",
                ])
                .await;
        }
        // 3. Delete VM
        ctx.reporter.begin_stage("removing workspace...");
        vm::delete(ctx.provisioner).await?;
        ctx.reporter.complete_stage();
    }

    // Accumulate errors for all cleanup steps
    ctx.reporter.begin_stage("cleaning up...");
    let mut errors: Vec<String> = Vec::new();

    // 4. Clear workspace state
    if let Err(e) = ctx.state_store.clear_async().await {
        errors.push(format!("Failed to clear state: {e}"));
    }

    // 5. Remove configuration files
    match ctx.paths.polis_dir() {
        Err(e) => errors.push(format!("Failed to resolve polis dir: {e}")),
        Ok(polis_dir) => {
            remove_if_exists(
                ctx.local_fs,
                &polis_dir.join("config.yaml"),
                "config.yaml",
                &mut errors,
            );
            remove_if_exists(
                ctx.local_fs,
                &polis_dir.join("certs"),
                "certs dir",
                &mut errors,
            );
            remove_if_exists(
                ctx.local_fs,
                &polis_dir.join("agents"),
                "agents dir",
                &mut errors,
            );
            remove_if_exists(
                ctx.local_fs,
                &polis_dir.join("known_hosts"),
                "known_hosts",
                &mut errors,
            );
            remove_if_exists(
                ctx.local_fs,
                &polis_dir.join("id_ed25519"),
                "id_ed25519",
                &mut errors,
            );
            remove_if_exists(
                ctx.local_fs,
                &polis_dir.join("id_ed25519.pub"),
                "id_ed25519.pub",
                &mut errors,
            );
        }
    }

    // 6. Remove SSH config file
    if let Err(e) = ctx.ssh.remove_config().await {
        errors.push(format!("Failed to remove SSH config: {e}"));
    }

    // 7. Remove Include directive from ~/.ssh/config
    if let Err(e) = ctx.ssh.remove_include_directive().await {
        errors.push(format!("Failed to remove SSH include directive: {e}"));
    }

    // 8. Remove cached images directory
    remove_if_exists(ctx.local_fs, &ctx.paths.images_dir(), "images dir", &mut errors);

    if errors.is_empty() {
        ctx.reporter.complete_stage();
        ctx.reporter.success("all workspace data removed");
        Ok(())
    } else {
        ctx.reporter.fail_stage();
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
    let result = if fs.is_dir(path) {
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
    use std::cell::Cell;
    use std::path::Path;
    use std::process::Output;

    use anyhow::Result;

    use super::*;
    use crate::application::ports::{InstanceInspector, InstanceLifecycle, InstanceSpec, ShellExecutor, WorkspaceStateStore};
    use crate::application::services::vm::test_support::{fail_output, impl_shell_executor_stubs, ok_output};

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn running_info() -> Output {
        ok_output(br#"{"info":{"polis":{"state":"Running","ipv4":[]}}}"#)
    }

    fn not_found_info() -> Output {
        fail_output()
    }

    // ── InstanceInspector stub ────────────────────────────────────────────────

    struct InspectorStub {
        info_output: Output,
    }

    impl InstanceInspector for InspectorStub {
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

    // ── InstanceLifecycle stub ────────────────────────────────────────────────

    struct LifecycleStub {
        delete_fails: bool,
        delete_called: Cell<bool>,
    }

    impl LifecycleStub {
        fn new(delete_fails: bool) -> Self {
            Self { delete_fails, delete_called: Cell::new(false) }
        }
    }

    impl InstanceLifecycle for LifecycleStub {
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
            self.delete_called.set(true);
            if self.delete_fails {
                anyhow::bail!("delete failed")
            }
            Ok(ok_output(b""))
        }
        async fn purge(&self) -> Result<Output> {
            Ok(ok_output(b""))
        }
    }

    // ── ShellExecutor stub ────────────────────────────────────────────────────

    struct ExecSpy {
        exec_fails: bool,
        exec_called: Cell<bool>,
        last_cmd: std::cell::RefCell<String>,
    }

    impl ExecSpy {
        fn new(exec_fails: bool) -> Self {
            Self {
                exec_fails,
                exec_called: Cell::new(false),
                last_cmd: std::cell::RefCell::new(String::new()),
            }
        }
    }

    impl ShellExecutor for ExecSpy {
        async fn exec(&self, args: &[&str]) -> Result<Output> {
            self.exec_called.set(true);
            *self.last_cmd.borrow_mut() = args.join(" ");
            if self.exec_fails {
                anyhow::bail!("exec failed")
            }
            Ok(ok_output(b""))
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    // ── Composite provisioner stub ────────────────────────────────────────────

    struct ProvisionerStub {
        inspector: InspectorStub,
        lifecycle: LifecycleStub,
        exec_spy: ExecSpy,
    }

    impl ProvisionerStub {
        fn running(delete_fails: bool, exec_fails: bool) -> Self {
            Self {
                inspector: InspectorStub { info_output: running_info() },
                lifecycle: LifecycleStub::new(delete_fails),
                exec_spy: ExecSpy::new(exec_fails),
            }
        }

        fn not_found() -> Self {
            Self {
                inspector: InspectorStub { info_output: not_found_info() },
                lifecycle: LifecycleStub::new(false),
                exec_spy: ExecSpy::new(false),
            }
        }
    }

    impl InstanceInspector for ProvisionerStub {
        async fn info(&self) -> Result<Output> {
            self.inspector.info().await
        }
        async fn version(&self) -> Result<Output> {
            self.inspector.version().await
        }
    }

    impl InstanceLifecycle for ProvisionerStub {
        async fn launch(&self, spec: &InstanceSpec<'_>) -> Result<Output> {
            self.lifecycle.launch(spec).await
        }
        async fn start(&self) -> Result<Output> {
            self.lifecycle.start().await
        }
        async fn stop(&self) -> Result<Output> {
            self.lifecycle.stop().await
        }
        async fn delete(&self) -> Result<Output> {
            self.lifecycle.delete().await
        }
        async fn purge(&self) -> Result<Output> {
            self.lifecycle.purge().await
        }
    }

    impl ShellExecutor for ProvisionerStub {
        async fn exec(&self, args: &[&str]) -> Result<Output> {
            self.exec_spy.exec(args).await
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    // ── WorkspaceStateStore stub ──────────────────────────────────────────────

    struct StateStoreStub {
        clear_fails: bool,
    }

    impl WorkspaceStateStore for StateStoreStub {
        async fn load_async(&self) -> Result<Option<crate::domain::workspace::WorkspaceState>> {
            anyhow::bail!("not expected")
        }
        async fn save_async(&self, _: &crate::domain::workspace::WorkspaceState) -> Result<()> {
            anyhow::bail!("not expected")
        }
        async fn clear_async(&self) -> Result<()> {
            if self.clear_fails {
                anyhow::bail!("clear failed")
            }
            Ok(())
        }
    }

    // ── ProgressReporter stub ─────────────────────────────────────────────────

    struct ReporterStub;

    impl ProgressReporter for ReporterStub {
        fn step(&self, _: &str) {}
        fn success(&self, _: &str) {}
        fn warn(&self, _: &str) {}
    }

    // ── LocalFs stub ──────────────────────────────────────────────────────────

    struct FsStub {
        exists: bool,
        is_dir: bool,
        remove_fails: bool,
    }

    impl LocalFs for FsStub {
        fn exists(&self, _: &Path) -> bool {
            self.exists
        }
        fn is_dir(&self, _: &Path) -> bool {
            self.is_dir
        }
        fn create_dir_all(&self, _: &Path) -> Result<()> {
            Ok(())
        }
        fn remove_dir_all(&self, _: &Path) -> Result<()> {
            if self.remove_fails {
                anyhow::bail!("remove_dir_all failed")
            }
            Ok(())
        }
        fn remove_file(&self, _: &Path) -> Result<()> {
            if self.remove_fails {
                anyhow::bail!("remove_file failed")
            }
            Ok(())
        }
        fn write(&self, _: &Path, _: String) -> Result<()> {
            Ok(())
        }
        fn read_to_string(&self, _: &Path) -> Result<String> {
            Ok(String::new())
        }
        fn set_permissions(&self, _: &Path, _: u32) -> Result<()> {
            Ok(())
        }
    }

    // ── LocalPaths stub ───────────────────────────────────────────────────────

    struct PathsStub {
        polis_dir_fails: bool,
    }

    impl LocalPaths for PathsStub {
        fn images_dir(&self) -> std::path::PathBuf {
            std::path::PathBuf::from("/tmp/images")
        }
        fn polis_dir(&self) -> Result<std::path::PathBuf> {
            if self.polis_dir_fails {
                anyhow::bail!("polis_dir failed")
            }
            Ok(std::path::PathBuf::from("/tmp/polis"))
        }
    }

    // ── SshConfigurator stub ──────────────────────────────────────────────────

    struct SshStub {
        remove_config_fails: bool,
        remove_include_fails: bool,
    }

    impl SshConfigurator for SshStub {
        async fn ensure_identity(&self) -> Result<String> {
            anyhow::bail!("not expected")
        }
        async fn update_host_key(&self, _: &str) -> Result<()> {
            anyhow::bail!("not expected")
        }
        async fn is_configured(&self) -> Result<bool> {
            anyhow::bail!("not expected")
        }
        async fn setup_config(&self) -> Result<()> {
            anyhow::bail!("not expected")
        }
        async fn validate_permissions(&self) -> Result<()> {
            anyhow::bail!("not expected")
        }
        async fn remove_config(&self) -> Result<()> {
            if self.remove_config_fails {
                anyhow::bail!("remove_config failed")
            }
            Ok(())
        }
        async fn remove_include_directive(&self) -> Result<()> {
            if self.remove_include_fails {
                anyhow::bail!("remove_include_directive failed")
            }
            Ok(())
        }
    }

    // ── delete_workspace tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_workspace_not_found() {
        let provisioner = ProvisionerStub::not_found();
        let state_store = StateStoreStub { clear_fails: false };
        let reporter = ReporterStub;

        let outcome = delete_workspace(&provisioner, &state_store, &reporter)
            .await
            .expect("should succeed");

        assert_eq!(outcome, DeleteOutcome::NotFound);
        assert!(!provisioner.lifecycle.delete_called.get(), "delete should NOT be called");
    }

    #[tokio::test]
    async fn delete_workspace_success() {
        let provisioner = ProvisionerStub::running(false, false);
        let state_store = StateStoreStub { clear_fails: false };
        let reporter = ReporterStub;

        let outcome = delete_workspace(&provisioner, &state_store, &reporter)
            .await
            .expect("should succeed");

        assert_eq!(outcome, DeleteOutcome::Deleted);
        assert!(provisioner.lifecycle.delete_called.get(), "delete should be called");
    }

    #[tokio::test]
    async fn delete_workspace_vm_delete_fails() {
        let provisioner = ProvisionerStub::running(true, false);
        let state_store = StateStoreStub { clear_fails: false };
        let reporter = ReporterStub;

        let err = delete_workspace(&provisioner, &state_store, &reporter)
            .await
            .expect_err("should fail");

        let msg = format!("{err:#}");
        assert!(msg.contains("delete failed"), "unexpected error: {msg}");
    }

    #[tokio::test]
    async fn delete_workspace_state_clear_fails() {
        let provisioner = ProvisionerStub::running(false, false);
        let state_store = StateStoreStub { clear_fails: true };
        let reporter = ReporterStub;

        let err = delete_workspace(&provisioner, &state_store, &reporter)
            .await
            .expect_err("should fail");

        let msg = format!("{err:#}");
        assert!(msg.contains("clear failed"), "unexpected error: {msg}");
    }

    // ── container stop behavior tests ─────────────────────────────────────────

    #[tokio::test]
    async fn delete_workspace_stops_containers_when_running() {
        let provisioner = ProvisionerStub::running(false, false);
        let state_store = StateStoreStub { clear_fails: false };
        let reporter = ReporterStub;

        delete_workspace(&provisioner, &state_store, &reporter)
            .await
            .expect("should succeed");

        assert!(provisioner.exec_spy.exec_called.get(), "exec should be called for container stop");
        let cmd = provisioner.exec_spy.last_cmd.borrow();
        assert!(
            cmd.contains("docker ps") && cmd.contains("docker stop"),
            "exec should use docker stop filter, got: {cmd}"
        );
    }

    #[tokio::test]
    async fn delete_workspace_container_stop_failure_continues() {
        let provisioner = ProvisionerStub::running(false, true);
        let state_store = StateStoreStub { clear_fails: false };
        let reporter = ReporterStub;

        let outcome = delete_workspace(&provisioner, &state_store, &reporter)
            .await
            .expect("should succeed despite exec failure");

        assert_eq!(outcome, DeleteOutcome::Deleted);
        assert!(provisioner.exec_spy.exec_called.get(), "exec should be called");
        assert!(provisioner.lifecycle.delete_called.get(), "delete should still be called");
    }

    // ── delete_all tests ──────────────────────────────────────────────────────

    fn make_cleanup_ctx<'a>(
        provisioner: &'a ProvisionerStub,
        state_store: &'a StateStoreStub,
        fs: &'a FsStub,
        paths: &'a PathsStub,
        ssh: &'a SshStub,
        reporter: &'a ReporterStub,
    ) -> CleanupContext<'a, ProvisionerStub, StateStoreStub, FsStub, PathsStub, SshStub, ReporterStub> {
        CleanupContext { provisioner, state_store, local_fs: fs, paths, ssh, reporter }
    }

    #[tokio::test]
    async fn delete_all_success() {
        let provisioner = ProvisionerStub::running(false, false);
        let state_store = StateStoreStub { clear_fails: false };
        let fs = FsStub { exists: false, is_dir: false, remove_fails: false };
        let paths = PathsStub { polis_dir_fails: false };
        let ssh = SshStub { remove_config_fails: false, remove_include_fails: false };
        let reporter = ReporterStub;

        let ctx = make_cleanup_ctx(&provisioner, &state_store, &fs, &paths, &ssh, &reporter);
        let result = delete_all(&ctx).await;

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }

    #[tokio::test]
    async fn delete_all_partial_failure() {
        let provisioner = ProvisionerStub::running(false, false);
        let state_store = StateStoreStub { clear_fails: true };
        let fs = FsStub { exists: false, is_dir: false, remove_fails: false };
        let paths = PathsStub { polis_dir_fails: false };
        let ssh = SshStub { remove_config_fails: true, remove_include_fails: false };
        let reporter = ReporterStub;

        let ctx = make_cleanup_ctx(&provisioner, &state_store, &fs, &paths, &ssh, &reporter);
        let err = delete_all(&ctx).await.expect_err("should fail with accumulated errors");

        let msg = err.to_string();
        assert!(msg.contains("clear failed"), "expected 'clear failed' in: {msg}");
        assert!(msg.contains("remove_config failed"), "expected 'remove_config failed' in: {msg}");
    }
}
