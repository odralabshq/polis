//! Unit tests for `polis agent` command.

#![allow(clippy::expect_used)]

use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::commands::agent::{self, AddArgs, AgentCommand, RemoveArgs};
use polis_cli::multipass::Multipass;
use polis_cli::output::OutputContext;

use crate::helpers::{ok_output, err_output};

fn quiet() -> OutputContext {
    OutputContext::new(true, true)
}

// ── Stub: returns fixed Output for every exec() call ────────────────────────

struct ExecStub(Output);

impl Multipass for ExecStub {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected in this test")
    }
    async fn start(&self) -> Result<Output> { anyhow::bail!("not expected in this test") }
    async fn stop(&self) -> Result<Output> { anyhow::bail!("not expected in this test") }
    async fn delete(&self) -> Result<Output> { anyhow::bail!("not expected in this test") }
    async fn purge(&self) -> Result<Output> { anyhow::bail!("not expected in this test") }
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("not expected in this test")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        Ok(Output {
            status: self.0.status,
            stdout: self.0.stdout.clone(),
            stderr: self.0.stderr.clone(),
        })
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(Output {
            status: self.0.status,
            stdout: self.0.stdout.clone(),
            stderr: self.0.stderr.clone(),
        })
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("not expected in this test")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected in this test")
    }
    async fn version(&self) -> Result<Output> { anyhow::bail!("not expected in this test") }
}

// ============================================================================
// list
// ============================================================================

#[tokio::test]
async fn test_list_empty_returns_ok() {
    let mp = ExecStub(ok_output(b""));
    let result = agent::run(AgentCommand::List, &mp, &quiet(), false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_json_returns_ok_with_agents_array() {
    let line = br#"{"dir":"myagent","name":"myagent","version":"v1.0","description":"My agent"}"#;
    let mp = ExecStub(ok_output(line));
    let result = agent::run(AgentCommand::List, &mp, &quiet(), true).await;
    assert!(result.is_ok());
}

// ============================================================================
// restart
// ============================================================================

#[tokio::test]
async fn test_restart_no_active_agent_returns_error() {
    let mp = ExecStub(ok_output(b""));
    let result = agent::run(AgentCommand::Restart, &mp, &quiet(), false).await;
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("No active agent"),
        "error should mention no active agent"
    );
}

// ============================================================================
// remove
// ============================================================================

#[tokio::test]
async fn test_remove_agent_not_installed_returns_error() {
    // exec returns failure → "test -d" fails → agent not installed
    let mp = ExecStub(err_output(1, b""));
    let result = agent::run(
        AgentCommand::Remove(RemoveArgs { name: "nonexistent".to_string() }),
        &mp,
        &quiet(),
        false,
    )
    .await;
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("not installed"),
        "error should mention not installed"
    );
}

// ============================================================================
// add — path validation (no VM needed)
// ============================================================================

#[tokio::test]
async fn test_add_path_not_found_returns_error() {
    let mp = ExecStub(ok_output(b""));
    let result = agent::run(
        AgentCommand::Add(AddArgs { path: "/nonexistent/path".to_string() }),
        &mp,
        &quiet(),
        false,
    )
    .await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("Path not found") || msg.contains("not found"), "got: {msg}");
}

#[tokio::test]
async fn test_add_missing_agent_yaml_returns_error() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let mp = ExecStub(ok_output(b""));
    let result = agent::run(
        AgentCommand::Add(AddArgs { path: dir.path().to_string_lossy().into_owned() }),
        &mp,
        &quiet(),
        false,
    )
    .await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("agent.yaml"), "got: {msg}");
}

#[tokio::test]
async fn test_add_malformed_agent_yaml_returns_error() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("agent.yaml"), b"{ not: valid: yaml: [[[").expect("write");
    let mp = ExecStub(ok_output(b""));
    let result = agent::run(
        AgentCommand::Add(AddArgs { path: dir.path().to_string_lossy().into_owned() }),
        &mp,
        &quiet(),
        false,
    )
    .await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("parse") || msg.contains("agent.yaml"), "got: {msg}");
}

// ============================================================================
// update — no active agent
// ============================================================================

#[tokio::test]
async fn test_update_no_active_agent_returns_error() {
    let mp = ExecStub(ok_output(b""));
    let result = agent::run(AgentCommand::Update, &mp, &quiet(), false).await;
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("No active agent"),
        "error should mention no active agent"
    );
}
