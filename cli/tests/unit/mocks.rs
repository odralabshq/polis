//! Shared mock infrastructure for unit tests.
//!
//! Provides canned sub-trait implementations and output helpers so each
//! test file doesn't have to re-define the same boilerplate.

#![allow(clippy::expect_used)]

use std::process::Output;

use anyhow::Result;
use polis_cli::application::ports::{
    FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec, ShellExecutor,
};

use crate::helpers::exit_status;

// ── Output helpers ────────────────────────────────────────────────────────────

#[must_use]
pub fn ok_output(stdout: &[u8]) -> Output {
    Output {
        status: exit_status(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

#[must_use]
pub fn err_output(stderr: &[u8]) -> Output {
    Output {
        status: exit_status(1),
        stdout: Vec::new(),
        stderr: stderr.to_vec(),
    }
}

fn unexpected<T>() -> Result<T> {
    anyhow::bail!("not expected in this test")
}

// ── Mock: VM not found ────────────────────────────────────────────────────────

pub struct MultipassVmNotFound;

impl InstanceInspector for MultipassVmNotFound {
    async fn info(&self) -> Result<Output> {
        Ok(err_output(b"instance \"polis\" does not exist"))
    }
    async fn version(&self) -> Result<Output> {
        unexpected()
    }
}

impl InstanceLifecycle for MultipassVmNotFound {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        unexpected()
    }
    async fn start(&self) -> Result<Output> {
        unexpected()
    }
    async fn stop(&self) -> Result<Output> {
        unexpected()
    }
    async fn delete(&self) -> Result<Output> {
        unexpected()
    }
    async fn purge(&self) -> Result<Output> {
        unexpected()
    }
}

impl FileTransfer for MultipassVmNotFound {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
}

impl ShellExecutor for MultipassVmNotFound {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(err_output(b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        unexpected()
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        unexpected()
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        unexpected()
    }
}

// ── Mock: VM stopped ─────────────────────────────────────────────────────────

pub struct MultipassVmStopped;

impl InstanceInspector for MultipassVmStopped {
    async fn info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Stopped"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        unexpected()
    }
}

impl InstanceLifecycle for MultipassVmStopped {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        unexpected()
    }
    async fn start(&self) -> Result<Output> {
        unexpected()
    }
    async fn stop(&self) -> Result<Output> {
        unexpected()
    }
    async fn delete(&self) -> Result<Output> {
        unexpected()
    }
    async fn purge(&self) -> Result<Output> {
        unexpected()
    }
}

impl FileTransfer for MultipassVmStopped {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
}

impl ShellExecutor for MultipassVmStopped {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(err_output(b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        unexpected()
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        unexpected()
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        unexpected()
    }
}

// ── Mock: exec call recorder ──────────────────────────────────────────────────

/// Records every `exec()` call as a `Vec<String>` (one entry per argument).
/// All exec calls succeed (exit 0). Other methods bail if called.
pub struct MultipassExecRecorder {
    pub calls: std::sync::Mutex<Vec<Vec<String>>>,
}

impl MultipassExecRecorder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Return a snapshot of all recorded exec calls.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn recorded_calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().expect("lock").clone()
    }
}

impl Default for MultipassExecRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceInspector for MultipassExecRecorder {
    async fn info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("not expected in this test")
    }
}

impl InstanceLifecycle for MultipassExecRecorder {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        anyhow::bail!("launch not expected in this test")
    }
    async fn start(&self) -> Result<Output> {
        anyhow::bail!("start not expected in this test")
    }
    async fn stop(&self) -> Result<Output> {
        anyhow::bail!("stop not expected in this test")
    }
    async fn delete(&self) -> Result<Output> {
        anyhow::bail!("delete not expected in this test")
    }
    async fn purge(&self) -> Result<Output> {
        anyhow::bail!("purge not expected in this test")
    }
}

impl FileTransfer for MultipassExecRecorder {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer not expected in this test")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer_recursive not expected in this test")
    }
}

impl ShellExecutor for MultipassExecRecorder {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        self.calls
            .lock()
            .expect("lock")
            .push(args.iter().map(std::string::ToString::to_string).collect());
        Ok(ok_output(b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("exec_with_stdin not expected in this test")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not expected in this test")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        anyhow::bail!("exec_status not expected in this test")
    }
}

// ── Mock: VM running ─────────────────────────────────────────────────────────

pub struct MultipassVmRunning;

impl InstanceInspector for MultipassVmRunning {
    async fn info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn version(&self) -> Result<Output> {
        unexpected()
    }
}

impl InstanceLifecycle for MultipassVmRunning {
    async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
        unexpected()
    }
    async fn start(&self) -> Result<Output> {
        unexpected()
    }
    async fn stop(&self) -> Result<Output> {
        unexpected()
    }
    async fn delete(&self) -> Result<Output> {
        unexpected()
    }
    async fn purge(&self) -> Result<Output> {
        unexpected()
    }
}

impl FileTransfer for MultipassVmRunning {
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
}

impl ShellExecutor for MultipassVmRunning {
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        unexpected()
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        unexpected()
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        unexpected()
    }
}
