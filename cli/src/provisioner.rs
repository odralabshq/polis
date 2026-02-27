use std::process::Output;

use anyhow::Result;

/// VM instance state — superset of the current `VmState` enum.
/// Adds `Stopping`, `Error`, and `NotFound` variants that the current
/// code handles via `Option` or error returns.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceState {
    Running,
    Stopped,
    Starting,
    Stopping,
    NotFound,
    Error,
}

/// Launch parameters for creating a new VM instance.
/// Replaces `LaunchParams` with a more domain-appropriate name.
pub struct InstanceSpec<'a> {
    /// Ubuntu image to launch, e.g. `"24.04"`.
    pub image: &'a str,
    /// Number of vCPUs, e.g. `"2"`.
    pub cpus: &'a str,
    /// Memory size, e.g. `"8G"`.
    pub memory: &'a str,
    /// Disk size, e.g. `"40G"`.
    pub disk: &'a str,
    /// Optional path to a cloud-init YAML file.
    pub cloud_init: Option<&'a str>,
    /// Launch timeout in seconds. Defaults to `"600"` when `None`.
    pub timeout: Option<&'a str>,
}

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

/// The canonical VM instance name used by all trait implementations.
pub const POLIS_INSTANCE: &str = "polis";

/// VM lifecycle operations: create, start, stop, destroy.
#[allow(async_fn_in_trait)]
pub trait InstanceLifecycle {
    /// Launch a new VM instance with the given spec.
    async fn launch(&self, spec: &InstanceSpec<'_>) -> Result<Output>;

    /// Start a stopped VM instance.
    async fn start(&self) -> Result<Output>;

    /// Stop a running VM instance.
    async fn stop(&self) -> Result<Output>;

    /// Delete the VM instance (can be recovered with `recover`).
    async fn delete(&self) -> Result<Output>;

    /// Permanently remove all deleted instances.
    async fn purge(&self) -> Result<Output>;
}

/// VM state inspection: query info and version.
#[allow(async_fn_in_trait)]
pub trait InstanceInspector {
    /// Get VM instance info as JSON.
    /// Renamed from `vm_info` — the "vm" prefix is redundant in a VM-focused trait.
    async fn info(&self) -> Result<Output>;

    /// Get the provisioner backend version.
    async fn version(&self) -> Result<Output>;
}

/// Host-to-VM file transfer operations.
#[allow(async_fn_in_trait)]
pub trait FileTransfer {
    /// Transfer a single file from host to VM.
    async fn transfer(&self, local_path: &str, remote_path: &str) -> Result<Output>;

    /// Recursively transfer a directory from host to VM.
    async fn transfer_recursive(&self, local_path: &str, remote_path: &str) -> Result<Output>;
}

/// Command execution inside the VM.
#[allow(async_fn_in_trait)]
pub trait ShellExecutor {
    /// Execute a command inside the VM and capture output.
    async fn exec(&self, args: &[&str]) -> Result<Output>;

    /// Execute a command inside the VM with stdin piped from `input`.
    async fn exec_with_stdin(&self, args: &[&str], input: &[u8]) -> Result<Output>;

    /// Spawn a command inside the VM with piped stdin/stdout for STDIO bridging.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails to spawn.
    #[allow(dead_code)]
    fn exec_spawn(&self, args: &[&str]) -> Result<tokio::process::Child>;

    /// Execute a command inside the VM with inherited stdio (interactive).
    async fn exec_status(&self, args: &[&str]) -> Result<std::process::ExitStatus>;
}

/// Composite trait combining all four sub-traits.
/// Used by consumers that need the full VM interface (e.g., `vm.rs`, `start.rs`).
pub trait VmProvisioner:
    InstanceLifecycle + InstanceInspector + FileTransfer + ShellExecutor
{
}

/// Blanket implementation: any type implementing all four sub-traits is a `VmProvisioner`.
impl<T> VmProvisioner for T where
    T: InstanceLifecycle + InstanceInspector + FileTransfer + ShellExecutor
{
}

use crate::command_runner::{
    CommandRunner, DEFAULT_CMD_TIMEOUT, DEFAULT_EXEC_TIMEOUT, TokioCommandRunner,
};
use anyhow::Context;

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
    async fn launch(&self, spec: &InstanceSpec<'_>) -> Result<std::process::Output> {
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

    async fn start(&self) -> Result<std::process::Output> {
        self.cmd_runner
            .run("multipass", &["start", POLIS_INSTANCE])
            .await
            .context("failed to run multipass start")
    }

    async fn stop(&self) -> Result<std::process::Output> {
        self.cmd_runner
            .run("multipass", &["stop", POLIS_INSTANCE])
            .await
            .context("failed to run multipass stop")
    }

    async fn delete(&self) -> Result<std::process::Output> {
        self.cmd_runner
            .run("multipass", &["delete", POLIS_INSTANCE])
            .await
            .context("failed to run multipass delete")
    }

    async fn purge(&self) -> Result<std::process::Output> {
        self.cmd_runner
            .run("multipass", &["purge"])
            .await
            .context("failed to run multipass purge")
    }
}

impl<R: CommandRunner> InstanceInspector for MultipassProvisioner<R> {
    async fn info(&self) -> Result<std::process::Output> {
        self.cmd_runner
            .run("multipass", &["info", POLIS_INSTANCE, "--format", "json"])
            .await
            .context("failed to run multipass info")
    }

    async fn version(&self) -> Result<std::process::Output> {
        self.cmd_runner
            .run("multipass", &["version"])
            .await
            .context("failed to run multipass version")
    }
}

impl<R: CommandRunner> FileTransfer for MultipassProvisioner<R> {
    async fn transfer(&self, local_path: &str, remote_path: &str) -> Result<std::process::Output> {
        let dest = format!("{POLIS_INSTANCE}:{remote_path}");
        self.cmd_runner
            .run("multipass", &["transfer", local_path, &dest])
            .await
            .context("failed to run multipass transfer")
    }

    async fn transfer_recursive(
        &self,
        local_path: &str,
        remote_path: &str,
    ) -> Result<std::process::Output> {
        let dest = format!("{POLIS_INSTANCE}:{remote_path}");
        self.cmd_runner
            .run("multipass", &["transfer", "--recursive", local_path, &dest])
            .await
            .context("failed to run multipass transfer")
    }
}

impl<R: CommandRunner> ShellExecutor for MultipassProvisioner<R> {
    async fn exec(&self, args: &[&str]) -> Result<std::process::Output> {
        let mut full_args = vec!["exec", POLIS_INSTANCE, "--"];
        full_args.extend_from_slice(args);
        self.exec_runner
            .run("multipass", &full_args)
            .await
            .context("failed to run multipass exec")
    }

    async fn exec_with_stdin(&self, args: &[&str], input: &[u8]) -> Result<std::process::Output> {
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
