//! Shared mock infrastructure for unit tests.
//!
//! Provides canned [`Multipass`] implementations and output helpers so each
//! test file doesn't have to re-define the same boilerplate.

#![allow(clippy::expect_used)]

use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::multipass::Multipass;

// ── Output helpers ────────────────────────────────────────────────────────────

pub fn ok_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

pub fn err_output(stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(1 << 8),
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
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
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
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
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

// ── Mock: VM running ─────────────────────────────────────────────────────────

pub struct MultipassVmRunning;

impl Multipass for MultipassVmRunning {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
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
