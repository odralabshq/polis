//! `polis delete [--all]` — remove workspace.

use anyhow::Result;

use crate::app::AppContext;
use crate::application::services::cleanup_service;
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

    if let Err(e) = execute_delete(args.all, args.no_backup, app).await {
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
        if args.no_backup {
            app.output
                .warn("Backup is disabled — workspace data will be permanently lost.");
        } else {
            app.output
                .info("Workspace data will be backed up to ~/.polis/backups/ before removal.");
        }
        app.output
            .info("Configuration, certificates, and cached downloads will be preserved.");
        app.output.info("");
    }
    Ok(args.yes || app.confirm("Continue?", false)?)
}

async fn execute_delete(all: bool, no_backup: bool, app: &AppContext) -> Result<()> {
    let reporter = app.terminal_reporter();
    if all {
        cleanup_service::delete_all(
            &app.provisioner,
            &app.state_mgr,
            &app.local_fs,
            &app.local_fs,
            &app.ssh,
            &reporter,
            no_backup,
        )
        .await
    } else {
        cleanup_service::delete_workspace(
            &app.provisioner,
            &app.state_mgr,
            &app.local_fs,
            &reporter,
            no_backup,
        )
        .await
    }
}
