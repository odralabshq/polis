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
use crate::domain::workspace::QUERY_SCRIPT;

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
        .is_some_and(|i| i.state == "running");

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
                .is_some_and(|i| i.state == "running"),
            credential_protection: containers
                .get("sentinel")
                .is_some_and(|i| i.state == "running"),
            malware_scanning: containers
                .get("scanner")
                .is_some_and(|i| i.state == "running"),
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

#[derive(serde::Deserialize)]
struct StatusResponse {
    uptime: Option<f64>,
    containers: Vec<ContainerEntry>,
}

#[derive(serde::Deserialize)]
struct ContainerEntry {
    #[serde(rename = "Service")]
    service: String,
    #[serde(rename = "State")]
    state: String,
    #[serde(rename = "Health")]
    health: Option<String>,
}

/// Gather uptime and container info in a single remote call.
async fn gather_remote_info(
    mp: &impl ShellExecutor,
) -> (Option<u64>, HashMap<String, ContainerInfo>) {
    let mut containers = HashMap::new();
    let mut uptime = None;

    // Call the query script inside the VM to avoid Multipass Windows pipe issues.
    let output = mp.exec(&[QUERY_SCRIPT, "status"]).await;

    let Ok(o) = output else {
        return (uptime, containers);
    };
    if !o.status.success() {
        return (uptime, containers);
    }

    // Parse the consolidated JSON response.
    if let Ok(response) = serde_json::from_slice::<StatusResponse>(&o.stdout) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        { uptime = response.uptime.map(|u| u as u64); }
        for entry in response.containers {
            containers.insert(
                entry.service,
                ContainerInfo {
                    state: entry.state,
                    health: entry.health,
                },
            );
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
