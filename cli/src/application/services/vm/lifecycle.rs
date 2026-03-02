//! VM lifecycle operations: create, start, stop, delete, restart, state.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use anyhow::{Context, Result};

use crate::application::ports::{
    AssetExtractor, FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec,
    ProgressReporter, ShellExecutor, SshConfigurator, VmProvisioner,
};

const VM_CPUS: &str = "2";
const VM_MEMORY: &str = "8G";
const VM_DISK: &str = "40G";

/// VM state as observed from the provisioner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    NotFound,
    Stopped,
    Starting,
    Running,
}

/// Check if VM exists.
pub async fn exists(mp: &impl InstanceInspector) -> bool {
    mp.info().await.map(|o| o.status.success()).unwrap_or(false)
}

/// Get current VM state.
///
/// # Errors
///
/// Returns an error if the multipass output cannot be parsed.
pub async fn state(mp: &impl InstanceInspector) -> Result<VmState> {
    let output = match mp.info().await {
        Ok(o) if o.status.success() => o,
        _ => return Ok(VmState::NotFound),
    };
    let info: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parsing multipass info")?;
    let state_str = info
        .get("info")
        .and_then(|i| i.get("polis"))
        .and_then(|p| p.get("state"))
        .and_then(|s| s.as_str())
        .unwrap_or("Unknown");
    Ok(match state_str {
        "Running" => VmState::Running,
        "Starting" => VmState::Starting,
        _ => VmState::Stopped,
    })
}

/// Resolve the primary IPv4 address of the polis VM.
///
/// Parses `multipass info --format json` output to extract the first IPv4
/// address from `info.polis.ipv4`.
///
/// # Errors
///
/// Returns an error if `info()` fails or no IPv4 address is found.
pub async fn resolve_vm_ip(mp: &impl InstanceInspector) -> Result<String> {
    let output = mp.info().await.context("failed to query VM info")?;
    anyhow::ensure!(output.status.success(), "multipass info failed");

    let info: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("invalid JSON from multipass info")?;

    info.get("info")
        .and_then(|i| i.get("polis"))
        .and_then(|p| p.get("ipv4"))
        .and_then(|arr| arr.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("no IPv4 address found for polis VM"))
}

/// Verify that cloud-init completed successfully inside the VM.
///
/// Runs `cloud-init status --wait` and maps the exit code:
/// - `0` → success, proceed to Phase 2
/// - `1` → critical failure (cloud-init reported a fatal error)
/// - `2` → degraded (cloud-init completed with warnings/non-fatal errors)
///
/// # Errors
///
/// Returns an error if cloud-init reported a failure (exit code 1 or 2), or
/// if the command could not be executed.
pub async fn verify_cloud_init(mp: &impl ShellExecutor) -> Result<()> {
    const LOG: &str = "/var/log/cloud-init-output.log";
    const RECOVERY: &str = "polis delete && polis start";

    let status = mp
        .exec_status(&["cloud-init", "status", "--wait"])
        .await
        .context("running cloud-init status")?;

    match status.code() {
        Some(0) => Ok(()),
        Some(1) => anyhow::bail!(
            "Cloud-init reported a critical failure.\n\
             Check the log for details: {LOG}\n\
             To recover, run: {RECOVERY}"
        ),
        Some(2) => anyhow::bail!(
            "Cloud-init completed in a degraded state.\n\
             Check the log for details: {LOG}\n\
             To recover, run: {RECOVERY}"
        ),
        Some(code) => anyhow::bail!(
            "Cloud-init exited with unexpected code {code}.\n\
             Check the log for details: {LOG}\n\
             To recover, run: {RECOVERY}"
        ),
        None => anyhow::bail!(
            "Cloud-init was terminated by a signal.\n\
             Check the log for details: {LOG}\n\
             To recover, run: {RECOVERY}"
        ),
    }
}

/// Create VM using cloud-init provisioning.
///
/// Extracts the embedded `cloud-init.yaml` to a temporary directory, then
/// invokes `multipass launch 24.04 --cloud-init <path> --timeout 900`.
/// After launch completes, verifies that cloud-init succeeded before returning.
///
/// # Errors
///
/// Returns an error if prerequisites are not met, asset extraction fails,
/// the multipass launch fails, or cloud-init reports a failure.
pub async fn create(
    mp: &impl VmProvisioner,
    assets: &impl AssetExtractor,
    ssh: &impl SshConfigurator,
    reporter: &impl ProgressReporter,
    quiet: bool,
) -> Result<()> {
    check_prerequisites(mp).await?;

    // Extract embedded assets (cloud-init.yaml, etc.) to a temp dir.
    let (assets_path, _assets_guard) = assets
        .extract_assets()
        .await
        .context("extracting embedded assets")?;

    // The Multipass daemon (especially snap-confined) runs as a separate user
    // and needs read access to the cloud-init file and its parent directory.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&assets_path, std::fs::Permissions::from_mode(0o755))
            .context("setting temp dir permissions for multipass")?;
        std::fs::set_permissions(
            assets_path.join("cloud-init.yaml"),
            std::fs::Permissions::from_mode(0o644),
        )
        .context("setting cloud-init.yaml permissions for multipass")?;
    }

    let cloud_init_path = assets_path.join("cloud-init.yaml");
    let cloud_init_str = cloud_init_path
        .to_str()
        .context("cloud-init path is not valid UTF-8")?
        .to_string();

    if !quiet {
        reporter.step("preparing workspace...");
    }
    let output = mp
        .launch(&InstanceSpec {
            image: "24.04",
            cpus: VM_CPUS,
            memory: VM_MEMORY,
            disk: VM_DISK,
            cloud_init: Some(&cloud_init_str),
            timeout: Some("900"),
        })
        .await
        .context("launching workspace")?;
    if !quiet && output.status.success() {
        reporter.success("workspace prepared");
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to create workspace.\n\nRun 'polis doctor' to diagnose.\n{stderr}");
    }

    // Verify cloud-init completed successfully before proceeding to Phase 2.
    verify_cloud_init(mp).await?;

    configure_credentials(mp).await;
    super::services::start_services_with_progress(mp, reporter, quiet).await;
    pin_host_key(ssh).await;
    Ok(())
}

/// Start existing VM.
///
/// # Errors
///
/// Returns an error if the multipass start command fails.
pub async fn start(mp: &impl InstanceLifecycle) -> Result<()> {
    let output = mp.start().await.context("starting workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to start workspace: {stderr}");
    }
    Ok(())
}

/// Stop VM.
///
/// # Errors
///
/// Returns an error if the multipass stop command fails.
pub async fn stop(mp: &(impl InstanceLifecycle + ShellExecutor)) -> Result<()> {
    // Stop all polis- containers (including agent sidecars not in the base
    // compose file). Using `docker stop` with a filter is more reliable than
    // `docker compose stop` which only knows about services in its file.
    let _ = mp
        .exec(&[
            "bash",
            "-c",
            "docker ps -q --filter name=polis- | xargs -r docker stop",
        ])
        .await;
    let output = mp.stop().await.context("stopping workspace")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to stop workspace: {stderr}");
    }
    Ok(())
}

/// Delete VM.
pub async fn delete(mp: &impl InstanceLifecycle) {
    let _ = mp.delete().await;
    let _ = mp.purge().await;
}

/// Restart a stopped VM.
///
/// # Errors
///
/// Returns an error if the multipass start command fails.
pub async fn restart(
    mp: &(impl InstanceLifecycle + ShellExecutor),
    reporter: &impl ProgressReporter,
    quiet: bool,
) -> Result<()> {
    if !quiet {
        reporter.step("starting workspace...");
    }
    start(mp).await?;
    if !quiet {
        reporter.success("workspace started");
    }

    super::services::start_services_with_progress(mp, reporter, quiet).await;
    Ok(())
}

// ── Private helpers ──────────────────────────────────────────────────────────

const MULTIPASS_MIN_VERSION: semver::Version = semver::Version::new(1, 16, 0);

/// # Errors
///
/// This function will return an error if the underlying operations fail.
async fn check_prerequisites(mp: &impl InstanceInspector) -> Result<()> {
    let output = mp.version().await.map_err(|_| {
        anyhow::anyhow!(
            "Workspace runtime not available.\n\nRun 'polis doctor' to diagnose and fix."
        )
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(ver_str) = stdout
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        && let Ok(v) = semver::Version::parse(ver_str)
        && v < MULTIPASS_MIN_VERSION
    {
        anyhow::bail!("workspace runtime needs update.\n\nRun 'polis doctor' to diagnose and fix.");
    }
    Ok(())
}

async fn configure_credentials(mp: &impl FileTransfer) {
    let ca_cert = std::path::PathBuf::from("certs/ca/ca.pem");
    if ca_cert.exists() {
        let _ = mp.transfer(&ca_cert.to_string_lossy(), "/tmp/ca.pem").await;
    }
}

async fn pin_host_key(ssh: &impl SshConfigurator) {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("polis"));
    if let Ok(output) = tokio::process::Command::new(exe)
        .args(["_extract-host-key"])
        .output()
        .await
        && output.status.success()
        && let Ok(host_key) = String::from_utf8(output.stdout)
    {
        let _ = ssh.update_host_key(host_key.trim()).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::process::Output;

    use anyhow::Result;

    use super::*;
    use crate::application::ports::{
        InstanceInspector, InstanceLifecycle, InstanceSpec, ShellExecutor,
    };
    use crate::application::services::vm::test_support::{
        exit_status, fail_output, impl_shell_executor_stubs, ok_output,
    };

    fn ok(stdout: &[u8]) -> Output {
        ok_output(stdout)
    }
    fn fail() -> Output {
        fail_output()
    }

    struct MultipassVmInfoStub(Output);
    impl InstanceInspector for MultipassVmInfoStub {
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn info(&self) -> Result<Output> {
            Ok(Output {
                status: self.0.status,
                stdout: self.0.stdout.clone(),
                stderr: self.0.stderr.clone(),
            })
        }
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn version(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }

    #[tokio::test]
    async fn state_not_found_when_vm_info_fails() {
        let mp = MultipassVmInfoStub(fail());
        assert_eq!(state(&mp).await.expect("state"), VmState::NotFound);
    }

    #[tokio::test]
    async fn state_running() {
        let mp = MultipassVmInfoStub(ok(br#"{"info":{"polis":{"state":"Running"}}}"#));
        assert_eq!(state(&mp).await.expect("state"), VmState::Running);
    }

    #[tokio::test]
    async fn state_stopped() {
        let mp = MultipassVmInfoStub(ok(br#"{"info":{"polis":{"state":"Stopped"}}}"#));
        assert_eq!(state(&mp).await.expect("state"), VmState::Stopped);
    }

    #[tokio::test]
    async fn exists_true_when_vm_info_succeeds() {
        let mp = MultipassVmInfoStub(ok(b"{}"));
        assert!(exists(&mp).await);
    }

    #[tokio::test]
    async fn exists_false_when_vm_info_fails() {
        let mp = MultipassVmInfoStub(fail());
        assert!(!exists(&mp).await);
    }

    struct MultipassRestartSpy {
        start_called: std::cell::Cell<bool>,
        exec_called: std::cell::Cell<bool>,
    }
    impl MultipassRestartSpy {
        fn new() -> Self {
            Self {
                start_called: std::cell::Cell::new(false),
                exec_called: std::cell::Cell::new(false),
            }
        }
    }
    impl InstanceLifecycle for MultipassRestartSpy {
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn start(&self) -> Result<Output> {
            self.start_called.set(true);
            Ok(ok(b""))
        }
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn stop(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn delete(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn purge(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }
    impl ShellExecutor for MultipassRestartSpy {
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            self.exec_called.set(true);
            Ok(ok(b""))
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    struct ReporterStub;
    impl ProgressReporter for ReporterStub {
        fn step(&self, _: &str) {}
        fn success(&self, _: &str) {}
        fn warn(&self, _: &str) {}
    }

    #[tokio::test]
    async fn restart_calls_start_and_services() {
        let mp = MultipassRestartSpy::new();
        let result = restart(&mp, &ReporterStub, true).await;
        assert!(result.is_ok());
        assert!(mp.start_called.get(), "start() should be called");
        assert!(
            mp.exec_called.get(),
            "exec() should be called for systemctl"
        );
    }

    struct MultipassExitStatusStub(i32);
    impl ShellExecutor for MultipassExitStatusStub {
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            Ok(exit_status(self.0))
        }
        impl_shell_executor_stubs!(exec, exec_with_stdin, exec_spawn);
    }

    #[tokio::test]
    async fn verify_cloud_init_succeeds_on_exit_code_0() {
        let mp = MultipassExitStatusStub(0);
        assert!(verify_cloud_init(&mp).await.is_ok());
    }

    #[tokio::test]
    async fn verify_cloud_init_critical_failure_on_exit_code_1() {
        let mp = MultipassExitStatusStub(1);
        let err = verify_cloud_init(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("critical failure"),
            "expected 'critical failure' in: {msg}"
        );
        assert!(
            msg.contains("/var/log/cloud-init-output.log"),
            "expected log path in: {msg}"
        );
        assert!(
            msg.contains("polis delete && polis start"),
            "expected recovery command in: {msg}"
        );
    }

    #[tokio::test]
    async fn verify_cloud_init_degraded_error_on_exit_code_2() {
        let mp = MultipassExitStatusStub(2);
        let err = verify_cloud_init(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(msg.contains("degraded"), "expected 'degraded' in: {msg}");
        assert!(
            msg.contains("/var/log/cloud-init-output.log"),
            "expected log path in: {msg}"
        );
        assert!(
            msg.contains("polis delete && polis start"),
            "expected recovery command in: {msg}"
        );
    }
}
