//! VM service operations: image pulling and service startup.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use anyhow::{Context, Result};

use crate::application::ports::{ProgressReporter, ShellExecutor};

/// Pull all Docker images inside the VM via `docker compose pull`.
///
/// Runs `timeout 600 docker compose -f /opt/polis/docker-compose.yml pull`
/// inside the VM, enforcing a 10-minute limit.
///
/// # Errors
///
/// - If the command exits with code 124 (timeout), returns an error suggesting
///   the user check network connectivity.
/// - If the command fails for any other reason, returns an error with the
///   captured stderr for diagnosis.
pub async fn pull_images(mp: &impl ShellExecutor, _reporter: &impl ProgressReporter) -> Result<()> {
    let output = mp
        .exec(&[
            "timeout",
            "600",
            "docker",
            "compose",
            "-f",
            "/opt/polis/docker-compose.yml",
            "pull",
        ])
        .await
        .context("pulling Docker images from GHCR")?;

    if output.status.success() {
        return Ok(());
    }

    // Exit code 124 means `timeout` killed the process.
    if output.status.code() == Some(124) {
        anyhow::bail!(
            "Docker image pull timed out after 10 minutes.\n\
             Check your network connectivity and retry with: polis start"
        );
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "failed to pull Docker images.\n\
         {stderr}\n\
         Check your network connectivity and retry with: polis start"
    );
}

/// Start polis services via systemctl inside the VM.
pub(super) async fn start_services(mp: &impl ShellExecutor) {
    let _ = mp.exec(&["sudo", "systemctl", "start", "polis"]).await;
}

/// Start services with progress messages.
pub(super) async fn start_services_with_progress(
    mp: &impl ShellExecutor,
    reporter: &impl ProgressReporter,
    quiet: bool,
) {
    if !quiet {
        reporter.step("securing workspace...");
    }
    start_services(mp).await;
    if !quiet {
        reporter.success("workspace secured");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::process::Output;

    use anyhow::Result;

    use super::*;
    use crate::application::ports::ShellExecutor;
    use crate::application::services::vm::test_support::{exit_status, impl_shell_executor_stubs};

    struct PullImagesStub {
        exit_code: i32,
        stderr: Vec<u8>,
    }

    impl PullImagesStub {
        fn success() -> Self {
            Self {
                exit_code: 0,
                stderr: vec![],
            }
        }
        fn failure(stderr: &[u8]) -> Self {
            Self {
                exit_code: 1,
                stderr: stderr.to_vec(),
            }
        }
        fn timeout() -> Self {
            Self {
                exit_code: 124,
                stderr: b"Timeout".to_vec(),
            }
        }
    }

    struct ReporterStub;
    impl ProgressReporter for ReporterStub {
        fn step(&self, _: &str) {}
        fn success(&self, _: &str) {}
        fn warn(&self, _: &str) {}
    }

    impl ShellExecutor for PullImagesStub {
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            Ok(Output {
                status: exit_status(self.exit_code),
                stdout: vec![],
                stderr: self.stderr.clone(),
            })
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    #[tokio::test]
    async fn pull_images_succeeds_on_exit_code_0() {
        let mp = PullImagesStub::success();
        let result = pull_images(&mp, &ReporterStub).await;
        assert!(result.is_ok(), "exit code 0 should succeed: {result:?}");
    }

    #[tokio::test]
    async fn pull_images_fails_on_nonzero_exit_code() {
        let mp = PullImagesStub::failure(b"connection refused");
        let result = pull_images(&mp, &ReporterStub).await;
        assert!(result.is_err(), "non-zero exit code should fail");
    }

    #[tokio::test]
    async fn pull_images_includes_stderr_in_error() {
        let mp = PullImagesStub::failure(b"Error response from daemon: manifest unknown");
        let err = pull_images(&mp, &ReporterStub)
            .await
            .expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("Error response from daemon"),
            "error must include stderr: {msg}"
        );
    }

    #[tokio::test]
    async fn pull_images_timeout_returns_specific_error() {
        let mp = PullImagesStub::timeout();
        let err = pull_images(&mp, &ReporterStub)
            .await
            .expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("timed out") || msg.contains("10 minutes"),
            "timeout error must mention timeout: {msg}"
        );
    }

    #[tokio::test]
    async fn pull_images_timeout_suggests_network_check() {
        let mp = PullImagesStub::timeout();
        let err = pull_images(&mp, &ReporterStub)
            .await
            .expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("network") || msg.contains("connectivity"),
            "timeout error must suggest checking network: {msg}"
        );
    }
}
