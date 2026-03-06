//! `polis delete [--all]` — remove workspace.

use anyhow::Result;

use crate::app::AppContext;
use crate::application::ports::ProgressReporter as _;
use crate::application::services::workspace_delete::{self, DeleteOutcome};
use crate::commands::DeleteArgs;

/// Run `polis delete [--all]`.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub async fn run(args: &DeleteArgs, app: &AppContext) -> Result<std::process::ExitCode> {
    let confirmed = if args.all {
        confirm_delete_all(args, app)?
    } else {
        confirm_delete_workspace(args, app)?
    };

    if !confirmed {
        app.output.info("Cancelled.");
        return Ok(std::process::ExitCode::SUCCESS);
    }

    if let Err(e) = execute_delete(args.all, app).await {
        app.output.error(&e.to_string());
        return Ok(std::process::ExitCode::FAILURE);
    }

    Ok(std::process::ExitCode::SUCCESS)
}

fn confirm_delete_all(args: &DeleteArgs, app: &AppContext) -> Result<bool> {
    Ok(args.yes || app.confirm("Remove all data?", false)?)
}

fn confirm_delete_workspace(args: &DeleteArgs, app: &AppContext) -> Result<bool> {
    if !app.output.quiet {
        app.output.info("");
        app.output.info("This will remove your workspace.");
        app.output
            .info("Configuration, certificates, and cached downloads will be preserved.");
        app.output.info("");
    }
    Ok(args.yes || app.confirm("Continue?", false)?)
}

async fn execute_delete(all: bool, app: &AppContext) -> Result<()> {
    let reporter = app.terminal_reporter();
    if all {
        let ctx = workspace_delete::CleanupContext {
            provisioner: &app.provisioner,
            state_store: &app.state_mgr,
            local_fs: &app.local_fs,
            paths: &app.local_fs,
            ssh: &app.ssh,
            reporter: &reporter,
        };
        workspace_delete::delete_all(&ctx).await
    } else {
        match workspace_delete::delete_workspace(&app.provisioner, &app.state_mgr, &reporter).await? {
            DeleteOutcome::NotFound => {
                reporter.success("no workspace to delete");
            }
            DeleteOutcome::Deleted => {
                reporter.success("workspace removed");
            }
        }
        Ok(())
    }
}
