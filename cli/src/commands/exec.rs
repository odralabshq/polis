//! `polis exec` — run a command inside the workspace container.

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args;

use crate::application::ports::ShellExecutor;
use crate::domain::workspace::CONTAINER_NAME;

/// Arguments for the exec command.
#[derive(Args)]
#[command(trailing_var_arg = true)]
pub struct ExecArgs {
    /// Command and arguments to run in the workspace
    #[arg(required = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// Run a command inside the workspace container.
///
/// Passes stdin, stdout, and stderr through transparently. When stdin is a
/// terminal, allocates a TTY in the container (`docker exec -it`).
///
/// # Errors
///
/// Returns an error if the command cannot be spawned.
pub async fn run(args: &ExecArgs, mp: &impl ShellExecutor) -> Result<ExitCode> {
    let interactive = std::io::stdin().is_terminal();

    let mut docker_args: Vec<&str> = vec!["docker", "exec"];
    if interactive {
        docker_args.push("-it");
    }
    docker_args.push(CONTAINER_NAME);
    docker_args.extend(args.command.iter().map(String::as_str));

    let status = mp
        .exec_status(&docker_args)
        .await
        .context("failed to exec in workspace")?;

    let code = status.code().unwrap_or(1);
    #[allow(clippy::cast_possible_truncation)]
    Ok(ExitCode::from(code as u8))
}
