//! Application service — workspace status gathering use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

use std::collections::HashMap;

use polis_common::types::{
    AgentHealth, AgentStatus, EventSeverity, SecurityEvents, SecurityStatus, StatusOutput,
    WorkspaceState, WorkspaceStatus,
};

use crate::application::ports::{InstanceInspector, ShellExecutor};
use crate::domain::workspace::COMPOSE_PATH;

/// Gather all workspace status information.
///
/// # Errors
///
/// This function is infallible — all errors are swallowed and reflected as
/// `WorkspaceState::Error` or absent optional fields.
struct ContainerInfo {
    state: String,
    health: Option<String>,
}

pub async fn gather_status(mp: &(impl InstanceInspector + ShellExecutor)) -> StatusOutput {
    let Some(vm_state) = check_multipass_status(mp).await else {
        return StatusOutput {
            workspace: workspace_unknown(),
            agent: None,
            security: empty_security(),
            events: empty_events(),
        };
    };

    if vm_state != WorkspaceState::Running {
        return StatusOutput {
            workspace: WorkspaceStatus {
                status: vm_state,
                uptime_seconds: None,
            },
            agent: None,
            security: empty_security(),
            events: empty_events(),
        };
    }

    // VM is running, gather detailed status in a single consolidated call
    let (uptime_seconds, containers) = gather_remote_info(mp).await;

    let workspace_info = containers.get("workspace");
    let is_workspace_running = workspace_info
        .map(|i| i.state == "running")
        .unwrap_or(false);

    let agent = workspace_info.map(|i| AgentStatus {
        name: "workspace".to_string(),
        status: match (i.state.as_str(), i.health.as_deref()) {
            ("running", Some("healthy")) => AgentHealth::Healthy,
            ("running", Some("unhealthy")) => AgentHealth::Unhealthy,
            ("running", _) => AgentHealth::Starting,
            _ => AgentHealth::Stopped,
        },
    });

    StatusOutput {
        workspace: WorkspaceStatus {
            status: if is_workspace_running {
                WorkspaceState::Running
            } else {
                WorkspaceState::Starting
            },
            uptime_seconds,
        },
        agent,
        security: SecurityStatus {
            traffic_inspection: containers
                .get("gate")
                .map_or(false, |i| i.state == "running"),
            credential_protection: containers
                .get("sentinel")
                .map_or(false, |i| i.state == "running"),
            malware_scanning: containers
                .get("scanner")
                .map_or(false, |i| i.state == "running"),
        },
        events: empty_events(),
    }
}

fn empty_security() -> SecurityStatus {
    SecurityStatus {
        traffic_inspection: false,
        credential_protection: false,
        malware_scanning: false,
    }
}

fn empty_events() -> SecurityEvents {
    SecurityEvents {
        count: 0,
        severity: EventSeverity::None,
    }
}

/// Check multipass VM state.
async fn check_multipass_status(mp: &impl InstanceInspector) -> Option<WorkspaceState> {
    let output = mp.info().await.ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not exist") || stderr.contains("was not found") {
            return Some(WorkspaceState::NotFound);
        }
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

/// Shell snippet to gather system metrics and container states in one pass.
///
/// We output uptime, a separator, and then the JSON container states.
/// This minimizes Multipass exec overhead (especially high on Windows).
const GATHER_STATUS_SCRIPT: &str =
    "cat /proc/uptime && echo '---' && docker compose -f {} ps --format json";

/// Gather uptime and container info in a single remote call.
async fn gather_remote_info(
    mp: &impl ShellExecutor,
) -> (Option<u64>, HashMap<String, ContainerInfo>) {
    let mut containers = HashMap::new();
    let mut uptime = None;

    // We use a consolidated shell command to minimize Multipass exec overhead (high on Windows).
    // This outputs uptime, a separator, and then the JSON container states.
    let cmd = GATHER_STATUS_SCRIPT.replace("{}", COMPOSE_PATH);

    let output = mp.exec(&["bash", "-c", &cmd]).await;

    let Ok(o) = output else {
        return (uptime, containers);
    };
    if !o.status.success() {
        return (uptime, containers);
    }

    let stdout = String::from_utf8_lossy(&o.stdout);
    let mut in_json_section = false;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line == "---" {
            in_json_section = true;
            continue;
        }

        if !in_json_section {
            // Parsing uptime (first line of output before ---)
            if let Some(u_str) = line.split_whitespace().next() {
                if let Ok(u) = u_str.parse::<f64>() {
                    uptime = Some(u as u64);
                }
            }
        } else {
            // Parsing docker compose ps JSON output
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                if let (Some(name), Some(state)) = (
                    json.get("Service").and_then(serde_json::Value::as_str),
                    json.get("State").and_then(serde_json::Value::as_str),
                ) {
                    containers.insert(
                        name.to_string(),
                        ContainerInfo {
                            state: state.to_string(),
                            health: json
                                .get("Health")
                                .and_then(serde_json::Value::as_str)
                                .map(std::string::ToString::to_string),
                        },
                    );
                }
            }
        }
    }

    (uptime, containers)
}

/// Return an unknown/error workspace status.
#[must_use]
pub fn workspace_unknown() -> WorkspaceStatus {
    WorkspaceStatus {
        status: WorkspaceState::Error,
        uptime_seconds: None,
    }
}
