//! Status command implementation.
//!
//! Displays workspace state, agent health, security status, and metrics.

#![allow(dead_code)] // Helper functions used only in tests

use std::time::Duration;

use anyhow::Result;
use polis_common::types::{
    AgentHealth, AgentStatus, EventSeverity, SecurityEvents, SecurityStatus, StatusOutput,
    WorkspaceState, WorkspaceStatus,
};
use tokio::time::timeout;

use crate::output::OutputContext;

/// Timeout for status checks.
const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// Run the status command.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub async fn run(ctx: &OutputContext, json: bool) -> Result<()> {
    let output = gather_status().await;

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_human_readable(ctx, &output);
    }

    Ok(())
}

/// Gather all status information.
async fn gather_status() -> StatusOutput {
    let workspace = get_workspace_status().await;
    let is_running = workspace.status == WorkspaceState::Running;

    let (security, agent) = if is_running {
        tokio::join!(get_security_status(), get_agent_status())
    } else {
        (
            SecurityStatus {
                traffic_inspection: false,
                credential_protection: false,
                malware_scanning: false,
            },
            None,
        )
    };

    StatusOutput {
        workspace,
        agent,
        security,
        events: SecurityEvents {
            count: 0,
            severity: EventSeverity::None,
        },
    }
}

/// Check workspace status via multipass.
async fn get_workspace_status() -> WorkspaceStatus {
    // First check if VM is running
    let Ok(Some(vm_state)) = timeout(CHECK_TIMEOUT, check_multipass_status()).await else {
        return workspace_unknown();
    };

    // If VM not running, return that state
    if vm_state != WorkspaceState::Running {
        return WorkspaceStatus {
            status: vm_state,
            uptime_seconds: None,
        };
    }

    // VM is running - check if polis-workspace container is running
    let container_running = check_workspace_container().await;

    WorkspaceStatus {
        status: if container_running {
            WorkspaceState::Running
        } else {
            WorkspaceState::Starting // VM up but container not ready
        },
        uptime_seconds: None,
    }
}

/// Check multipass VM state.
async fn check_multipass_status() -> Option<WorkspaceState> {
    let output = tokio::process::Command::new("multipass")
        .args(["info", "polis", "--format", "json"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let info: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let state = info.get("info")?.get("polis")?.get("state")?.as_str()?;

    Some(match state {
        "Running" => WorkspaceState::Running,
        "Stopped" => WorkspaceState::Stopped,
        "Starting" => WorkspaceState::Starting,
        "Stopping" => WorkspaceState::Stopping,
        _ => WorkspaceState::Error,
    })
}

/// Check if polis-workspace container is running inside VM.
async fn check_workspace_container() -> bool {
    let output = tokio::process::Command::new("multipass")
        .args(["exec", "polis", "--", "docker", "compose", "ps", "--format", "json", "workspace"])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(first_line) = stdout.lines().next() else {
        return false;
    };

    serde_json::from_str::<serde_json::Value>(first_line)
        .ok()
        .and_then(|c| c.get("State")?.as_str().map(|s| s == "running"))
        .unwrap_or(false)
}

/// Check security services inside multipass VM.
async fn get_security_status() -> SecurityStatus {
    let output = tokio::process::Command::new("multipass")
        .args(["exec", "polis", "--", "docker", "compose", "ps", "--format", "json"])
        .output()
        .await;

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => {
            return SecurityStatus {
                traffic_inspection: false,
                credential_protection: false,
                malware_scanning: false,
            }
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut gate = false;
    let mut sentinel = false;
    let mut scanner = false;

    for line in stdout.lines() {
        if let Ok(container) = serde_json::from_str::<serde_json::Value>(line) {
            let service = container.get("Service").and_then(|s| s.as_str());
            let state = container.get("State").and_then(|s| s.as_str());

            if state == Some("running") {
                match service {
                    Some("gate") => gate = true,
                    Some("sentinel") => sentinel = true,
                    Some("scanner") => scanner = true,
                    _ => {}
                }
            }
        }
    }

    SecurityStatus {
        traffic_inspection: gate,
        credential_protection: sentinel,
        malware_scanning: scanner,
    }
}

/// Check agent status inside multipass VM.
async fn get_agent_status() -> Option<AgentStatus> {
    let output = tokio::process::Command::new("multipass")
        .args(["exec", "polis", "--", "docker", "compose", "ps", "--format", "json", "workspace"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?;
    let container: serde_json::Value = serde_json::from_str(first_line).ok()?;

    let state = container.get("State")?.as_str()?;
    let health = container.get("Health").and_then(|h| h.as_str());

    let status = match (state, health) {
        ("running", Some("healthy")) => AgentHealth::Healthy,
        ("running", Some("unhealthy")) => AgentHealth::Unhealthy,
        ("running", _) => AgentHealth::Starting,
        _ => AgentHealth::Stopped,
    };

    let name = crate::state::StateManager::new()
        .ok()
        .and_then(|mgr| mgr.load().ok().flatten())
        .map_or_else(|| "unknown".to_string(), |state| state.agent);

    Some(AgentStatus { name, status })
}

/// Print human-readable status output.
fn print_human_readable(ctx: &OutputContext, status: &StatusOutput) {
    ctx.kv("Workspace:", workspace_state_display(status.workspace.status));

    if let Some(agent) = &status.agent {
        ctx.kv(
            "Agent:",
            &format!("{} ({})", agent.name, agent_health_display(agent.status)),
        );
    }

    if let Some(uptime) = status.workspace.uptime_seconds {
        ctx.kv("Uptime:", &format_uptime(uptime));
    }

    println!();
    ctx.header("Security:");

    if status.security.traffic_inspection {
        ctx.success("Traffic inspection active");
    } else {
        ctx.warn("Traffic inspection inactive");
    }
    if status.security.credential_protection {
        ctx.success("Credential protection enabled");
    } else {
        ctx.warn("Credential protection disabled");
    }
    if status.security.malware_scanning {
        ctx.success("Malware scanning enabled");
    } else {
        ctx.warn("Malware scanning disabled");
    }

    if status.events.count > 0 {
        println!();
        ctx.warn(&format!("{} security events", status.events.count));
        ctx.info("Run: polis logs --security");
    }
}

#[must_use]
pub fn format_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[must_use]
pub fn workspace_state_display(state: WorkspaceState) -> &'static str {
    match state {
        WorkspaceState::Running => "running",
        WorkspaceState::Stopped => "stopped",
        WorkspaceState::Starting => "starting",
        WorkspaceState::Stopping => "stopping",
        WorkspaceState::Error => "error",
    }
}

#[must_use]
pub fn agent_health_display(health: AgentHealth) -> &'static str {
    match health {
        AgentHealth::Healthy => "healthy",
        AgentHealth::Unhealthy => "unhealthy",
        AgentHealth::Starting => "starting",
        AgentHealth::Stopped => "stopped",
    }
}

#[must_use]
pub fn format_agent_line(name: &str, health: AgentHealth) -> String {
    format!("{name} ({})", agent_health_display(health))
}

#[must_use]
pub fn format_events_warning(count: u32) -> String {
    let noun = if count == 1 { "event" } else { "events" };
    format!("{count} security {noun}\nRun: polis logs --security")
}

#[must_use]
pub fn workspace_unknown() -> WorkspaceStatus {
    WorkspaceStatus {
        status: WorkspaceState::Error,
        uptime_seconds: None,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use polis_common::types::{
        AgentHealth, AgentStatus, EventSeverity, SecurityEvents, SecurityStatus, StatusOutput,
        WorkspaceState, WorkspaceStatus,
    };

    #[test]
    fn test_format_uptime_hours_and_minutes() {
        assert_eq!(format_uptime(9240), "2h 34m");
    }

    #[test]
    fn test_format_uptime_minutes_only() {
        assert_eq!(format_uptime(300), "5m");
    }

    #[test]
    fn test_format_uptime_zero() {
        assert_eq!(format_uptime(0), "0m");
    }

    #[test]
    fn test_workspace_state_display_all() {
        assert_eq!(workspace_state_display(WorkspaceState::Running), "running");
        assert_eq!(workspace_state_display(WorkspaceState::Stopped), "stopped");
        assert_eq!(workspace_state_display(WorkspaceState::Starting), "starting");
        assert_eq!(workspace_state_display(WorkspaceState::Stopping), "stopping");
        assert_eq!(workspace_state_display(WorkspaceState::Error), "error");
    }

    #[test]
    fn test_agent_health_display_all() {
        assert_eq!(agent_health_display(AgentHealth::Healthy), "healthy");
        assert_eq!(agent_health_display(AgentHealth::Unhealthy), "unhealthy");
        assert_eq!(agent_health_display(AgentHealth::Starting), "starting");
        assert_eq!(agent_health_display(AgentHealth::Stopped), "stopped");
    }

    #[test]
    fn test_format_agent_line() {
        assert_eq!(format_agent_line("claude-dev", AgentHealth::Healthy), "claude-dev (healthy)");
    }

    #[test]
    fn test_format_events_warning_singular() {
        assert!(format_events_warning(1).contains("1 security event\n"));
    }

    #[test]
    fn test_format_events_warning_plural() {
        assert!(format_events_warning(2).contains("2 security events"));
    }

    #[test]
    fn test_workspace_unknown() {
        let ws = workspace_unknown();
        assert_eq!(ws.status, WorkspaceState::Error);
        assert!(ws.uptime_seconds.is_none());
    }

    fn test_status() -> StatusOutput {
        StatusOutput {
            workspace: WorkspaceStatus { status: WorkspaceState::Running, uptime_seconds: Some(9240) },
            agent: Some(AgentStatus { name: "claude-dev".to_string(), status: AgentHealth::Healthy }),
            security: SecurityStatus { traffic_inspection: true, credential_protection: true, malware_scanning: true },
            events: SecurityEvents { count: 2, severity: EventSeverity::Warning },
        }
    }

    #[test]
    fn test_status_json_roundtrip() {
        let status = test_status();
        let json = serde_json::to_string(&status).expect("serialize");
        let back: StatusOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.workspace.status, WorkspaceState::Running);
        assert_eq!(back.events.count, 2);
    }

    #[test]
    fn test_status_json_omits_none_fields() {
        let status = StatusOutput {
            workspace: WorkspaceStatus { status: WorkspaceState::Stopped, uptime_seconds: None },
            agent: None,
            security: SecurityStatus { traffic_inspection: false, credential_protection: false, malware_scanning: false },
            events: SecurityEvents { count: 0, severity: EventSeverity::None },
        };
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(!json.contains("uptime_seconds"));
        assert!(!json.contains(r#""agent""#));
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod proptests {
    use super::*;
    use polis_common::types::{AgentHealth, WorkspaceState};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_format_uptime_ends_with_m(seconds in 0u64..=604_800) {
            prop_assert!(format_uptime(seconds).ends_with('m'));
        }

        #[test]
        fn prop_format_uptime_hours_has_h(hours in 1u64..168) {
            prop_assert!(format_uptime(hours * 3600).contains('h'));
        }

        #[test]
        fn prop_workspace_state_lowercase(state in prop_oneof![
            Just(WorkspaceState::Running),
            Just(WorkspaceState::Stopped),
            Just(WorkspaceState::Starting),
            Just(WorkspaceState::Stopping),
            Just(WorkspaceState::Error),
        ]) {
            prop_assert!(workspace_state_display(state).chars().all(|c| c.is_lowercase()));
        }

        #[test]
        fn prop_agent_health_lowercase(health in prop_oneof![
            Just(AgentHealth::Healthy),
            Just(AgentHealth::Unhealthy),
            Just(AgentHealth::Starting),
            Just(AgentHealth::Stopped),
        ]) {
            prop_assert!(agent_health_display(health).chars().all(|c| c.is_lowercase()));
        }

        #[test]
        fn prop_events_warning_has_hint(count in 0u32..1000) {
            prop_assert!(format_events_warning(count).contains("polis logs"));
        }
    }
}
