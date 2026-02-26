//! Multipass CLI abstraction — enables test doubles for all `multipass` commands.

use std::process::Output;

use anyhow::{Context, Result};

/// VM name used by all multipass operations.
pub const VM_NAME: &str = "polis";

/// Parameters for `multipass launch`. Struct-based to avoid breaking
/// test doubles on future parameter additions.
pub struct LaunchParams<'a> {
    /// Ubuntu image to launch, e.g. `"24.04"` (not a `file://` URL).
    pub image: &'a str,
    /// Number of vCPUs, e.g. `"2"`.
    pub cpus: &'a str,
    /// Memory size, e.g. `"8G"`.
    pub memory: &'a str,
    /// Disk size, e.g. `"40G"`.
    pub disk: &'a str,
    /// Optional path to a cloud-init YAML file.
    /// When `Some`, `--cloud-init <path>` is appended to the launch command.
    /// When `None`, the flag is omitted entirely.
    pub cloud_init: Option<&'a str>,
    /// Launch timeout in seconds, e.g. `"900"`.
    /// Defaults to `"600"` when `None`.
    pub timeout: Option<&'a str>,
}

/// Abstraction over the multipass CLI, enabling test doubles.
///
/// All methods target the `polis` VM. The production implementation
/// delegates to the `multipass` binary via [`tokio::process::Command`].
#[allow(async_fn_in_trait)]
pub trait Multipass {
    /// Run `multipass info polis --format json`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn vm_info(&self) -> Result<Output>;

    /// Run `multipass launch` with the given VM parameters.
    ///
    /// Includes `--cloud-init <path>` when `params.cloud_init` is `Some`;
    /// omits the flag when it is `None`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn launch(&self, params: &LaunchParams<'_>) -> Result<Output>;

    /// Run `multipass start polis`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn start(&self) -> Result<Output>;

    /// Run `multipass stop polis`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn stop(&self) -> Result<Output>;

    /// Run `multipass delete polis`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn delete(&self) -> Result<Output>;

    /// Run `multipass purge`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn purge(&self) -> Result<Output>;

    /// Run `multipass transfer <local_path> polis:<remote_path>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn transfer(&self, local_path: &str, remote_path: &str) -> Result<Output>;

    /// Run `multipass transfer --recursive <local_path> polis:<remote_path>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn transfer_recursive(&self, local_path: &str, remote_path: &str) -> Result<Output>;

    /// Run `multipass exec polis -- <args>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn exec(&self, args: &[&str]) -> Result<Output>;

    /// Run `multipass exec polis -- <args>` with stdin piped from `input`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned or stdin write fails.
    async fn exec_with_stdin(&self, args: &[&str], input: &[u8]) -> Result<Output>;

    /// Spawn `multipass exec polis -- <args>` with piped stdin/stdout for STDIO bridging.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    fn exec_spawn(&self, args: &[&str]) -> Result<tokio::process::Child>;

    /// Run `multipass exec polis -- <args>` with inherited stdio.
    ///
    /// Stdin, stdout, and stderr are passed through transparently,
    /// enabling interactive use and real-time output streaming.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    async fn exec_status(&self, args: &[&str]) -> Result<std::process::ExitStatus>;

    /// Run `multipass version`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned (i.e. multipass not on PATH).
    async fn version(&self) -> Result<Output>;
}

/// Production implementation — shells out to the `multipass` binary.
pub struct MultipassCli;

impl Multipass for MultipassCli {
    async fn vm_info(&self) -> Result<Output> {
        tokio::process::Command::new("multipass")
            .args(["info", VM_NAME, "--format", "json"])
            .output()
            .await
            .context("failed to run multipass info")
    }

    async fn launch(&self, params: &LaunchParams<'_>) -> Result<Output> {
        let timeout = params.timeout.unwrap_or("600");
        let mut args = vec![
            "launch",
            params.image,
            "--name",
            VM_NAME,
            "--cpus",
            params.cpus,
            "--memory",
            params.memory,
            "--disk",
            params.disk,
            "--timeout",
            timeout,
        ];
        if let Some(path) = params.cloud_init {
            args.push("--cloud-init");
            args.push(path);
        }
        tokio::process::Command::new("multipass")
            .args(&args)
            .output()
            .await
            .context("failed to run multipass launch")
    }

    async fn start(&self) -> Result<Output> {
        tokio::process::Command::new("multipass")
            .args(["start", VM_NAME])
            .output()
            .await
            .context("failed to run multipass start")
    }

    async fn stop(&self) -> Result<Output> {
        tokio::process::Command::new("multipass")
            .args(["stop", VM_NAME])
            .output()
            .await
            .context("failed to run multipass stop")
    }

    async fn delete(&self) -> Result<Output> {
        tokio::process::Command::new("multipass")
            .args(["delete", VM_NAME])
            .output()
            .await
            .context("failed to run multipass delete")
    }

    async fn purge(&self) -> Result<Output> {
        tokio::process::Command::new("multipass")
            .arg("purge")
            .output()
            .await
            .context("failed to run multipass purge")
    }

    async fn transfer(&self, local_path: &str, remote_path: &str) -> Result<Output> {
        tokio::process::Command::new("multipass")
            .args(["transfer", local_path, &format!("{VM_NAME}:{remote_path}")])
            .output()
            .await
            .context("failed to run multipass transfer")
    }

    async fn transfer_recursive(&self, local_path: &str, remote_path: &str) -> Result<Output> {
        tokio::process::Command::new("multipass")
            .args([
                "transfer",
                "--recursive",
                local_path,
                &format!("{VM_NAME}:{remote_path}"),
            ])
            .output()
            .await
            .context("failed to run multipass transfer --recursive")
    }

    async fn exec(&self, args: &[&str]) -> Result<Output> {
        let mut cmd_args: Vec<&str> = vec!["exec", VM_NAME, "--"];
        cmd_args.extend_from_slice(args);
        tokio::process::Command::new("multipass")
            .args(&cmd_args)
            .output()
            .await
            .context("failed to run multipass exec")
    }

    async fn exec_with_stdin(&self, args: &[&str], input: &[u8]) -> Result<Output> {
        use tokio::io::AsyncWriteExt;

        let mut cmd_args: Vec<&str> = vec!["exec", VM_NAME, "--"];
        cmd_args.extend_from_slice(args);

        let mut child = tokio::process::Command::new("multipass")
            .args(&cmd_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("failed to spawn multipass exec")?;

        if let Some(mut stdin) = child.stdin.take() {
            let input = input.to_vec();
            tokio::spawn(async move {
                let _ = stdin.write_all(&input).await;
            });
        }

        child
            .wait_with_output()
            .await
            .context("failed to wait for multipass exec")
    }

    fn exec_spawn(&self, args: &[&str]) -> Result<tokio::process::Child> {
        let mut cmd_args: Vec<&str> = vec!["exec", VM_NAME, "--"];
        cmd_args.extend_from_slice(args);

        tokio::process::Command::new("multipass")
            .args(&cmd_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .context("failed to spawn multipass exec")
    }

    async fn version(&self) -> Result<Output> {
        tokio::process::Command::new("multipass")
            .arg("version")
            .output()
            .await
            .context("failed to run multipass version")
    }

    async fn exec_status(&self, args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut cmd_args: Vec<&str> = vec!["exec", VM_NAME, "--"];
        cmd_args.extend_from_slice(args);
        tokio::process::Command::new("multipass")
            .args(&cmd_args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .await
            .context("failed to run multipass exec")
    }
}
