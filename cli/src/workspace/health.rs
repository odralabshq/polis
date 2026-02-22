//! Health checks and readiness waiting.

use std::time::Duration;

use anyhow::Result;

use crate::multipass::Multipass;
use crate::workspace::COMPOSE_PATH;

/// Health status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Unhealthy { reason: String },
    Unknown,
}

/// REL-004: Get health check timeout from environment or use default.
fn get_health_timeout() -> (u32, Duration) {
    let timeout_secs: u64 = std::env::var("POLIS_HEALTH_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let delay = Duration::from_secs(2);
    #[allow(clippy::cast_possible_truncation)]
    let max_attempts = (timeout_secs / 2) as u32;
    (max_attempts.max(1), delay)
}

/// Wait for workspace to become healthy.
///
/// Polls every 2 seconds. Timeout configurable via `POLIS_HEALTH_TIMEOUT` env var
/// (default: 60 seconds).
///
/// # Errors
///
/// Returns an error if the workspace does not become healthy within timeout.
pub async fn wait_ready(mp: &impl Multipass, quiet: bool) -> Result<()> {
    use owo_colors::{OwoColorize, Stream::Stdout, Style};
    // Logo gradient stop 4 (46,53,147) — L3
    let tag_style = Style::new().truecolor(46, 53, 147);

    let fmt = |msg: &str| {
        format!(
            "{}  {}",
            "[inception]".if_supports_color(Stdout, |t| t.style(tag_style)),
            msg,
        )
    };

    let pb =
        (!quiet).then(|| crate::output::progress::spinner(&fmt("agent isolation complete...")));

    let (max_attempts, delay) = get_health_timeout();

    for attempt in 1..=max_attempts {
        match check(mp).await {
            HealthStatus::Healthy => {
                if let Some(pb) = pb {
                    crate::output::progress::finish_ok(&pb, &fmt("agent containment active."));
                }
                return Ok(());
            }
            HealthStatus::Unhealthy { reason } if attempt == max_attempts => {
                if let Some(pb) = &pb {
                    pb.finish_and_clear();
                }
                anyhow::bail!(
                    "Workspace did not start properly.\n\nReason: {reason}\nDiagnose: polis doctor\nView logs: polis logs"
                );
            }
            _ => tokio::time::sleep(delay).await,
        }
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
    anyhow::bail!(
        "Workspace did not start properly.\n\nDiagnose: polis doctor\nView logs: polis logs"
    )
}

/// Check current health status.
pub async fn check(mp: &impl Multipass) -> HealthStatus {
    let Ok(output) = mp
        .exec(&[
            "docker",
            "compose",
            "-f",
            COMPOSE_PATH,
            "ps",
            "--format",
            "json",
            "workspace",
        ])
        .await
    else {
        return HealthStatus::Unknown;
    };

    if !output.status.success() {
        return HealthStatus::Unknown;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(line) = stdout.lines().next() else {
        return HealthStatus::Unknown;
    };

    let container: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return HealthStatus::Unknown,
    };

    let state = container
        .get("State")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let health = container
        .get("Health")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    if state == "running" && health == "healthy" {
        HealthStatus::Healthy
    } else if state == "running" {
        HealthStatus::Unhealthy {
            reason: format!("health: {health}"),
        }
    } else {
        HealthStatus::Unhealthy {
            reason: format!("state: {state}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    use anyhow::Result;

    use super::*;
    use crate::multipass::Multipass;

    fn mock_output(stdout: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    /// Mock multipass with configurable `exec()` output for health checks.
    struct MultipassExecStub(Result<Output>);
    impl Multipass for MultipassExecStub {
        async fn vm_info(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn start(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn stop(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn delete(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn purge(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            unimplemented!()
        }
        async fn version(&self) -> Result<Output> {
            unimplemented!()
        }
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            match &self.0 {
                Ok(o) => Ok(Output {
                    status: o.status,
                    stdout: o.stdout.clone(),
                    stderr: o.stderr.clone(),
                }),
                Err(e) => Err(anyhow::anyhow!("{e}")),
            }
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            unimplemented!()
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn check_healthy() {
        let mp = MultipassExecStub(Ok(mock_output(
            br#"{"State":"running","Health":"healthy"}"#,
        )));
        assert_eq!(check(&mp).await, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn check_running_not_healthy() {
        let mp = MultipassExecStub(Ok(mock_output(
            br#"{"State":"running","Health":"starting"}"#,
        )));
        assert!(matches!(check(&mp).await, HealthStatus::Unhealthy { .. }));
    }

    #[tokio::test]
    async fn check_not_running() {
        let mp = MultipassExecStub(Ok(mock_output(br#"{"State":"exited","Health":""}"#)));
        assert!(matches!(check(&mp).await, HealthStatus::Unhealthy { .. }));
    }

    #[tokio::test]
    async fn check_exec_fails_returns_unknown() {
        let mp = MultipassExecStub(Err(anyhow::anyhow!("exec failed")));
        assert_eq!(check(&mp).await, HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn check_bad_json_returns_unknown() {
        let mp = MultipassExecStub(Ok(mock_output(b"not json")));
        assert_eq!(check(&mp).await, HealthStatus::Unknown);
    }
}
