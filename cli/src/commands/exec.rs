//! `polis exec` — run a command inside the workspace container.

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args;

use crate::app::AppContext;
use crate::application::services::workspace;
use crate::domain::process::exit_code_from_status;

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
/// Returns an error if the workspace is not running or the command cannot be spawned.
pub async fn run(app: &AppContext, args: &ExecArgs) -> Result<ExitCode> {
    let interactive = std::io::stdin().is_terminal();

    let cmd_args: Vec<&str> = args.command.iter().map(String::as_str).collect();

    let status = workspace::exec(app.provisioner(), &cmd_args, interactive).await?;

    Ok(exit_code_from_status(status))
}
