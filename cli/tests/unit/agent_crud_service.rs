//! Integration tests for the `agent_crud` application service.
//!
//! **Property 7 (partial): Agent operations route through provisioner**
//!
//! Verifies that `install_agent()` calls domain validation before any I/O
//! and that all file transfers / shell executions go through the injected
//! provisioner port traits.

#![allow(clippy::expect_used)]

use std::process::Output;
use std::sync::Mutex;

use anyhow::Result;
use polis_cli::application::ports::{
    FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec, ProgressReporter,
    ShellExecutor, WorkspaceStateStore,
};
use polis_cli::application::services::agent_crud::install_agent;
use polis_cli::domain::workspace::WorkspaceState;

use crate::helpers::exit_status;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ok(stdout: &[u8]) -> Output {
    Output {
        status: exit_status(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

fn fail() -> Output {
    Output {
        status: exit_status(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

/// Minimal valid agent.yaml content.
fn valid_agent_yaml(name: &str) -> String {
    format!(
        r#"apiVersion: polis.dev/v1
kind: AgentPlugin
metadata:
  name: {name}
  displayName: "Test Agent"
  version: "1.0.0"
  description: "A test agent"
spec:
  packaging: script
  install: install.sh
  runtime:
    command: /usr/bin/agent
    workdir: /app
    user: agentuser
"#
    )
}

// ── Mock: no-op progress reporter ────────────────────────────────────────────

struct NoopReporter;

impl ProgressReporter for NoopReporter {
    fn step(&self, _: &str) {}
    fn success(&self, _: &str) {}
    fn warn(&self, _: &str) {}
}

// ── Mock: in-memory state store ───────────────────────────────────────────────

struct MemoryStateStore {
    state: Mutex<Option<WorkspaceState>>,
    load_calls: Mutex<u32>,
    save_calls: Mutex<u32>,
}

impl MemoryStateStore {
    fn new(initial: Option<WorkspaceState>) -> Self {
        Self {
            state: Mutex::new(initial),
            load_calls: Mutex::new(0),
            save_calls: Mutex::new(0),
        }
    }

    fn load_count(&self) -> u32 {
        *self.load_calls.lock().expect("lock")
    }

    fn save_count(&self) -> u32 {
        *self.save_calls.lock().expect("lock")
    }
}

impl WorkspaceStateStore for MemoryStateStore {
    async fn load_async(&self) -> Result<Option<WorkspaceState>> {
        *self.load_calls.lock().expect("lock") += 1;
        Ok(self.state.lock().expect("lock").clone())
    }

    async fn save_async(&self, state: &WorkspaceState) -> Result<()> {
        *self.save_calls.lock().expect("lock") += 1;
        *self.state.lock().expect("lock") = Some(state.clone());
        Ok(())
    }
}

// ── Mock: no-op LocalArtifactWriter ──────────────────────────────────────────

struct NoopLocalArtifactWriter;

impl polis_cli::application::ports::LocalArtifactWriter for NoopLocalArtifactWriter {
    async fn write_agent_artifacts(
        &self,
        _agent_name: &str,
        _files: std::collections::HashMap<String, String>,
    ) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from("/tmp/noop"))
    }
}

// ── Mock: recording provisioner (VM running) ──────────────────────────────────

/// Records all `exec()` and `transfer()` calls. VM is always "Running".
struct RecordingProvisioner {
    exec_calls: Mutex<Vec<Vec<String>>>,
    transfer_calls: Mutex<Vec<(String, String)>>,
    /// When true, `test -d` for the agent dir returns success (agent exists).
    agent_already_exists: bool,
}

impl RecordingProvisioner {
    fn new() -> Self {
        Self {
            exec_calls: Mutex::new(Vec::new()),
            transfer_calls: Mutex::new(Vec::new()),
            agent_already_exists: false,
        }
    }

    fn with_agent_exists() -> Self {
        Self {
            exec_calls: Mutex::new(Vec::new()),
            transfer_calls: Mutex::new(Vec::new()),
            agent_already_exists: true,
        }
    }

    fn exec_call_count(&self) -> usize {
        self.exec_calls.lock().expect("lock").len()
    }

    fn transfer_call_count(&self) -> usize {
        self.transfer_calls.lock().expect("lock").len()
    }

    fn all_exec_args(&self) -> Vec<Vec<String>> {
        self.exec_calls.lock().expect("lock").clone()
    }
}

impl InstanceInspector for RecordingProvisioner {
    async fn info(&self) -> Result<Output> {
        Ok(ok(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        Ok(ok(b"multipass 1.16.0
multipassd 1.16.0"))
    }
}

impl InstanceLifecycle for RecordingProvisioner {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn start(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn stop(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok(b""))
    }
}

impl FileTransfer for RecordingProvisioner {
    async fn transfer(&self, local: &str, remote: &str) -> Result<Output> {
        self.transfer_calls
            .lock()
            .expect("lock")
            .push((local.to_owned(), remote.to_owned()));
        Ok(ok(b""))
    }
    async fn transfer_recursive(&self, local: &str, remote: &str) -> Result<Output> {
        self.transfer_calls
            .lock()
            .expect("lock")
            .push((local.to_owned(), remote.to_owned()));
        Ok(ok(b""))
    }
}

impl ShellExecutor for RecordingProvisioner {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        self.exec_calls
            .lock()
            .expect("lock")
            .push(args.iter().map(ToString::to_string).collect());
        // `test -d <dir>` — return success only if agent_already_exists
        let joined = args.join(" ");
        if joined.starts_with("test -d") {
            return Ok(if self.agent_already_exists {
                ok(b"")
            } else {
                fail()
            });
        }
        Ok(ok(b""))
    }
    async fn exec_with_stdin(&self, args: &[&str], _: &[u8]) -> Result<Output> {
        self.exec_calls
            .lock()
            .expect("lock")
            .push(args.iter().map(ToString::to_string).collect());
        Ok(ok(b""))
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not expected in agent_crud tests")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        Ok(exit_status(0))
    }
}

// ── Mock: VM stopped provisioner ─────────────────────────────────────────────

struct VmStoppedProvisioner;

impl InstanceInspector for VmStoppedProvisioner {
    async fn info(&self) -> Result<Output> {
        Ok(ok(br#"{"info":{"polis":{"state":"Stopped"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        Ok(ok(b"multipass 1.16.0
multipassd 1.16.0"))
    }
}

impl InstanceLifecycle for VmStoppedProvisioner {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn start(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn stop(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok(b""))
    }
}

impl FileTransfer for VmStoppedProvisioner {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        Ok(ok(b""))
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        Ok(ok(b""))
    }
}

impl ShellExecutor for VmStoppedProvisioner {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(fail())
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        Ok(ok(b""))
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("not expected")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        Ok(exit_status(0))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// **Property 7 (partial): Agent operations route through provisioner**
///
/// `install_agent()` must call domain validation (reads agent.yaml from disk)
/// before making any VM calls. Verifies that all file transfers go through
/// the injected `FileTransfer` port.
#[tokio::test]
async fn install_agent_routes_transfer_through_provisioner() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let agent_dir = tmp.path().join("agents").join("test-agent");
    std::fs::create_dir_all(&agent_dir).expect("create agent dir");
    std::fs::write(agent_dir.join("agent.yaml"), valid_agent_yaml("test-agent"))
        .expect("write agent.yaml");

    let provisioner = RecordingProvisioner::new();
    let state_mgr = MemoryStateStore::new(None);
    let artifact_writer = NoopLocalArtifactWriter;
    let reporter = NoopReporter;

    let result = install_agent(
        &provisioner,
        &state_mgr,
        &artifact_writer,
        &reporter,
        agent_dir.to_str().expect("path"),
    )
    .await
    .expect("install_agent should succeed");

    assert_eq!(result, "test-agent");

    // At least one transfer call must have gone through the provisioner
    assert!(
        provisioner.transfer_call_count() > 0,
        "install_agent must route file transfer through provisioner, got {} transfer calls",
        provisioner.transfer_call_count()
    );
}

/// Domain validation runs before any VM I/O.
///
/// When the agent.yaml is missing, `install_agent()` must fail immediately
/// without making any exec or transfer calls to the provisioner.
#[tokio::test]
async fn install_agent_validates_before_vm_io() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    // No agent.yaml — validation should fail immediately
    let agent_dir = tmp.path().join("agents").join("bad-agent");
    std::fs::create_dir_all(&agent_dir).expect("create dir");

    let provisioner = RecordingProvisioner::new();
    let state_mgr = MemoryStateStore::new(None);
    let artifact_writer = NoopLocalArtifactWriter;
    let reporter = NoopReporter;

    let result = install_agent(
        &provisioner,
        &state_mgr,
        &artifact_writer,
        &reporter,
        agent_dir.to_str().expect("path"),
    )
    .await;

    assert!(result.is_err(), "should fail when agent.yaml is missing");
    // No VM calls should have been made — validation failed before any I/O
    assert_eq!(
        provisioner.exec_call_count(),
        0,
        "no exec calls should be made when domain validation fails"
    );
    assert_eq!(
        provisioner.transfer_call_count(),
        0,
        "no transfer calls should be made when domain validation fails"
    );
}

/// VM must be Running for install to proceed.
///
/// When the VM is stopped, `install_agent()` must fail with a clear message
/// after domain validation but before any file transfer.
#[tokio::test]
async fn install_agent_requires_vm_running() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let agent_dir = tmp.path().join("agents").join("test-agent");
    std::fs::create_dir_all(&agent_dir).expect("create agent dir");
    std::fs::write(agent_dir.join("agent.yaml"), valid_agent_yaml("test-agent"))
        .expect("write agent.yaml");

    let provisioner = VmStoppedProvisioner;
    let state_mgr = MemoryStateStore::new(None);
    let artifact_writer = NoopLocalArtifactWriter;
    let reporter = NoopReporter;

    let result = install_agent(
        &provisioner,
        &state_mgr,
        &artifact_writer,
        &reporter,
        agent_dir.to_str().expect("path"),
    )
    .await;

    assert!(result.is_err(), "should fail when VM is not running");
    let msg = result.expect_err("expected error").to_string();
    assert!(
        msg.to_lowercase().contains("not running") || msg.to_lowercase().contains("start"),
        "error should mention VM not running or suggest starting: {msg}"
    );
}

/// Attempting to install an already-installed agent returns an error.
#[tokio::test]
async fn install_agent_fails_if_already_installed() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let agent_dir = tmp.path().join("agents").join("test-agent");
    std::fs::create_dir_all(&agent_dir).expect("create agent dir");
    std::fs::write(agent_dir.join("agent.yaml"), valid_agent_yaml("test-agent"))
        .expect("write agent.yaml");

    // Provisioner reports agent dir already exists
    let provisioner = RecordingProvisioner::with_agent_exists();
    let state_mgr = MemoryStateStore::new(None);
    let artifact_writer = NoopLocalArtifactWriter;
    let reporter = NoopReporter;

    let result = install_agent(
        &provisioner,
        &state_mgr,
        &artifact_writer,
        &reporter,
        agent_dir.to_str().expect("path"),
    )
    .await;

    assert!(
        result.is_err(),
        "should fail when agent is already installed"
    );
    let msg = result.expect_err("expected error").to_string();
    assert!(
        msg.contains("already installed") || msg.contains("Remove"),
        "error should mention already installed: {msg}"
    );
    // No transfer should have occurred
    assert_eq!(
        provisioner.transfer_call_count(),
        0,
        "no transfer when agent already exists"
    );
}

/// Progress reporter receives step/success calls during install.
#[tokio::test]
async fn install_agent_calls_progress_reporter() {
    use std::sync::Arc;

    struct CountingReporter {
        steps: Arc<Mutex<Vec<String>>>,
        successes: Arc<Mutex<Vec<String>>>,
    }

    impl ProgressReporter for CountingReporter {
        fn step(&self, msg: &str) {
            self.steps.lock().expect("lock").push(msg.to_owned());
        }
        fn success(&self, msg: &str) {
            self.successes.lock().expect("lock").push(msg.to_owned());
        }
        fn warn(&self, _: &str) {}
    }

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let agent_dir = tmp.path().join("agents").join("test-agent");
    std::fs::create_dir_all(&agent_dir).expect("create agent dir");
    std::fs::write(agent_dir.join("agent.yaml"), valid_agent_yaml("test-agent"))
        .expect("write agent.yaml");

    let steps = Arc::new(Mutex::new(Vec::new()));
    let successes = Arc::new(Mutex::new(Vec::new()));
    let reporter = CountingReporter {
        steps: Arc::clone(&steps),
        successes: Arc::clone(&successes),
    };

    let provisioner = RecordingProvisioner::new();
    let state_mgr = MemoryStateStore::new(None);
    let artifact_writer = NoopLocalArtifactWriter;

    install_agent(
        &provisioner,
        &state_mgr,
        &artifact_writer,
        &reporter,
        agent_dir.to_str().expect("path"),
    )
    .await
    .expect("install should succeed");

    assert!(
        !steps.lock().expect("lock").is_empty(),
        "reporter should receive step() calls"
    );
    assert!(
        !successes.lock().expect("lock").is_empty(),
        "reporter should receive success() calls"
    );
}
