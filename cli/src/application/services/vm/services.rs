//! VM service operations: image pulling and service startup.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use anyhow::{Context, Result};

use crate::application::ports::ShellExecutor;

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
pub async fn pull_images(mp: &impl ShellExecutor) -> Result<()> {
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

/// Start services with inception progress spinner.
pub(super) async fn start_services_with_progress(mp: &impl ShellExecutor, quiet: bool) {
    let pb = (!quiet).then(|| {
        crate::output::progress::spinner(&inception_line("L2", "agent isolation starting..."))
    });
    start_services(mp).await;
    if let Some(pb) = pb {
        crate::output::progress::finish_ok(
            &pb,
            &inception_line("L2", "agent isolation starting..."),
        );
    }
}

fn inception_line(level: &str, msg: &str) -> String {
    use owo_colors::{OwoColorize, Stream::Stdout, Style};
    let tag_style = match level {
        "L0" => Style::new().truecolor(107, 33, 168),
        "L1" => Style::new().truecolor(93, 37, 163),
        "L2" => Style::new().truecolor(64, 47, 153),
        _ => Style::new().truecolor(46, 53, 147),
    };
    format!(
        "{}  {}",
        "[inception]".if_supports_color(Stdout, |t| t.style(tag_style)),
        msg
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::process::{ExitStatus, Output};

    use anyhow::Result;

    use super::*;
    use crate::application::ports::ShellExecutor;

    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        #[allow(clippy::cast_sign_loss)]
        ExitStatus::from_raw(code as u32)
    }

    struct PullImagesStub {
        exit_code: i32,
        stderr: Vec<u8>,
    }

    impl PullImagesStub {
        fn success() -> Self {
            Self { exit_code: 0, stderr: vec![] }
        }
        fn failure(stderr: &[u8]) -> Self {
            Self { exit_code: 1, stderr: stderr.to_vec() }
        }
        fn timeout() -> Self {
            Self { exit_code: 124, stderr: b"Timeout".to_vec() }
        }
    }

    impl ShellExecutor for PullImagesStub {
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            Ok(Output {
                status: exit_status(self.exit_code),
                stdout: vec![],
                stderr: self.stderr.clone(),
            })
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            anyhow::bail!("not expected")
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            anyhow::bail!("not expected")
        }
    }

    #[tokio::test]
    async fn pull_images_succeeds_on_exit_code_0() {
        let mp = PullImagesStub::success();
        let result = pull_images(&mp).await;
        assert!(result.is_ok(), "exit code 0 should succeed: {result:?}");
    }

    #[tokio::test]
    async fn pull_images_fails_on_nonzero_exit_code() {
        let mp = PullImagesStub::failure(b"connection refused");
        let result = pull_images(&mp).await;
        assert!(result.is_err(), "non-zero exit code should fail");
    }

    #[tokio::test]
    async fn pull_images_includes_stderr_in_error() {
        let mp = PullImagesStub::failure(b"Error response from daemon: manifest unknown");
        let err = pull_images(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("Error response from daemon"),
            "error must include stderr: {msg}"
        );
    }

    #[tokio::test]
    async fn pull_images_timeout_returns_specific_error() {
        let mp = PullImagesStub::timeout();
        let err = pull_images(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("timed out") || msg.contains("10 minutes"),
            "timeout error must mention timeout: {msg}"
        );
    }

    #[tokio::test]
    async fn pull_images_timeout_suggests_network_check() {
        let mp = PullImagesStub::timeout();
        let err = pull_images(&mp).await.expect_err("expected Err");
        let msg = err.to_string();
        assert!(
            msg.contains("network") || msg.contains("connectivity"),
            "timeout error must suggest checking network: {msg}"
        );
    }
}
