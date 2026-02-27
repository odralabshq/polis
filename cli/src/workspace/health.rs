//! Health checks and readiness waiting.

use std::time::Duration;

use anyhow::Result;

use crate::provisioner::ShellExecutor;
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
pub async fn wait_ready(mp: &impl ShellExecutor, quiet: bool) -> Result<()> {
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
pub async fn check(mp: &impl ShellExecutor) -> HealthStatus {
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
mod property_tests {
    use std::process::{ExitStatus, Output};

    use anyhow::Result;
    use proptest::prelude::*;

    use super::*;
    use crate::provisioner::ShellExecutor;

    #[cfg(unix)]
    fn exit_status_ok() -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }

    #[cfg(windows)]
    fn exit_status_ok() -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }

    #[cfg(unix)]
    fn exit_status_fail() -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(1 << 8)
    }

    #[cfg(windows)]
    fn exit_status_fail() -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(1)
    }

    struct PropTestExecStub(Result<Output>);

    impl ShellExecutor for PropTestExecStub {
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
            anyhow::bail!("not expected")
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            anyhow::bail!("not expected")
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            anyhow::bail!("not expected")
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// **Validates: Requirements 8.2, 11.3**
        ///
        /// Property 8: Health Status Parsing Preservation
        ///
        /// For any valid `docker compose ps` JSON payload with various State/Health
        /// combinations, `health::check()` must produce the correct `HealthStatus`:
        ///   - State="running" + Health="healthy"  → HealthStatus::Healthy
        ///   - State="running" + Health=<other>    → HealthStatus::Unhealthy { reason: "health: <health>" }
        ///   - State=<not running>                 → HealthStatus::Unhealthy { reason: "state: <state>" }
        #[test]
        fn prop_health_status_parsing_preservation(
            state in prop_oneof![
                Just("running".to_string()),
                Just("stopped".to_string()),
                Just("exited".to_string()),
                Just("paused".to_string()),
            ],
            health in prop_oneof![
                Just("healthy".to_string()),
                Just("unhealthy".to_string()),
                Just("starting".to_string()),
                Just("".to_string()),
            ],
        ) {
            let json = format!(r#"{{"State":"{state}","Health":"{health}"}}"#);
            let output = Output {
                status: exit_status_ok(),
                stdout: format!("{json}\n").into_bytes(),
                stderr: Vec::new(),
            };
            let mp = PropTestExecStub(Ok(output));

            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            let result = rt.block_on(check(&mp));

            if state == "running" && health == "healthy" {
                prop_assert_eq!(result, HealthStatus::Healthy,
                    "expected Healthy for state={:?} health={:?}", state, health);
            } else if state == "running" {
                let expected_reason = format!("health: {health}");
                prop_assert!(
                    matches!(&result, HealthStatus::Unhealthy { reason } if reason == &expected_reason),
                    "expected Unhealthy {{ reason: {expected_reason:?} }} for state={state:?} health={health:?}, got {result:?}"
                );
            } else {
                let expected_reason = format!("state: {state}");
                prop_assert!(
                    matches!(&result, HealthStatus::Unhealthy { reason } if reason == &expected_reason),
                    "expected Unhealthy {{ reason: {expected_reason:?} }} for state={state:?} health={health:?}, got {result:?}"
                );
            }
        }

        /// **Validates: Requirements 8.2, 11.3**
        ///
        /// Property 8 (exec error): exec error → HealthStatus::Unknown
        #[test]
        fn prop_exec_error_returns_unknown(
            _state in prop_oneof![
                Just("running".to_string()),
                Just("stopped".to_string()),
            ],
        ) {
            let mp = PropTestExecStub(Err(anyhow::anyhow!("exec failed")));
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            let result = rt.block_on(check(&mp));
            prop_assert_eq!(result, HealthStatus::Unknown,
                "expected Unknown for exec error");
        }

        /// **Validates: Requirements 8.2, 11.3**
        ///
        /// Property 8 (bad JSON): bad JSON → HealthStatus::Unknown
        #[test]
        fn prop_bad_json_returns_unknown(
            garbage in "[^{}\n]{1,30}",
        ) {
            let output = Output {
                status: exit_status_ok(),
                stdout: garbage.as_bytes().to_vec(),
                stderr: Vec::new(),
            };
            let mp = PropTestExecStub(Ok(output));
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            let result = rt.block_on(check(&mp));
            prop_assert_eq!(result, HealthStatus::Unknown,
                "expected Unknown for bad JSON: {:?}", garbage);
        }

        /// **Validates: Requirements 8.2, 11.3**
        ///
        /// Property 8 (non-zero exit): non-zero exit status → HealthStatus::Unknown
        #[test]
        fn prop_nonzero_exit_returns_unknown(
            state in prop_oneof![
                Just("running".to_string()),
                Just("exited".to_string()),
            ],
            health in prop_oneof![
                Just("healthy".to_string()),
                Just("unhealthy".to_string()),
            ],
        ) {
            let json = format!(r#"{{"State":"{state}","Health":"{health}"}}"#);
            let output = Output {
                status: exit_status_fail(),
                stdout: format!("{json}\n").into_bytes(),
                stderr: Vec::new(),
            };
            let mp = PropTestExecStub(Ok(output));
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            let result = rt.block_on(check(&mp));
            prop_assert_eq!(result, HealthStatus::Unknown,
                "expected Unknown for non-zero exit");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::{ExitStatus, Output};

    use anyhow::Result;

    use super::*;
    use crate::provisioner::ShellExecutor;

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

    fn mock_output(stdout: &[u8]) -> Output {
        Output {
            status: exit_status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    /// Mock multipass with configurable `exec()` output for health checks.
    struct MultipassExecStub(Result<Output>);
    impl ShellExecutor for MultipassExecStub {
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
