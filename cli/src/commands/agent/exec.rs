//! `polis agent exec` — run agent-specific commands via `commands.sh`.

use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::app::App;
use crate::application::ports::ShellExecutor;
use crate::application::vm::lifecycle::{self as vm, VmState};
use crate::domain::error::WorkspaceError;
use crate::domain::process::exit_code_from_status;
use crate::domain::workspace::VM_ROOT;

/// Run an agent-specific command.
///
/// Reads the agent manifest to locate `spec.commands`, then executes
/// `bash <commands.sh> <container> <subcmd> [args...]` inside the VM.
///
/// # Errors
///
/// Returns an error if the workspace is not running, the agent has no
/// commands script, or the command fails.
pub async fn run(app: &impl App, name: &str, subcmd: &str, args: &[String]) -> Result<ExitCode> {
    let mp = app.provisioner();

    if !crate::domain::agent::validate::AGENT_NAME_RE.is_match(name) {
        anyhow::bail!("Invalid agent name format");
    }

    // Ensure workspace is running.
    let state = vm::state(mp).await?;
    if state != VmState::Running {
        return Err(WorkspaceError::NotRunning.into());
    }

    // Read the agent manifest to find the commands script.
    let manifest_path = format!("{VM_ROOT}/agents/{name}/agent.yaml");
    let cat = mp
        .exec(&["cat", &manifest_path])
        .await
        .context("reading agent manifest")?;
    if !cat.status.success() {
        bail!("Agent '{name}' not found or manifest unreadable");
    }

    let manifest: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(&cat.stdout).context("parsing agent manifest")?;

    let commands_script = manifest
        .get("spec")
        .and_then(|s| s.get("commands"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    if commands_script.is_empty() {
        bail!("Agent '{name}' does not define any commands.");
    }

    // Build the full path to the commands script.
    let script_path = format!("{VM_ROOT}/agents/{name}/{commands_script}");

    // The container name follows the convention: polis-workspace
    // (agents run inside the workspace container as systemd services).
    let container = "polis-workspace";

    // Build the command: bash <script> <container> <subcmd> [args...]
    let mut cmd_args: Vec<String> = vec![
        "bash".to_string(),
        script_path,
        container.to_string(),
        subcmd.to_string(),
    ];
    cmd_args.extend(args.iter().cloned());

    let cmd_refs: Vec<&str> = cmd_args.iter().map(String::as_str).collect();

    let status = mp.exec_status(&cmd_refs).await?;

    Ok(exit_code_from_status(status))
}
