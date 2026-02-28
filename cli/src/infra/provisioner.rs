//! Infrastructure implementation of VM provisioner port traits.
//!
//! `MultipassProvisioner<R>` routes all multipass CLI calls through a
//! `CommandRunner`. `TimeoutView<'a, R>` provides scoped timeout overrides
//! for inspection and shell execution operations.

#![allow(dead_code)] // Refactor in progress — defined ahead of callers

use std::process::Output;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::application::ports::{
    FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec, POLIS_INSTANCE, ShellExecutor,
};
use crate::command_runner::{
    CommandRunner, DEFAULT_CMD_TIMEOUT, DEFAULT_EXEC_TIMEOUT, TokioCommandRunner,
};

/// Infrastructure adapter that routes all multipass CLI calls through a `CommandRunner`.
///
/// Generic over `R: CommandRunner` so that tests can inject a mock runner
/// without spawning real processes.
pub struct MultipassProvisioner<R: CommandRunner> {
    cmd_runner: R,
    exec_runner: R,
}

impl<R: CommandRunner> MultipassProvisioner<R> {
    /// Create a new provisioner with explicit runner instances.
    pub fn new(cmd_runner: R, exec_runner: R) -> Self {
        Self {
            cmd_runner,
            exec_runner,
        }
    }

    /// Create a `TimeoutView` that overrides the command timeout for the
    /// duration of the returned view's lifetime.
    ///
    /// `TimeoutView` intentionally implements only `InstanceInspector` and
    /// `ShellExecutor` — not `InstanceLifecycle` or `FileTransfer`. Lifecycle
    /// and transfer operations must not be subject to short timeouts because
    /// they involve long-running VM operations (launch, transfer large files).
    pub fn with_cmd_timeout(&self, timeout: Duration) -> TimeoutView<'_, R> {
        TimeoutView {
            provisioner: self,
            timeout,
        }
    }
}

impl MultipassProvisioner<TokioCommandRunner> {
    /// Convenience constructor for production use.
    #[must_use]
    pub fn default_runner() -> Self {
        Self {
            cmd_runner: TokioCommandRunner::new(DEFAULT_CMD_TIMEOUT),
            exec_runner: TokioCommandRunner::new(DEFAULT_EXEC_TIMEOUT),
        }
    }
}

impl<R: CommandRunner> InstanceLifecycle for MultipassProvisioner<R> {
    async fn launch(&self, spec: &InstanceSpec<'_>) -> Result<Output> {
        let timeout = spec.timeout.unwrap_or("600");
        let mut args = vec![
            "launch",
            spec.image,
            "--name",
            POLIS_INSTANCE,
            "--cpus",
            spec.cpus,
            "--memory",
            spec.memory,
            "--disk",
            spec.disk,
            "--timeout",
            timeout,
        ];
        if let Some(path) = spec.cloud_init {
            args.push("--cloud-init");
            args.push(path);
        }
        self.cmd_runner
            .run("multipass", &args)
            .await
            .context("multipass launch")
    }

    async fn start(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["start", POLIS_INSTANCE])
            .await
            .context("multipass start")
    }

    async fn stop(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["stop", POLIS_INSTANCE])
            .await
            .context("multipass stop")
    }

    async fn delete(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["delete", POLIS_INSTANCE])
            .await
            .context("multipass delete")
    }

    async fn purge(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["purge"])
            .await
            .context("multipass purge")
    }
}

impl<R: CommandRunner> InstanceInspector for MultipassProvisioner<R> {
    async fn info(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["info", POLIS_INSTANCE, "--format", "json"])
            .await
            .context("multipass info")
    }

    async fn version(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["version"])
            .await
            .context("multipass version")
    }
}

impl<R: CommandRunner> FileTransfer for MultipassProvisioner<R> {
    async fn transfer(&self, local: &str, remote: &str) -> Result<Output> {
        let dest = format!("{POLIS_INSTANCE}:{remote}");
        self.cmd_runner
            .run("multipass", &["transfer", local, &dest])
            .await
            .context("multipass transfer")
    }

    async fn transfer_recursive(&self, local: &str, remote: &str) -> Result<Output> {
        let dest = format!("{POLIS_INSTANCE}:{remote}");
        self.cmd_runner
            .run("multipass", &["transfer", "--recursive", local, &dest])
            .await
            .context("multipass transfer --recursive")
    }
}

impl<R: CommandRunner> ShellExecutor for MultipassProvisioner<R> {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        let mut full = vec!["exec", POLIS_INSTANCE, "--"];
        full.extend_from_slice(args);
        self.exec_runner
            .run("multipass", &full)
            .await
            .context("multipass exec")
    }

    async fn exec_with_stdin(&self, args: &[&str], input: &[u8]) -> Result<Output> {
        let mut full = vec!["exec", POLIS_INSTANCE, "--"];
        full.extend_from_slice(args);
        self.exec_runner
            .run_with_stdin("multipass", &full, input)
            .await
            .context("multipass exec")
    }

    fn exec_spawn(&self, args: &[&str]) -> Result<tokio::process::Child> {
        let mut full = vec!["exec", POLIS_INSTANCE, "--"];
        full.extend_from_slice(args);
        self.cmd_runner
            .spawn("multipass", &full)
            .context("multipass exec spawn")
    }

    async fn exec_status(&self, args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut full = vec!["exec", POLIS_INSTANCE, "--"];
        full.extend_from_slice(args);
        self.cmd_runner
            .run_status("multipass", &full)
            .await
            .context("multipass exec status")
    }
}

// ── TimeoutView ───────────────────────────────────────────────────────────────

/// A scoped view of a `MultipassProvisioner` with a custom command timeout.
///
/// Intentionally implements only `InstanceInspector` and `ShellExecutor`.
/// Lifecycle and transfer operations are excluded because they involve
/// long-running VM operations that must not be subject to short timeouts.
pub struct TimeoutView<'a, R: CommandRunner> {
    provisioner: &'a MultipassProvisioner<R>,
    timeout: Duration,
}

impl<R: CommandRunner> InstanceInspector for TimeoutView<'_, R> {
    async fn info(&self) -> Result<Output> {
        self.provisioner
            .cmd_runner
            .run_with_timeout(
                "multipass",
                &["info", POLIS_INSTANCE, "--format", "json"],
                self.timeout,
            )
            .await
            .context("multipass info (timeout view)")
    }

    async fn version(&self) -> Result<Output> {
        self.provisioner
            .cmd_runner
            .run_with_timeout("multipass", &["version"], self.timeout)
            .await
            .context("multipass version (timeout view)")
    }
}

impl<R: CommandRunner> ShellExecutor for TimeoutView<'_, R> {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        let mut full = vec!["exec", POLIS_INSTANCE, "--"];
        full.extend_from_slice(args);
        self.provisioner
            .exec_runner
            .run_with_timeout("multipass", &full, self.timeout)
            .await
            .context("multipass exec (timeout view)")
    }

    async fn exec_with_stdin(&self, args: &[&str], input: &[u8]) -> Result<Output> {
        let mut full = vec!["exec", POLIS_INSTANCE, "--"];
        full.extend_from_slice(args);
        self.provisioner
            .exec_runner
            .run_with_stdin("multipass", &full, input)
            .await
            .context("multipass exec with stdin (timeout view)")
    }

    fn exec_spawn(&self, args: &[&str]) -> Result<tokio::process::Child> {
        let mut full = vec!["exec", POLIS_INSTANCE, "--"];
        full.extend_from_slice(args);
        self.provisioner
            .cmd_runner
            .spawn("multipass", &full)
            .context("multipass exec spawn")
    }

    async fn exec_status(&self, args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut full = vec!["exec", POLIS_INSTANCE, "--"];
        full.extend_from_slice(args);
        self.provisioner
            .cmd_runner
            .run_status("multipass", &full)
            .await
            .context("multipass exec status (timeout view)")
    }
}
