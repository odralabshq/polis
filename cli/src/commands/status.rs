//! Status command implementation.
//!
//! Displays workspace state, agent health, security status, and metrics.

use anyhow::Result;
use polis_common::types::{
    AgentHealth, AgentStatus, EventSeverity, SecurityEvents, SecurityStatus, StatusOutput,
    WorkspaceState, WorkspaceStatus,
};

use crate::multipass::Multipass;
use crate::output::OutputContext;
use crate::workspace::COMPOSE_PATH;

/// Run the status command.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
#[allow(clippy::unused_async)] // async contract with cli.rs
pub async fn run(ctx: &OutputContext, json: bool, mp: &impl Multipass) -> Result<()> {
    let output = gather_status(mp);

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_human_readable(ctx, &output);
    }

    Ok(())
}

/// Gather all status information.
fn gather_status(mp: &impl Multipass) -> StatusOutput {
    let workspace = get_workspace_status(mp);
    let is_running = workspace.status == WorkspaceState::Running;

    let (security, agent) = if is_running {
        (get_security_status(mp), get_agent_status(mp))
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
fn get_workspace_status(mp: &impl Multipass) -> WorkspaceStatus {
    let Some(vm_state) = check_multipass_status(mp) else {
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
    let container_running = check_workspace_container(mp);

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
fn check_multipass_status(mp: &impl Multipass) -> Option<WorkspaceState> {
    let output = mp.vm_info().ok()?;

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
fn check_workspace_container(mp: &impl Multipass) -> bool {
    let output = mp.exec(&[
        "docker",
        "compose",
        "-f",
        COMPOSE_PATH,
        "ps",
        "--format",
        "json",
        "workspace",
    ]);

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
fn get_security_status(mp: &impl Multipass) -> SecurityStatus {
    let output = mp.exec(&[
        "docker",
        "compose",
        "-f",
        COMPOSE_PATH,
        "ps",
        "--format",
        "json",
    ]);

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => {
            return SecurityStatus {
                traffic_inspection: false,
                credential_protection: false,
                malware_scanning: false,
            };
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
fn get_agent_status(mp: &impl Multipass) -> Option<AgentStatus> {
    let output = mp
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

    Some(AgentStatus {
        name: "workspace".to_string(),
        status,
    })
}

/// Print human-readable status output.
fn print_human_readable(ctx: &OutputContext, status: &StatusOutput) {
    ctx.kv(
        "Workspace:",
        workspace_state_display(status.workspace.status),
    );

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

#[allow(dead_code)] // Used by tests and future features
#[must_use]
pub fn format_agent_line(name: &str, health: AgentHealth) -> String {
    format!("{name} ({})", agent_health_display(health))
}

#[allow(dead_code)] // Used by tests and future features
#[must_use]
pub fn format_events_warning(count: u32) -> String {
    let noun = if count == 1 { "event" } else { "events" };
    format!("{count} security {noun}\nRun: polis logs --security")
}

#[allow(dead_code)] // Used by tests
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
        assert_eq!(
            workspace_state_display(WorkspaceState::Starting),
            "starting"
        );
        assert_eq!(
            workspace_state_display(WorkspaceState::Stopping),
            "stopping"
        );
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
        assert_eq!(
            format_agent_line("claude-dev", AgentHealth::Healthy),
            "claude-dev (healthy)"
        );
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
            workspace: WorkspaceStatus {
                status: WorkspaceState::Running,
                uptime_seconds: Some(9240),
            },
            agent: Some(AgentStatus {
                name: "claude-dev".to_string(),
                status: AgentHealth::Healthy,
            }),
            security: SecurityStatus {
                traffic_inspection: true,
                credential_protection: true,
                malware_scanning: true,
            },
            events: SecurityEvents {
                count: 2,
                severity: EventSeverity::Warning,
            },
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
            workspace: WorkspaceStatus {
                status: WorkspaceState::Stopped,
                uptime_seconds: None,
            },
            agent: None,
            security: SecurityStatus {
                traffic_inspection: false,
                credential_protection: false,
                malware_scanning: false,
            },
            events: SecurityEvents {
                count: 0,
                severity: EventSeverity::None,
            },
        };
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(!json.contains("uptime_seconds"));
        assert!(!json.contains(r#""agent""#));
    }
}
