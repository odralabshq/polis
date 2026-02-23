//! Shared test helpers: mock Multipass implementations and output constructors.

#![allow(dead_code)]

use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::multipass::Multipass;

// ── Output constructors ──────────────────────────────────────────────────────

pub fn ok_output(stdout: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

pub fn err_output(code: i32, stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: Vec::new(),
        stderr: stderr.to_vec(),
    }
}

// ── Shared mock implementations ──────────────────────────────────────────────

/// VM does not exist (multipass info exits 1).
pub struct VmNotFound;

impl Multipass for VmNotFound {
    async fn vm_info(&self) -> Result<Output> {
        Ok(err_output(1, b"instance \"polis\" does not exist"))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("launch not expected in this test")
    }
    async fn start(&self) -> Result<Output> {
        anyhow::bail!("start not expected in this test")
    }
    async fn stop(&self) -> Result<Output> {
        anyhow::bail!("stop not expected in this test")
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer not expected in this test")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer_recursive not expected in this test")
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(err_output(1, b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("exec_with_stdin not expected in this test")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not expected in this test")
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("version not expected in this test")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        anyhow::bail!("exec_status not expected in this test")
    }
}

/// VM exists and is stopped.
pub struct VmStopped;

impl Multipass for VmStopped {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Stopped"}}}"#))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("launch not expected in this test")
    }
    async fn start(&self) -> Result<Output> {
        anyhow::bail!("start not expected in this test")
    }
    async fn stop(&self) -> Result<Output> {
        anyhow::bail!("stop not expected in this test")
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer not expected in this test")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer_recursive not expected in this test")
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(err_output(1, b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("exec_with_stdin not expected in this test")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not expected in this test")
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("version not expected in this test")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        anyhow::bail!("exec_status not expected in this test")
    }
}

/// VM exists and is running.
pub struct VmRunning;

impl Multipass for VmRunning {
    async fn vm_info(&self) -> Result<Output> {
        Ok(ok_output(br#"{"info":{"polis":{"state":"Running"}}}"#))
    }
    async fn launch(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("launch not expected in this test")
    }
    async fn start(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn stop(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn delete(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn purge(&self) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer not expected in this test")
    }
    async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
        anyhow::bail!("transfer_recursive not expected in this test")
    }
    async fn exec(&self, _: &[&str]) -> Result<Output> {
        Ok(ok_output(b""))
    }
    async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
        anyhow::bail!("exec_with_stdin not expected in this test")
    }
    fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
        anyhow::bail!("exec_spawn not expected in this test")
    }
    async fn version(&self) -> Result<Output> {
        anyhow::bail!("version not expected in this test")
    }
    async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
        anyhow::bail!("exec_status not expected in this test")
    }
}
