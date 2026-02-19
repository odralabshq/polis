//! Multipass CLI abstraction — enables test doubles for all `multipass` commands.

use std::process::{Command, Output};

use anyhow::{Context, Result};

/// VM name used by all multipass operations.
pub const VM_NAME: &str = "polis";

/// Abstraction over the multipass CLI, enabling test doubles.
///
/// All methods target the `polis` VM. The production implementation
/// delegates to the `multipass` binary via [`std::process::Command`].
pub trait Multipass {
    /// Run `multipass info polis --format json`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    fn vm_info(&self) -> Result<Output>;

    /// Run `multipass launch` with the given VM parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    fn launch(&self, image_url: &str, cpus: &str, memory: &str, disk: &str) -> Result<Output>;

    /// Run `multipass start polis`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    fn start(&self) -> Result<Output>;

    /// Run `multipass transfer <local_path> polis:<remote_path>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    fn transfer(&self, local_path: &str, remote_path: &str) -> Result<Output>;

    /// Run `multipass exec polis -- <args>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be spawned.
    fn exec(&self, args: &[&str]) -> Result<Output>;
}

/// Production implementation — shells out to the `multipass` binary.
pub struct MultipassCli;

impl Multipass for MultipassCli {
    fn vm_info(&self) -> Result<Output> {
        Command::new("multipass")
            .args(["info", VM_NAME, "--format", "json"])
            .output()
            .context("failed to run multipass info")
    }

    fn launch(&self, image_url: &str, cpus: &str, memory: &str, disk: &str) -> Result<Output> {
        Command::new("multipass")
            .args([
                "launch", image_url, "--name", VM_NAME, "--cpus", cpus, "--memory", memory,
                "--disk", disk,
            ])
            .output()
            .context("failed to run multipass launch")
    }

    fn start(&self) -> Result<Output> {
        Command::new("multipass")
            .args(["start", VM_NAME])
            .output()
            .context("failed to run multipass start")
    }

    fn transfer(&self, local_path: &str, remote_path: &str) -> Result<Output> {
        Command::new("multipass")
            .args(["transfer", local_path, &format!("{VM_NAME}:{remote_path}")])
            .output()
            .context("failed to run multipass transfer")
    }

    fn exec(&self, args: &[&str]) -> Result<Output> {
        let mut cmd_args: Vec<&str> = vec!["exec", VM_NAME, "--"];
        cmd_args.extend_from_slice(args);
        Command::new("multipass")
            .args(&cmd_args)
            .output()
            .context("failed to run multipass exec")
    }
}
