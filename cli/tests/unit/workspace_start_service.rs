//! Integration tests for the `workspace_start` application service.
//!
//! **Property 7 (partial): All VM operations route through provisioner**
//!
//! Verifies that `start_workspace()` routes all VM interactions through the
//! injected `VmProvisioner` and `WorkspaceStateStore` port traits.

#![allow(clippy::expect_used)]

use std::process::Output;
use std::sync::Mutex;

use anyhow::Result;
use polis_cli::application::ports::{
    FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec, ProgressReporter,
    ShellExecutor, WorkspaceStateStore,
};
use polis_cli::application::services::workspace_start::{StartOutcome, start_workspace};
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

    fn saved_state(&self) -> Option<WorkspaceState> {
        self.state.lock().expect("lock").clone()
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

// ── Mock: recording provisioner (VM running) ──────────────────────────────────

/// Records all `exec()` calls. VM is always "Running".
struct RecordingProvisioner {
    execs: Mutex<Vec<Vec<String>>>,
    execs_with_stdin: Mutex<Vec<Vec<String>>>,
    transfers: Mutex<Vec<(String, String)>>,
}

impl RecordingProvisioner {
    fn new() -> Self {
        Self {
            execs: Mutex::new(Vec::new()),
            execs_with_stdin: Mutex::new(Vec::new()),
            transfers: Mutex::new(Vec::new()),
        }
    }

    fn exec_call_count(&self) -> usize {
        self.execs.lock().expect("lock").len()
    }

    fn transfer_call_count(&self) -> usize {
        self.transfers.lock().expect("lock").len()
    }

    fn all_exec_args(&self) -> Vec<Vec<String>> {
        self.execs.lock().expect("lock").clone()
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
        self.transfers
            .lock()
            .expect("lock")
            .push((local.to_owned(), remote.to_owned()));
        Ok(ok(b""))
    }
    async fn transfer_recursive(&self, local: &str, remote: &str) -> Result<Output> {
        self.transfers
            .lock()
            .expect("lock")
            .push((local.to_owned(), remote.to_owned()));
        Ok(ok(b""))
    }
}

impl ShellExecutor for RecordingProvisioner {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        self.execs
            .lock()
            .expect("lock")
            .push(args.iter().map(ToString::to_string).collect());
        Ok(ok(b""))
    }
    async fn exec_with_stdin(&self, args: &[&str], _: &[u8]) -> Result<Output> {
        self.execs_with_stdin
            .lock()
            .expect("lock")
            .push(args.iter().map(ToString::to_string).collect());
        Ok(ok(b""))
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not expected")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        Ok(exit_status(0))
    }
}

// ── Mock: VM not found provisioner ────────────────────────────────────────────

/// VM is not found — simulates a fresh install scenario.
struct VmNotFoundProvisioner {
    exec_calls: Mutex<Vec<Vec<String>>>,
    transfer_calls: Mutex<Vec<(String, String)>>,
}

impl VmNotFoundProvisioner {
    fn new() -> Self {
        Self {
            exec_calls: Mutex::new(Vec::new()),
            transfer_calls: Mutex::new(Vec::new()),
        }
    }

    fn exec_call_count(&self) -> usize {
        self.exec_calls.lock().expect("lock").len()
    }

    fn transfer_call_count(&self) -> usize {
        self.transfer_calls.lock().expect("lock").len()
    }
}

impl InstanceInspector for VmNotFoundProvisioner {
    async fn info(&self) -> Result<Output> {
        Ok(fail()) // VM not found
    }
    async fn version(&self) -> Result<Output> {
        Ok(ok(b"multipass 1.16.0
multipassd 1.16.0"))
    }
}

impl InstanceLifecycle for VmNotFoundProvisioner {
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

impl FileTransfer for VmNotFoundProvisioner {
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

impl ShellExecutor for VmNotFoundProvisioner {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        self.exec_calls
            .lock()
            .expect("lock")
            .push(args.iter().map(ToString::to_string).collect());
        // Return healthy docker compose ps output for health check
        let joined = args.join(" ");
        if joined.contains("docker compose") && joined.contains("ps") {
            return Ok(ok(br#"{"State":"running","Health":"healthy"}"#));
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
        anyhow::bail!("exec_spawn not expected")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        Ok(exit_status(0)) // cloud-init success
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// **Property 7 (partial): All VM operations route through provisioner**
///
/// When VM is already running with the same agent config, `start_workspace`
/// returns `AlreadyRunning` without making any VM calls.
#[tokio::test]
async fn already_running_same_agent_returns_already_running() {
    use chrono::Utc;
    let provisioner = RecordingProvisioner::new();
    let state_mgr = MemoryStateStore::new(Some(WorkspaceState {
        workspace_id: "polis-0123456789abcdef".to_string(),
        created_at: Utc::now(),
        image_sha256: None,
        image_source: None,
        active_agent: None,
    }));
    let reporter = NoopReporter;

    let result = start_workspace(
        &provisioner,
        &state_mgr,
        &reporter,
        None,
        std::path::Path::new("/nonexistent"),
        "0.1.0",
    )
    .await
    .expect("start_workspace should succeed");

    assert!(
        matches!(result, StartOutcome::AlreadyRunning { agent: None }),
        "expected AlreadyRunning"
    );
    // State was loaded to check current agent
    assert_eq!(state_mgr.load_count(), 1, "should load state once");
    // No state was saved (nothing changed)
    assert_eq!(state_mgr.save_count(), 0, "should not save state");
    // No exec calls beyond info() for VM state check
    assert_eq!(
        provisioner.exec_call_count(),
        0,
        "no exec calls when already running with same config"
    );
}

/// When VM is running with a different agent, `start_workspace` returns an error.
#[tokio::test]
async fn already_running_different_agent_returns_error() {
    use chrono::Utc;
    let provisioner = RecordingProvisioner::new();
    let state_mgr = MemoryStateStore::new(Some(WorkspaceState {
        workspace_id: "polis-0123456789abcdef".to_string(),
        created_at: Utc::now(),
        image_sha256: None,
        image_source: None,
        active_agent: Some("openclaw".to_string()),
    }));
    let reporter = NoopReporter;

    let result = start_workspace(
        &provisioner,
        &state_mgr,
        &reporter,
        None, // requesting no agent, but "openclaw" is running
        std::path::Path::new("/nonexistent"),
        "0.1.0",
    )
    .await;

    assert!(result.is_err(), "should fail when agent config differs");
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            msg.contains("Stop first"),
            "error should suggest stopping: {msg}"
        );
    }
}

/// When VM is not found, `start_workspace` routes all VM operations through
/// the provisioner (launch, transfer, exec, etc.).
#[tokio::test]
async fn vm_not_found_routes_all_ops_through_provisioner() {
    let provisioner = VmNotFoundProvisioner::new();
    let state_mgr = MemoryStateStore::new(None);
    let reporter = NoopReporter;

    // Use a real temp dir with a fake tarball for the sha256 step
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let tar_path = tmp.path().join("polis-setup.config.tar");
    // Create a minimal valid tar file
    {
        let file = std::fs::File::create(&tar_path).expect("create tar");
        let mut builder = tar::Builder::new(file);
        let data = b"test content";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "test.txt", data.as_ref())
            .expect("append");
        builder.finish().expect("finish tar");
    }

    let result = start_workspace(
        &provisioner,
        &state_mgr,
        &reporter,
        None,
        tmp.path(),
        "0.1.0",
    )
    .await
    .expect("start_workspace should succeed for new VM");

    assert!(
        matches!(result, StartOutcome::Created { agent: None }),
        "expected Created outcome"
    );

    // Verify VM operations went through provisioner
    assert!(
        provisioner.exec_call_count() > 0,
        "should have made exec calls through provisioner"
    );
    assert!(
        provisioner.transfer_call_count() > 0,
        "should have made transfer calls through provisioner"
    );

    // State should have been saved after successful creation
    assert_eq!(state_mgr.save_count(), 1, "should save state once");
    let saved = state_mgr.saved_state().expect("state should be saved");
    assert!(
        saved.workspace_id.starts_with("polis-"),
        "workspace_id should have polis- prefix"
    );
    assert_eq!(saved.active_agent, None);
}

/// Progress reporter receives step/success calls during provisioning.
#[tokio::test]
async fn progress_reporter_receives_calls() {
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

    let steps = Arc::new(Mutex::new(Vec::new()));
    let successes = Arc::new(Mutex::new(Vec::new()));
    let reporter = CountingReporter {
        steps: Arc::clone(&steps),
        successes: Arc::clone(&successes),
    };

    let provisioner = VmNotFoundProvisioner::new();
    let state_mgr = MemoryStateStore::new(None);

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let tar_path = tmp.path().join("polis-setup.config.tar");
    {
        let file = std::fs::File::create(&tar_path).expect("create tar");
        let mut builder = tar::Builder::new(file);
        let data = b"test";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "test.txt", data.as_ref())
            .expect("append");
        builder.finish().expect("finish tar");
    }

    start_workspace(
        &provisioner,
        &state_mgr,
        &reporter,
        None,
        tmp.path(),
        "0.1.0",
    )
    .await
    .expect("should succeed");

    let step_count = steps.lock().expect("lock").len();
    let success_count = successes.lock().expect("lock").len();

    assert!(step_count > 0, "reporter should receive step() calls");
    assert!(success_count > 0, "reporter should receive success() calls");
}
