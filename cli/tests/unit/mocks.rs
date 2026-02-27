//! Shared mock infrastructure for unit tests.
//!
//! Provides canned [`Multipass`] implementations and output helpers so each
//! test file doesn't have to re-define the same boilerplate.

#![allow(clippy::expect_used)]

use std::process::Output;

use anyhow::Result;
use polis_cli::multipass::Multipass;

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

impl Multipass for MultipassVmNotFound {
    async fn vm_info(&self) -> Result<Output> {
        Ok(err_output(b"instance \"polis\" does not exist"))
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(err_output(b""))
    }
    async fn launch(&self, _: &polis_cli::multipass::LaunchParams<'_>) -> Result<Output> {
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
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        unexpected()
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        unexpected()
    }
    async fn version(&self) -> Result<Output> {
        unexpected()
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        unexpected()
    }
}

// ── Mock: VM stopped ─────────────────────────────────────────────────────────

pub struct MultipassVmStopped;

impl Multipass for MultipassVmStopped {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Stopped"}}}"#))
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(err_output(b""))
    }
    async fn launch(&self, _: &polis_cli::multipass::LaunchParams<'_>) -> Result<Output> {
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
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        unexpected()
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        unexpected()
    }
    async fn version(&self) -> Result<Output> {
        unexpected()
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        unexpected()
    }
}

// ── Mock: exec call recorder ──────────────────────────────────────────────────

/// Records every `exec()` call as a `Vec<String>` (one entry per argument).
/// All exec calls succeed (exit 0). Other methods panic if called.
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

impl Multipass for MultipassExecRecorder {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        self.calls
            .lock()
            .expect("lock")
            .push(args.iter().map(std::string::ToString::to_string).collect());
        Ok(ok_output(b""))
    }
    async fn launch(&self, _: &polis_cli::multipass::LaunchParams<'_>) -> Result<Output> {
        panic!("launch not expected in this test")
    }
    async fn start(&self) -> Result<Output> {
        panic!("start not expected in this test")
    }
    async fn stop(&self) -> Result<Output> {
        panic!("stop not expected in this test")
    }
    async fn delete(&self) -> Result<Output> {
        panic!("delete not expected in this test")
    }
    async fn purge(&self) -> Result<Output> {
        panic!("purge not expected in this test")
    }
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        panic!("transfer not expected in this test")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        panic!("transfer_recursive not expected in this test")
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        panic!("exec_with_stdin not expected in this test")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        panic!("exec_spawn not expected in this test")
    }
    async fn version(&self) -> Result<Output> {
        panic!("version not expected in this test")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        panic!("exec_status not expected in this test")
    }
}

// ── Mock: VM running ─────────────────────────────────────────────────────────

pub struct MultipassVmRunning;

impl Multipass for MultipassVmRunning {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn launch(&self, _: &polis_cli::multipass::LaunchParams<'_>) -> Result<Output> {
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
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        unexpected()
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        unexpected()
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        unexpected()
    }
    async fn version(&self) -> Result<Output> {
        unexpected()
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        unexpected()
    }
}
