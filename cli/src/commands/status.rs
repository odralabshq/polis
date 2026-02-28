//! Status command implementation.
//!
//! Thin handler: delegates status gathering to `application::services::workspace_status`,
//! then renders the result via `app.renderer()`.

use anyhow::Result;
use polis_common::types::{AgentHealth, WorkspaceState};

use crate::app::AppContext;
use crate::application::ports::{InstanceInspector, ShellExecutor};
use crate::application::services::workspace_status::gather_status;

/// Run the status command.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub async fn run(app: &AppContext, mp: &(impl InstanceInspector + ShellExecutor)) -> Result<()> {
    let output = gather_status(mp).await;
    app.renderer().render_status(&output)
}

// ── Display helpers (used by tests and output layer) ─────────────────────────

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

#[cfg(test)]
#[must_use]
pub fn format_agent_line(name: &str, health: AgentHealth) -> String {
    format!("{name} ({})", agent_health_display(health))
}

#[cfg(test)]
#[must_use]
pub fn format_events_warning(count: u32) -> String {
    let noun = if count == 1 { "event" } else { "events" };
    format!("{count} security {noun}\nRun: polis logs --security")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::application::services::workspace_status::workspace_unknown;
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
