use anyhow::Result;
use cp_api_types::{
    ActionResponse, AgentResponse, BlockedListResponse, ContainersResponse, EventsResponse,
    LevelResponse, RulesResponse, StatusResponse, WorkspaceResponse,
};
use polis_cli::application::ports::{ControlPlanePort, InstanceInspector, ShellExecutor};
use polis_cli::application::services::workspace_status::gather_status;
use polis_common::types::{AgentHealth, WorkspaceState};
use std::process::{ExitStatus, Output};

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

fn mock_output(stdout: &[u8], success: bool) -> Output {
    Output {
        status: exit_status(i32::from(!success)),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

struct MockVm {
    info_response: Result<Output>,
    exec_responses: std::collections::HashMap<Vec<String>, Result<Output>>,
}

impl MockVm {
    fn new() -> Self {
        Self {
            info_response: Ok(mock_output(b"{}", false)),
            exec_responses: std::collections::HashMap::new(),
        }
    }

    fn with_info(mut self, stdout: &[u8]) -> Self {
        self.info_response = Ok(mock_output(stdout, true));
        self
    }

    fn with_exec(mut self, args: &[&str], stdout: &[u8], success: bool) -> Self {
        let key = args.iter().map(ToString::to_string).collect();
        self.exec_responses
            .insert(key, Ok(mock_output(stdout, success)));
        self
    }
}

impl InstanceInspector for MockVm {
    async fn info(&self) -> Result<Output> {
        self.info_response
            .as_ref()
            .map(|o| Output {
                status: o.status,
                stdout: o.stdout.clone(),
                stderr: o.stderr.clone(),
            })
            .map_err(|_| anyhow::anyhow!("mock error"))
    }

    async fn version(&self) -> Result<Output> {
        Ok(mock_output(b"", true))
    }
}

impl ShellExecutor for MockVm {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        let key: Vec<String> = args.iter().map(ToString::to_string).collect();
        if let Some(res) = self.exec_responses.get(&key) {
            return res
                .as_ref()
                .map(|o| Output {
                    status: o.status,
                    stdout: o.stdout.clone(),
                    stderr: o.stderr.clone(),
                })
                .map_err(|_| anyhow::anyhow!("mock error"));
        }
        Ok(mock_output(b"", false))
    }

    async fn exec_with_stdin(&self, _args: &[&str], _input: &[u8]) -> Result<Output> {
        Ok(mock_output(b"", true))
    }

    fn exec_spawn(&self, _args: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not implemented in mock")
    }

    async fn exec_status(&self, _args: &[&str]) -> Result<ExitStatus> {
        Ok(exit_status(0))
    }
}

struct NullControlPlane;

impl ControlPlanePort for NullControlPlane {
    async fn status(&self) -> Result<StatusResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn blocked_requests(&self) -> Result<BlockedListResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn approve_request(&self, _request_id: &str) -> Result<ActionResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn deny_request(&self, _request_id: &str) -> Result<ActionResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn security_events(&self, _limit: usize) -> Result<EventsResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn rules(&self) -> Result<RulesResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn add_rule(&self, _pattern: &str, _action: &str) -> Result<ActionResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn set_security_level(&self, _level: &str) -> Result<LevelResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn workspace(&self) -> Result<WorkspaceResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn agent(&self) -> Result<AgentResponse> {
        anyhow::bail!("control plane unavailable")
    }

    async fn containers(&self) -> Result<ContainersResponse> {
        anyhow::bail!("control plane unavailable")
    }
}

struct ReadyControlPlane;

impl ControlPlanePort for ReadyControlPlane {
    async fn status(&self) -> Result<StatusResponse> {
        Ok(StatusResponse {
            security_level: "balanced".to_string(),
            pending_count: 2,
            recent_approvals: 1,
            events_count: 5,
        })
    }

    async fn blocked_requests(&self) -> Result<BlockedListResponse> {
        Ok(BlockedListResponse { items: Vec::new() })
    }

    async fn approve_request(&self, _request_id: &str) -> Result<ActionResponse> {
        Ok(ActionResponse {
            message: "approved".to_string(),
        })
    }

    async fn deny_request(&self, _request_id: &str) -> Result<ActionResponse> {
        Ok(ActionResponse {
            message: "denied".to_string(),
        })
    }

    async fn security_events(&self, _limit: usize) -> Result<EventsResponse> {
        Ok(EventsResponse { events: Vec::new() })
    }

    async fn rules(&self) -> Result<RulesResponse> {
        Ok(RulesResponse { rules: Vec::new() })
    }

    async fn add_rule(&self, _pattern: &str, _action: &str) -> Result<ActionResponse> {
        Ok(ActionResponse {
            message: "rule added".to_string(),
        })
    }

    async fn set_security_level(&self, level: &str) -> Result<LevelResponse> {
        Ok(LevelResponse {
            level: level.to_string(),
        })
    }

    async fn workspace(&self) -> Result<WorkspaceResponse> {
        Ok(WorkspaceResponse {
            status: "running".to_string(),
            uptime_seconds: Some(90),
            containers: cp_api_types::ContainerSummary {
                total: 4,
                healthy: 3,
                unhealthy: 1,
                starting: 0,
            },
            networks: std::collections::HashMap::new(),
        })
    }

    async fn agent(&self) -> Result<AgentResponse> {
        Ok(AgentResponse {
            name: "openclaw".to_string(),
            display_name: "OpenClaw".to_string(),
            version: "1.0.0".to_string(),
            status: "running".to_string(),
            health: "healthy".to_string(),
            uptime_seconds: Some(90),
            ports: Vec::new(),
            resources: cp_api_types::ResourceUsage {
                memory_usage_mb: 512,
                memory_limit_mb: 4096,
                cpu_percent: 12.5,
            },
            stale: false,
        })
    }

    async fn containers(&self) -> Result<ContainersResponse> {
        Ok(ContainersResponse {
            containers: vec![
                cp_api_types::ContainerInfo {
                    name: "polis-gate".to_string(),
                    service: "gate".to_string(),
                    status: "running".to_string(),
                    health: "healthy".to_string(),
                    uptime_seconds: Some(90),
                    memory_usage_mb: 10,
                    memory_limit_mb: 256,
                    cpu_percent: 1.0,
                    network: "internal".to_string(),
                    ip: "10.0.0.2".to_string(),
                    stale: false,
                },
                cp_api_types::ContainerInfo {
                    name: "polis-sentinel".to_string(),
                    service: "sentinel".to_string(),
                    status: "running".to_string(),
                    health: "starting".to_string(),
                    uptime_seconds: Some(90),
                    memory_usage_mb: 12,
                    memory_limit_mb: 256,
                    cpu_percent: 2.0,
                    network: "internal".to_string(),
                    ip: "10.0.0.3".to_string(),
                    stale: false,
                },
                cp_api_types::ContainerInfo {
                    name: "polis-scanner".to_string(),
                    service: "scanner".to_string(),
                    status: "running".to_string(),
                    health: "healthy".to_string(),
                    uptime_seconds: Some(90),
                    memory_usage_mb: 14,
                    memory_limit_mb: 256,
                    cpu_percent: 3.0,
                    network: "internal".to_string(),
                    ip: "10.0.0.4".to_string(),
                    stale: false,
                },
            ],
        })
    }
}

#[tokio::test]
async fn status_parses_healthy_response() {
    let mock = MockVm::new()
        .with_info(br#"{"info":{"polis":{"state":"Running"}}}"#)
        .with_exec(
            &["/opt/polis/scripts/polis-query.sh", "status"],
            br#"{"uptime":1764.75,"containers":[
                {"Service":"workspace","State":"running","Health":"healthy"},
                {"Service":"gate","State":"running","Health":""},
                {"Service":"sentinel","State":"running","Health":""},
                {"Service":"scanner","State":"running","Health":""}
            ]}"#,
            true,
        );

    let result = gather_status(&NullControlPlane, &mock).await;
    assert_eq!(result.workspace.status, WorkspaceState::Running);
    assert_eq!(result.workspace.uptime_seconds, Some(1764));
    assert_eq!(
        result.agent.as_ref().map(|a| a.status),
        Some(AgentHealth::Healthy)
    );
    assert!(result.security.traffic_inspection);
    assert!(result.security.credential_protection);
    assert!(result.security.malware_scanning);
}

#[tokio::test]
async fn status_degrades_gracefully_when_script_missing() {
    let mock = MockVm::new()
        .with_info(br#"{"info":{"polis":{"state":"Running"}}}"#)
        .with_exec(
            &["/opt/polis/scripts/polis-query.sh", "status"],
            b"command not found",
            false,
        );

    let result = gather_status(&NullControlPlane, &mock).await;
    assert_eq!(result.workspace.status, WorkspaceState::Starting);
    assert!(!result.security.traffic_inspection);
}

#[tokio::test]
async fn status_handles_malformed_json() {
    let mock = MockVm::new()
        .with_info(br#"{"info":{"polis":{"state":"Running"}}}"#)
        .with_exec(
            &["/opt/polis/scripts/polis-query.sh", "status"],
            b"not json at all",
            true,
        );

    let result = gather_status(&NullControlPlane, &mock).await;
    assert_eq!(result.workspace.status, WorkspaceState::Starting);
}

#[tokio::test]
async fn status_prefers_control_plane_when_available() {
    let mock = MockVm::new();

    let result = gather_status(&ReadyControlPlane, &mock).await;

    assert_eq!(result.workspace.status, WorkspaceState::Running);
    assert_eq!(result.workspace.uptime_seconds, Some(90));
    assert_eq!(
        result.agent.as_ref().map(|agent| agent.name.as_str()),
        Some("OpenClaw")
    );
    assert_eq!(result.events.count, 5);
    assert_eq!(
        result
            .containers
            .as_ref()
            .map(|containers| containers.healthy),
        Some(3)
    );
    assert!(result.security.traffic_inspection);
    assert!(result.security.credential_protection);
    assert!(result.security.malware_scanning);
}
