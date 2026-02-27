use std::process::{Output, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::AsyncReadExt;

/// Default timeout for multipass CLI commands (info, start, stop, etc.).
pub const DEFAULT_CMD_TIMEOUT: Duration = Duration::from_secs(30);

/// Default timeout for `multipass exec` commands (runs inside VM, may be slower).
pub const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(30);

/// Generic command execution with timeout and guaranteed process kill.
///
/// This trait is NOT tied to Multipass — it can run any external command.
/// The production implementation uses tokio; test doubles can return
/// canned results without spawning processes.
#[allow(async_fn_in_trait)]
pub trait CommandRunner {
    /// Run a command with the default timeout.
    async fn run(&self, program: &str, args: &[&str]) -> Result<Output>;

    /// Run a command with a custom timeout (overrides default).
    async fn run_with_timeout(
        &self,
        program: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Result<Output>;

    /// Run a command with stdin piped from `input`.
    async fn run_with_stdin(&self, program: &str, args: &[&str], input: &[u8]) -> Result<Output>;

    /// Spawn a command and return the child handle.
    /// No timeout — caller manages the child lifetime.
    /// `kill_on_drop(true)` is set as a safety net.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails to spawn.
    #[allow(dead_code)]
    fn spawn(&self, program: &str, args: &[&str]) -> Result<tokio::process::Child>;

    /// Run a command with inherited stdio (interactive pass-through).
    /// No timeout — used for commands like `cloud-init status --wait`.
    async fn run_status(&self, program: &str, args: &[&str]) -> Result<std::process::ExitStatus>;
}

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

        // Read stdout/stderr CONCURRENTLY with wait() to avoid pipe deadlock.
        // If the child writes more than the OS pipe buffer (64KB Linux, 4KB
        // some Windows configs), it blocks on write. If we only call
        // child.wait() first, wait() never resolves → deadlock.
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

        // Write stdin in a spawned task to avoid deadlock with stdout/stderr reads
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

        // Read stdout/stderr CONCURRENTLY with wait() to avoid pipe deadlock
        // (same rationale as run_with_timeout — see comment there).
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
