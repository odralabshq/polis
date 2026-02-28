//! Infrastructure implementation of the `CommandRunner` port.
//!
//! `TokioCommandRunner` is the production implementation that uses tokio
//! for async process execution with guaranteed timeout and kill on all platforms.

use std::process::{Output, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::AsyncReadExt;

use crate::application::ports::CommandRunner;

/// Default timeout for multipass CLI commands (info, start, stop, etc.).
pub const DEFAULT_CMD_TIMEOUT: Duration = Duration::from_secs(30);

/// Default timeout for `multipass exec` commands (runs inside VM, may be slower).
pub const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(30);

/// Production `CommandRunner` — uses tokio for async process execution
/// with guaranteed timeout and kill on all platforms.
///
/// On Windows, `tokio::time::timeout` around `.output().await` does NOT kill
/// the child process when the timeout fires — the future is dropped but the
/// OS process keeps running. This implementation uses `tokio::select!` with
/// explicit `child.kill()` to guarantee the process is terminated.
pub struct TokioCommandRunner {
    timeout: Duration,
}

impl TokioCommandRunner {
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl CommandRunner for TokioCommandRunner {
    async fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
        self.run_with_timeout(program, args, self.timeout).await
    }

    async fn run_with_timeout(
        &self,
        program: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Result<Output> {
        let mut child = tokio::process::Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn {program}"))?;

        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();

        tokio::select! {
            result = async {
                let (status, stdout, stderr) = tokio::join!(
                    child.wait(),
                    async {
                        let mut buf = Vec::new();
                        if let Some(ref mut h) = stdout_handle {
                            let _ = h.read_to_end(&mut buf).await;
                        }
                        buf
                    },
                    async {
                        let mut buf = Vec::new();
                        if let Some(ref mut h) = stderr_handle {
                            let _ = h.read_to_end(&mut buf).await;
                        }
                        buf
                    },
                );
                Ok(Output {
                    status: status.with_context(|| format!("waiting for {program}"))?,
                    stdout,
                    stderr,
                })
            } => result,
            () = tokio::time::sleep(timeout) => {
                let _ = child.kill().await;
                anyhow::bail!("{program} timed out after {}s", timeout.as_secs())
            }
        }
    }

    async fn run_with_stdin(&self, program: &str, args: &[&str], input: &[u8]) -> Result<Output> {
        let mut child = tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn {program}"))?;

        let stdin_handle = child.stdin.take();
        let input_owned = input.to_vec();
        let stdin_task = tokio::spawn(async move {
            if let Some(mut stdin) = stdin_handle {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(&input_owned).await;
            }
        });

        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();

        tokio::select! {
            result = async {
                let (status, stdout, stderr) = tokio::join!(
                    child.wait(),
                    async {
                        let mut buf = Vec::new();
                        if let Some(ref mut h) = stdout_handle {
                            let _ = h.read_to_end(&mut buf).await;
                        }
                        buf
                    },
                    async {
                        let mut buf = Vec::new();
                        if let Some(ref mut h) = stderr_handle {
                            let _ = h.read_to_end(&mut buf).await;
                        }
                        buf
                    },
                );
                let _ = stdin_task.await;
                Ok(Output {
                    status: status.with_context(|| format!("waiting for {program}"))?,
                    stdout,
                    stderr,
                })
            } => result,
            () = tokio::time::sleep(self.timeout) => {
                let _ = child.kill().await;
                anyhow::bail!("{program} timed out after {}s", self.timeout.as_secs())
            }
        }
    }

    #[allow(dead_code)] // Reserved for future interactive command spawning
    fn spawn(&self, program: &str, args: &[&str]) -> Result<tokio::process::Child> {
        tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn {program}"))
    }

    async fn run_status(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut child = tokio::process::Command::new(program)
            .args(args)
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn {program}"))?;

        child
            .wait()
            .await
            .with_context(|| format!("waiting for {program}"))
    }
}


