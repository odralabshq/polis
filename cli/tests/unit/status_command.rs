use anyhow::Result;
use polis_cli::application::ports::{InstanceInspector, ShellExecutor};
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

    let result = gather_status(&mock).await;
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

    let result = gather_status(&mock).await;
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

    let result = gather_status(&mock).await;
    assert_eq!(result.workspace.status, WorkspaceState::Starting);
}
