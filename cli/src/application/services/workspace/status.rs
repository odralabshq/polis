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

struct ContainerInfo {
    state: String,
    health: Option<String>,
}

pub async fn gather(mp: &(impl InstanceInspector + ShellExecutor)) -> StatusOutput {
    let Some(vm_state) = check_multipass_status(mp).await else {
        return StatusOutput {
            workspace: workspace_unknown(),
            agent: None,
            containers: None,
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
            containers: None,
            security: empty_security(),
            events: empty_events(),
        };
    }

    // VM is running, gather detailed status in a single consolidated call
    let (uptime_seconds, containers) = gather_remote_info(mp).await;

    let workspace_info = containers.get("workspace");
    let is_workspace_running = workspace_info.is_some_and(|i| i.state == "running");

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
        containers: None,
        security: SecurityStatus {
            traffic_inspection: containers.get("gate").is_some_and(|i| i.state == "running"),
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

    let output = mp.exec(&[QUERY_SCRIPT, "status"]).await;

    let Ok(o) = output else {
        return (uptime, containers);
    };
    if !o.status.success() {
        return (uptime, containers);
    }

    if let Ok(response) = serde_json::from_slice::<StatusResponse>(&o.stdout) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            uptime = response.uptime.map(|u| u as u64);
        }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::application::ports::{InstanceInspector, ShellExecutor};
    use crate::application::vm::test_support::{fail_output, impl_shell_executor_stubs, ok_output};
    use anyhow::Result;
    use std::process::Output;

    // ── Combined stub (InstanceInspector + ShellExecutor) ─────────────────

    struct StatusStub {
        info_out: Output,
        exec_out: Output,
    }

    impl StatusStub {
        fn not_found() -> Self {
            Self {
                info_out: Output {
                    status: crate::application::vm::test_support::exit_status(1),
                    stdout: vec![],
                    stderr: b"instance \"polis\" does not exist".to_vec(),
                },
                exec_out: fail_output(),
            }
        }
        fn stopped() -> Self {
            Self {
                info_out: ok_output(br#"{"info":{"polis":{"state":"Stopped","ipv4":[]}}}"#),
                exec_out: fail_output(),
            }
        }
        fn running(exec_json: &[u8]) -> Self {
            Self {
                info_out: ok_output(
                    br#"{"info":{"polis":{"state":"Running","ipv4":["10.0.0.1"]}}}"#,
                ),
                exec_out: ok_output(exec_json),
            }
        }
    }

    impl InstanceInspector for StatusStub {
        async fn info(&self) -> Result<Output> {
            Ok(Output {
                status: self.info_out.status,
                stdout: self.info_out.stdout.clone(),
                stderr: self.info_out.stderr.clone(),
            })
        }
        async fn version(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }

    impl ShellExecutor for StatusStub {
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            Ok(Output {
                status: self.exec_out.status,
                stdout: self.exec_out.stdout.clone(),
                stderr: self.exec_out.stderr.clone(),
            })
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    #[tokio::test]
    async fn gather_vm_not_found_returns_error_status() {
        let mp = StatusStub::not_found();
        let out = gather(&mp).await;
        assert_eq!(out.workspace.status, WorkspaceState::NotFound);
        assert!(out.agent.is_none());
    }

    #[tokio::test]
    async fn gather_vm_stopped_returns_stopped_status() {
        let mp = StatusStub::stopped();
        let out = gather(&mp).await;
        assert_eq!(out.workspace.status, WorkspaceState::Stopped);
        assert!(out.agent.is_none());
    }

    #[tokio::test]
    async fn gather_vm_running_healthy_workspace() {
        let exec_json = br#"{"uptime":120.0,"containers":[{"Service":"workspace","State":"running","Health":"healthy"},{"Service":"gate","State":"running","Health":null},{"Service":"sentinel","State":"running","Health":null},{"Service":"scanner","State":"running","Health":null}]}"#;
        let mp = StatusStub::running(exec_json);
        let out = gather(&mp).await;
        assert_eq!(out.workspace.status, WorkspaceState::Running);
        assert!(out.agent.is_some());
        assert_eq!(out.agent.unwrap().status, AgentHealth::Healthy);
        assert!(out.security.traffic_inspection);
        assert!(out.security.credential_protection);
        assert!(out.security.malware_scanning);
    }

    #[tokio::test]
    async fn gather_vm_running_unhealthy_workspace() {
        let exec_json = br#"{"uptime":30.0,"containers":[{"Service":"workspace","State":"running","Health":"unhealthy"}]}"#;
        let mp = StatusStub::running(exec_json);
        let out = gather(&mp).await;
        assert!(matches!(out.agent.unwrap().status, AgentHealth::Unhealthy));
    }

    #[tokio::test]
    async fn gather_exec_failure_returns_starting_state() {
        // exec fails → no containers → workspace not in map → Starting
        let mp = StatusStub::running(b"not-json");
        let out = gather(&mp).await;
        assert_eq!(out.workspace.status, WorkspaceState::Starting);
    }

    #[tokio::test]
    async fn workspace_unknown_returns_error() {
        let ws = workspace_unknown();
        assert_eq!(ws.status, WorkspaceState::Error);
        assert!(ws.uptime_seconds.is_none());
    }
}
