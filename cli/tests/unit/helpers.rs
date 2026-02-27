//! Shared test helpers: mock Multipass implementations and output constructors.

#![allow(dead_code)]

use std::process::{ExitStatus, Output};

use anyhow::Result;
use polis_cli::multipass::Multipass;

// ── Cross-platform ExitStatus construction ───────────────────────────────────

/// Build an `ExitStatus` from a logical exit code (0 = success, non-zero = failure).
///
/// On Unix the raw wait-status encodes the exit code in bits 8–15, so we shift.
/// On Windows `ExitStatusExt::from_raw` takes the exit code directly.
#[cfg(unix)]
pub fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
pub fn exit_status(code: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    #[allow(clippy::cast_sign_loss)]
    ExitStatus::from_raw(code as u32)
}

// ── Output constructors ──────────────────────────────────────────────────────

pub fn ok_output(stdout: &[u8]) -> Output {
    Output {
        status: exit_status(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

pub fn err_output(code: i32, stderr: &[u8]) -> Output {
    Output {
        status: exit_status(code),
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
    async fn launch(&self, _: &polis_cli::multipass::LaunchParams<'_>) -> Result<Output> {
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
    async fn launch(&self, _: &polis_cli::multipass::LaunchParams<'_>) -> Result<Output> {
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
    async fn launch(&self, _: &polis_cli::multipass::LaunchParams<'_>) -> Result<Output> {
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
