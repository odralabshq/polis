//! Application service — workspace status gathering use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.
//! All I/O is routed through injected port traits.

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
pub async fn gather_status(mp: &(impl InstanceInspector + ShellExecutor)) -> StatusOutput {
    let workspace = get_workspace_status(mp).await;
    let is_running = workspace.status == WorkspaceState::Running;

    let (security, agent) = if is_running {
        (get_security_status(mp).await, get_agent_status(mp).await)
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
async fn get_workspace_status(mp: &(impl InstanceInspector + ShellExecutor)) -> WorkspaceStatus {
    let Some(vm_state) = check_multipass_status(mp).await else {
        return workspace_unknown();
    };

    if vm_state != WorkspaceState::Running {
        return WorkspaceStatus {
            status: vm_state,
            uptime_seconds: None,
        };
    }

    let container_running = check_workspace_container(mp).await;
    let uptime_seconds = get_uptime(mp).await;

    WorkspaceStatus {
        status: if container_running {
            WorkspaceState::Running
        } else {
            WorkspaceState::Starting
        },
        uptime_seconds,
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

/// Get VM uptime in seconds via shell execution.
async fn get_uptime(mp: &impl ShellExecutor) -> Option<u64> {
    let output = mp.exec(&["cat", "/proc/uptime"]).await.ok()?;
    if !output.status.success() {
        return None;
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let uptime_str = stdout.split_whitespace().next()?;
    let uptime: f64 = uptime_str.parse().ok()?;
    Some(uptime as u64)
}

/// Check if polis-workspace container is running inside VM.
async fn check_workspace_container(mp: &impl ShellExecutor) -> bool {
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
async fn get_security_status(mp: &impl ShellExecutor) -> SecurityStatus {
    let (gate, sentinel, scanner) = tokio::join!(
        is_service_running(mp, "gate"),
        is_service_running(mp, "sentinel"),
        is_service_running(mp, "scanner"),
    );

    SecurityStatus {
        traffic_inspection: gate,
        credential_protection: sentinel,
        malware_scanning: scanner,
    }
}

/// Check if a single docker compose service is running inside the VM.
async fn is_service_running(mp: &impl ShellExecutor, service: &str) -> bool {
    let output = mp
        .exec(&[
            "docker",
            "compose",
            "-f",
            COMPOSE_PATH,
            "ps",
            "--format",
            "json",
            service,
        ])
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

/// Check agent status inside multipass VM.
async fn get_agent_status(mp: &impl ShellExecutor) -> Option<AgentStatus> {
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

    Some(AgentStatus {
        name: "workspace".to_string(),
        status,
    })
}

/// Return an unknown/error workspace status.
#[must_use]
pub fn workspace_unknown() -> WorkspaceStatus {
    WorkspaceStatus {
        status: WorkspaceState::Error,
        uptime_seconds: None,
    }
}
