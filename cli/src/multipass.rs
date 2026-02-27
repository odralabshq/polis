//! Multipass CLI standalone helpers — timeout-safe wrappers and IP resolution.

use std::process::Output;

use anyhow::{Context, Result};

use crate::command_runner::{CommandRunner, TokioCommandRunner};
use crate::provisioner::InstanceInspector;

/// VM name used by all multipass operations.
pub const VM_NAME: &str = "polis";

/// Run `multipass exec polis -- <args>` with a hard timeout that kills the
/// child process if it doesn't complete in time.
///
/// On Windows, `tokio::time::timeout` around `.output().await` does NOT kill
/// the child process when the timeout fires — the future is dropped but the
/// OS process keeps running, causing the await to never resolve. This helper
/// uses `tokio::select!` with explicit `child.kill()` to guarantee the process
/// is terminated on both platforms.
///
/// Returns `Ok(Output)` on success, or `Err` on spawn failure / timeout.
/// # Errors
///
/// Returns an error if the command fails to spawn, or if it exceeds the
/// specified timeout.
pub async fn exec_with_timeout(args: &[&str], timeout: std::time::Duration) -> Result<Output> {
    let mut cmd_args: Vec<&str> = vec!["exec", VM_NAME, "--"];
    cmd_args.extend_from_slice(args);
    TokioCommandRunner::new(timeout)
        .run("multipass", &cmd_args)
        .await
        .context("failed to run multipass exec")
}

/// Run a non-exec multipass subcommand with a timeout.
///
/// Builds `["subcommand", ...args]` and delegates to [`TokioCommandRunner`].
///
/// # Errors
///
/// Returns an error if the command fails to spawn or exceeds the timeout.
pub async fn cmd_with_timeout(
    subcommand: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<Output> {
    let mut cmd_args: Vec<&str> = vec![subcommand];
    cmd_args.extend_from_slice(args);
    TokioCommandRunner::new(timeout)
        .run("multipass", &cmd_args)
        .await
        .with_context(|| format!("failed to run multipass {subcommand}"))
}

/// Extracts the primary IPv4 address of the `polis` VM from `multipass info`.
///
/// The JSON structure is `{ "info": { "polis": { "ipv4": ["172.x.x.x", ...] } } }`.
/// Returns the first address, which is the primary interface on the host-VM bridge.
///
/// # Errors
///
/// Returns an error if `multipass info` fails or no IPv4 address is found.
pub async fn resolve_vm_ip(mp: &impl InstanceInspector) -> Result<String> {
    let output = mp.info().await.context("failed to query VM info")?;
    anyhow::ensure!(output.status.success(), "multipass info failed");

    let info: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("invalid JSON from multipass info")?;

    info.get("info")
        .and_then(|i| i.get(VM_NAME))
        .and_then(|p| p.get("ipv4"))
        .and_then(|arr| arr.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("no IPv4 address found for polis VM"))
}
