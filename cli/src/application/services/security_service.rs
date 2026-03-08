//! Application service — security management use-cases.
//!
//! Wraps `polis-approve` inside the toolbox container to manage blocked
//! requests, domain rules, and security levels via Valkey.

use anyhow::{Context, Result};
use std::{future::Future, time::Duration};

use crate::application::ports::{ConfigStore, ControlPlanePort, ProgressReporter, ShellExecutor};

/// Container name for the toolbox service (runs polis-approve CLI).
const TOOLBOX_CONTAINER: &str = "polis-toolbox";
const SECURITY_EVENT_LIMIT: usize = 20;
const CONTROL_PLANE_WAIT_MESSAGE: &str =
    "Control-plane request is taking longer than expected; waiting...";

#[cfg(test)]
const CONTROL_PLANE_WAIT_THRESHOLD: Duration = Duration::from_millis(25);
#[cfg(not(test))]
const CONTROL_PLANE_WAIT_THRESHOLD: Duration = Duration::from_secs(1);

async fn api_first_with_progress<T, ApiFuture, FallbackFuture>(
    reporter: &impl ProgressReporter,
    api_future: ApiFuture,
    fallback_future: FallbackFuture,
) -> Result<T>
where
    ApiFuture: Future<Output = Result<T>>,
    FallbackFuture: Future<Output = Result<T>>,
{
    tokio::pin!(api_future);
    let api_result = tokio::select! {
        result = &mut api_future => result,
        () = tokio::time::sleep(CONTROL_PLANE_WAIT_THRESHOLD) => {
            reporter.step(CONTROL_PLANE_WAIT_MESSAGE);
            api_future.await
        }
    };

    match api_result {
        Ok(value) => Ok(value),
        Err(_) => fallback_future.await,
    }
}

/// Run polis-approve inside the toolbox container and capture output.
///
/// Reads the mcp-admin password from the mounted Docker secret and injects it
/// as `polis_VALKEY_PASS` so `polis-approve` can authenticate to Valkey.
async fn toolbox_approve(mp: &impl ShellExecutor, args: &[&str]) -> Result<String> {
    let pass_output = mp
        .exec(&[
            "docker",
            "exec",
            TOOLBOX_CONTAINER,
            "cat",
            "/run/secrets/valkey_mcp_admin_password",
        ])
        .await
        .context("failed to read mcp-admin password from toolbox container")?;

    if !pass_output.status.success() {
        let stderr = String::from_utf8_lossy(&pass_output.stderr);
        if stderr.contains("No such container") {
            anyhow::bail!(
                "Toolbox container is not running. Start the workspace first: polis start"
            );
        }
        anyhow::bail!(
            "Toolbox container missing Valkey credentials. Try restarting: polis stop && polis start"
        );
    }

    let pass = String::from_utf8_lossy(&pass_output.stdout)
        .trim()
        .to_string();
    let pass_env = format!("polis_VALKEY_PASS={pass}");

    let mut cmd: Vec<&str> = vec![
        "docker",
        "exec",
        "-e",
        &pass_env,
        TOOLBOX_CONTAINER,
        "polis-approve",
    ];
    cmd.extend_from_slice(args);

    let output = mp
        .exec(&cmd)
        .await
        .context("failed to exec polis-approve in toolbox container")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let msg = if stderr.is_empty() { &stdout } else { &stderr };
        anyhow::bail!("polis-approve failed: {}", msg.trim());
    }

    Ok(stdout)
}

/// Result of a security status query.
pub struct SecurityStatus {
    /// Current security level from local config.
    pub level: String,
    /// Pending request lines (empty if none).
    pub pending_lines: Vec<String>,
    /// Error message if pending query failed.
    pub pending_error: Option<String>,
}

/// Query security status: level + pending count.
///
/// # Errors
///
/// Returns an error if the local config cannot be loaded.
pub async fn get_status(
    store: &impl ConfigStore,
    reporter: &impl ProgressReporter,
    cp: &impl ControlPlanePort,
    mp: &impl ShellExecutor,
) -> Result<SecurityStatus> {
    let config = crate::application::services::config_service::load_config(store)?;
    let fallback_level = config.security.level;
    let fallback_level_for_toolbox = fallback_level.clone();
    let (level, pending_lines, pending_error) = api_first_with_progress(
        reporter,
        async {
            cp.status().await.map(|status| {
                (
                    status.security_level,
                    format_pending_count(status.pending_count),
                    None,
                )
            })
        },
        async move {
            match toolbox_approve(mp, &["list-pending"]).await {
                Ok(output) => Ok((
                    fallback_level_for_toolbox,
                    parse_pending_output(&output),
                    None,
                )),
                Err(error) => Ok((fallback_level, vec![], Some(format!("{error}")))),
            }
        },
    )
    .await?;

    Ok(SecurityStatus {
        level,
        pending_lines,
        pending_error,
    })
}

/// List pending blocked requests. Returns lines of output.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or polis-approve fails.
pub async fn list_pending(
    reporter: &impl ProgressReporter,
    cp: &impl ControlPlanePort,
    mp: &impl ShellExecutor,
) -> Result<Vec<String>> {
    api_first_with_progress(
        reporter,
        async {
            cp.blocked_requests()
                .await
                .map(|response| response.items.iter().map(format_blocked_item).collect())
        },
        async {
            let output = toolbox_approve(mp, &["list-pending"]).await?;
            Ok(parse_pending_output(&output))
        },
    )
    .await
}

/// Approve a blocked request. Returns confirmation message.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or the request ID is invalid.
pub async fn approve(
    reporter: &impl ProgressReporter,
    cp: &impl ControlPlanePort,
    mp: &impl ShellExecutor,
    request_id: &str,
) -> Result<String> {
    api_first_with_progress(
        reporter,
        async {
            cp.approve_request(request_id)
                .await
                .map(|response| response.message)
        },
        async {
            let output = toolbox_approve(mp, &["approve", request_id]).await?;
            Ok(output.trim().to_string())
        },
    )
    .await
}

/// Deny a blocked request. Returns confirmation message.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or the request ID is invalid.
pub async fn deny(
    reporter: &impl ProgressReporter,
    cp: &impl ControlPlanePort,
    mp: &impl ShellExecutor,
    request_id: &str,
) -> Result<String> {
    api_first_with_progress(
        reporter,
        async {
            cp.deny_request(request_id)
                .await
                .map(|response| response.message)
        },
        async {
            let output = toolbox_approve(mp, &["deny", request_id]).await?;
            Ok(output.trim().to_string())
        },
    )
    .await
}

/// Query recent security events from the Valkey event log.
///
/// # Errors
///
/// Returns an error if the state container is unreachable.
pub async fn get_log(
    reporter: &impl ProgressReporter,
    cp: &impl ControlPlanePort,
    mp: &impl ShellExecutor,
) -> Result<Vec<String>> {
    api_first_with_progress(
        reporter,
        async {
            cp.security_events(SECURITY_EVENT_LIMIT)
                .await
                .map(|response| response.events.iter().map(format_event_item).collect())
        },
        async {
            let output = mp
                .exec(&[
                    "docker",
                    "exec",
                    "polis-state",
                    "sh",
                    "-c",
                    "REDISCLI_AUTH=$(cat /run/secrets/valkey_mcp_admin_password) \
                     valkey-cli --tls \
                     --cert /etc/valkey/tls/client.crt \
                     --key /etc/valkey/tls/client.key \
                     --cacert /etc/valkey/tls/ca.crt \
                     --user mcp-admin --no-auth-warning \
                     ZREVRANGEBYSCORE polis:log:events +inf -inf LIMIT 0 20",
                ])
                .await
                .context("failed to query security log")?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let trimmed = stdout.trim();

            if trimmed.is_empty() || trimmed == "(empty array)" || trimmed == "(empty list or set)"
            {
                Ok(vec![])
            } else {
                Ok(trimmed.lines().map(String::from).collect())
            }
        },
    )
    .await
}

/// Set an auto-approve rule. Returns confirmation message.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or the pattern/action is invalid.
pub async fn auto_allow(
    reporter: &impl ProgressReporter,
    cp: &impl ControlPlanePort,
    mp: &impl ShellExecutor,
    pattern: &str,
    action: &str,
) -> Result<String> {
    api_first_with_progress(
        reporter,
        async {
            cp.add_rule(pattern, action)
                .await
                .map(|response| response.message)
        },
        async {
            let output = toolbox_approve(mp, &["auto-approve", pattern, action]).await?;
            Ok(output.trim().to_string())
        },
    )
    .await
}

/// Set the security level (validates, updates Valkey + local config).
///
/// # Errors
///
/// Returns an error if the level is invalid, the toolbox is unreachable, or config save fails.
pub async fn set_level(
    store: &impl ConfigStore,
    reporter: &impl ProgressReporter,
    cp: &impl ControlPlanePort,
    mp: &impl ShellExecutor,
    level: &str,
) -> Result<String> {
    match level {
        "relaxed" | "balanced" | "strict" => {}
        _ => {
            anyhow::bail!("Invalid security level '{level}': expected relaxed, balanced, or strict")
        }
    }

    let output = api_first_with_progress(
        reporter,
        async {
            cp.set_security_level(level)
                .await
                .map(|response| format!("Security level set to {}", response.level))
        },
        async { toolbox_approve(mp, &["set-security-level", level]).await },
    )
    .await?;

    let mut config = crate::application::services::config_service::load_config(store)?;
    config.security.level = level.to_string();
    crate::application::services::config_service::save_config(store, &config)?;

    Ok(output.trim().to_string())
}

fn parse_pending_output(output: &str) -> Vec<String> {
    let trimmed = output.trim();
    if trimmed == "no pending requests" || trimmed.is_empty() {
        vec![]
    } else {
        trimmed.lines().map(String::from).collect()
    }
}

fn format_pending_count(count: usize) -> Vec<String> {
    if count == 0 {
        vec![]
    } else {
        vec![format!("{count} pending blocked request(s)")]
    }
}

fn format_blocked_item(item: &cp_api_types::BlockedItem) -> String {
    format!(
        "{} | {} | {} | {}",
        item.request_id, item.reason, item.destination, item.blocked_at
    )
}

fn format_event_item(item: &cp_api_types::EventItem) -> String {
    let request_id = item.request_id.as_deref().unwrap_or("-");
    format!(
        "{} | {} | {} | {}",
        item.timestamp, item.event_type, request_id, item.details
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use cp_api_types::{
        ActionResponse, AgentResponse, BlockedItem, BlockedListResponse, ContainersResponse,
        EventsResponse, LevelResponse, RulesResponse, StatusResponse, WorkspaceResponse,
    };
    use std::{
        collections::VecDeque,
        process::{Output, Stdio},
        sync::Mutex,
    };

    use crate::application::{ports::ProgressReporter, services::vm::test_support::ok_output};

    #[derive(Default)]
    struct RecordingReporter {
        steps: Mutex<Vec<String>>,
    }

    impl RecordingReporter {
        fn recorded_steps(&self) -> Vec<String> {
            self.steps.lock().expect("steps lock").clone()
        }
    }

    impl ProgressReporter for RecordingReporter {
        fn step(&self, message: &str) {
            self.steps
                .lock()
                .expect("steps lock")
                .push(message.to_string());
        }

        fn success(&self, _message: &str) {}

        fn warn(&self, _message: &str) {}
    }

    struct MockShell {
        outputs: Mutex<VecDeque<Output>>,
        calls: Mutex<usize>,
    }

    impl MockShell {
        fn new(outputs: Vec<Output>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into()),
                calls: Mutex::new(0),
            }
        }

        fn call_count(&self) -> usize {
            *self.calls.lock().expect("calls lock")
        }
    }

    impl ShellExecutor for MockShell {
        async fn exec(&self, _args: &[&str]) -> Result<Output> {
            *self.calls.lock().expect("calls lock") += 1;
            self.outputs
                .lock()
                .expect("outputs lock")
                .pop_front()
                .context("missing mocked shell output")
        }

        async fn exec_with_stdin(&self, _args: &[&str], _input: &[u8]) -> Result<Output> {
            anyhow::bail!("exec_with_stdin not expected in test")
        }

        fn exec_spawn(&self, _args: &[&str]) -> Result<tokio::process::Child> {
            let mut command = tokio::process::Command::new("cmd");
            command
                .arg("/C")
                .arg("exit 0")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            command.spawn().context("failed to spawn placeholder child")
        }

        async fn exec_status(&self, _args: &[&str]) -> Result<std::process::ExitStatus> {
            anyhow::bail!("exec_status not expected in test")
        }
    }

    struct MockControlPlane {
        blocked_delay: Duration,
        blocked: Result<BlockedListResponse>,
    }

    impl MockControlPlane {
        fn blocked_success(delay: Duration) -> Self {
            Self {
                blocked_delay: delay,
                blocked: Ok(BlockedListResponse {
                    items: vec![BlockedItem {
                        request_id: "req-12345678".to_string(),
                        reason: "credential_detected".to_string(),
                        destination: "https://example.test".to_string(),
                        blocked_at: Utc
                            .with_ymd_and_hms(2026, 3, 8, 2, 0, 0)
                            .single()
                            .expect("valid timestamp"),
                        status: "pending".to_string(),
                    }],
                }),
            }
        }

        fn blocked_error(delay: Duration) -> Self {
            Self {
                blocked_delay: delay,
                blocked: Err(anyhow::anyhow!("control-plane unavailable")),
            }
        }
    }

    impl ControlPlanePort for MockControlPlane {
        async fn status(&self) -> Result<StatusResponse> {
            anyhow::bail!("status not expected")
        }

        async fn blocked_requests(&self) -> Result<BlockedListResponse> {
            tokio::time::sleep(self.blocked_delay).await;
            match &self.blocked {
                Ok(response) => Ok(response.clone()),
                Err(error) => Err(anyhow::anyhow!(error.to_string())),
            }
        }

        async fn approve_request(&self, _request_id: &str) -> Result<ActionResponse> {
            anyhow::bail!("approve_request not expected")
        }

        async fn deny_request(&self, _request_id: &str) -> Result<ActionResponse> {
            anyhow::bail!("deny_request not expected")
        }

        async fn security_events(&self, _limit: usize) -> Result<EventsResponse> {
            anyhow::bail!("security_events not expected")
        }

        async fn rules(&self) -> Result<RulesResponse> {
            anyhow::bail!("rules not expected")
        }

        async fn add_rule(&self, _pattern: &str, _action: &str) -> Result<ActionResponse> {
            anyhow::bail!("add_rule not expected")
        }

        async fn set_security_level(&self, _level: &str) -> Result<LevelResponse> {
            anyhow::bail!("set_security_level not expected")
        }

        async fn workspace(&self) -> Result<WorkspaceResponse> {
            anyhow::bail!("workspace not expected")
        }

        async fn agent(&self) -> Result<AgentResponse> {
            anyhow::bail!("agent not expected")
        }

        async fn containers(&self) -> Result<ContainersResponse> {
            anyhow::bail!("containers not expected")
        }
    }

    #[tokio::test]
    async fn list_pending_emits_wait_message_before_fallback_on_slow_failure() {
        let reporter = RecordingReporter::default();
        let cp = MockControlPlane::blocked_error(Duration::from_millis(50));
        let shell = MockShell::new(vec![
            ok_output(b"secret\n"),
            ok_output(b"req-123 pending\n"),
        ]);

        let lines = list_pending(&reporter, &cp, &shell)
            .await
            .expect("list pending");

        assert_eq!(lines, vec!["req-123 pending".to_string()]);
        assert_eq!(
            reporter.recorded_steps(),
            vec![CONTROL_PLANE_WAIT_MESSAGE.to_string()]
        );
        assert_eq!(shell.call_count(), 2);
    }

    #[tokio::test]
    async fn list_pending_does_not_emit_wait_message_for_fast_control_plane_success() {
        let reporter = RecordingReporter::default();
        let cp = MockControlPlane::blocked_success(Duration::from_millis(0));
        let shell = MockShell::new(Vec::new());

        let lines = list_pending(&reporter, &cp, &shell)
            .await
            .expect("list pending");

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("req-12345678"));
        assert!(reporter.recorded_steps().is_empty());
        assert_eq!(shell.call_count(), 0);
    }
}
