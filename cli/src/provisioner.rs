use std::process::Output;

use anyhow::{Context, Result};

use crate::application::ports::{
    FileTransfer, InstanceInspector, InstanceLifecycle, InstanceSpec, InstanceState,
    POLIS_INSTANCE, ShellExecutor,
};
use crate::command_runner::{
    CommandRunner, DEFAULT_CMD_TIMEOUT, DEFAULT_EXEC_TIMEOUT, TokioCommandRunner,
};

/// Parsed VM instance information.
/// Returned by `InstanceInspector::info()` as a domain type
/// instead of raw `std::process::Output`.
///
/// Design decision: `info()` returns `Result<Output>` (not `InstanceInfo`)
/// in the initial implementation to minimize migration risk. Consumers
/// already parse the JSON themselves. A follow-up can introduce `InstanceInfo`
/// as a richer return type once the trait split is stable.
#[allow(dead_code)]
pub struct InstanceInfo {
    pub name: String,
    pub state: InstanceState,
    pub ipv4: Option<String>,
    pub image: String,
    pub cpus: u32,
    pub memory: String,
    pub disk: String,
}

/// Infrastructure adapter that routes all multipass CLI calls through a `CommandRunner`.
///
/// Generic over `R: CommandRunner` so that tests can inject a mock runner
/// without spawning real processes.
///
/// Two runners are held:
/// - `cmd_runner`: used for multipass subcommands (info, start, stop, …)
/// - `exec_runner`: used for `multipass exec` commands (may have a longer timeout)
pub struct MultipassProvisioner<R: CommandRunner> {
    cmd_runner: R,
    exec_runner: R,
}

impl<R: CommandRunner> MultipassProvisioner<R> {
    /// Create a new provisioner with explicit runner instances.
    #[allow(dead_code)]
    pub fn new(cmd_runner: R, exec_runner: R) -> Self {
        Self {
            cmd_runner,
            exec_runner,
        }
    }
}

impl MultipassProvisioner<TokioCommandRunner> {
    /// Convenience constructor for production use.
    /// Creates a `MultipassProvisioner` backed by `TokioCommandRunner` with default timeouts.
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
            .context("failed to run multipass launch")
    }

    async fn start(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["start", POLIS_INSTANCE])
            .await
            .context("failed to run multipass start")
    }

    async fn stop(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["stop", POLIS_INSTANCE])
            .await
            .context("failed to run multipass stop")
    }

    async fn delete(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["delete", POLIS_INSTANCE])
            .await
            .context("failed to run multipass delete")
    }

    async fn purge(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["purge"])
            .await
            .context("failed to run multipass purge")
    }
}

impl<R: CommandRunner> InstanceInspector for MultipassProvisioner<R> {
    async fn info(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["info", POLIS_INSTANCE, "--format", "json"])
            .await
            .context("failed to run multipass info")
    }

    async fn version(&self) -> Result<Output> {
        self.cmd_runner
            .run("multipass", &["version"])
            .await
            .context("failed to run multipass version")
    }
}

impl<R: CommandRunner> FileTransfer for MultipassProvisioner<R> {
    async fn transfer(&self, local_path: &str, remote_path: &str) -> Result<Output> {
        let dest = format!("{POLIS_INSTANCE}:{remote_path}");
        self.cmd_runner
            .run("multipass", &["transfer", local_path, &dest])
            .await
            .context("failed to run multipass transfer")
    }

    async fn transfer_recursive(&self, local_path: &str, remote_path: &str) -> Result<Output> {
        let dest = format!("{POLIS_INSTANCE}:{remote_path}");
        self.cmd_runner
            .run("multipass", &["transfer", "--recursive", local_path, &dest])
            .await
            .context("failed to run multipass transfer")
    }
}

impl<R: CommandRunner> ShellExecutor for MultipassProvisioner<R> {
    async fn exec(&self, args: &[&str]) -> Result<Output> {
        let mut full_args = vec!["exec", POLIS_INSTANCE, "--"];
        full_args.extend_from_slice(args);
        self.exec_runner
            .run("multipass", &full_args)
            .await
            .context("failed to run multipass exec")
    }

    async fn exec_with_stdin(&self, args: &[&str], input: &[u8]) -> Result<Output> {
        let mut full_args = vec!["exec", POLIS_INSTANCE, "--"];
        full_args.extend_from_slice(args);
        self.exec_runner
            .run_with_stdin("multipass", &full_args, input)
            .await
            .context("failed to run multipass exec")
    }

    fn exec_spawn(&self, args: &[&str]) -> Result<tokio::process::Child> {
        let mut full_args = vec!["exec", POLIS_INSTANCE, "--"];
        full_args.extend_from_slice(args);
        self.cmd_runner
            .spawn("multipass", &full_args)
            .context("failed to run multipass exec")
    }

    async fn exec_status(&self, args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut full_args = vec!["exec", POLIS_INSTANCE, "--"];
        full_args.extend_from_slice(args);
        self.cmd_runner
            .run_status("multipass", &full_args)
            .await
            .context("failed to run multipass exec")
    }
}
