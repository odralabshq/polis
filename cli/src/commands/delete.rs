//! `polis delete [--all]` — remove workspace.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;

use crate::app::App;
use crate::application::services::workspace::{self as workspace_svc, DeleteOutcome};

/// Arguments for the delete command.
#[derive(Args)]
pub struct DeleteArgs {
    /// Remove everything including certificates, cache, and configuration
    #[arg(long)]
    pub all: bool,

    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Skip workspace data backup before deletion
    #[arg(long)]
    pub no_backup: bool,
}

/// Run `polis delete [--all]`.
///
/// # Errors
///
/// Returns an error if the delete operation fails.
pub async fn run(app: &impl App, args: &DeleteArgs) -> Result<ExitCode> {
    let confirmed = if args.all {
        args.yes || app.non_interactive() || app.confirm("Remove all data?", false)?
    } else {
        if !app.output().quiet {
            app.output().info("This will remove your workspace.");
            if args.no_backup {
                app.output()
                    .warn("Backup is disabled — workspace data will be permanently lost.");
            } else {
                app.output()
                    .info("Workspace data will be backed up to ~/.polis/backups/ before removal.");
            }
            app.output()
                .info("Configuration, certificates, and cached downloads will be preserved.");
        }
        args.yes || app.non_interactive() || app.confirm("Continue?", false)?
    };

    if !confirmed {
        app.output().info("Cancelled.");
        return Ok(ExitCode::SUCCESS);
    }

    let reporter = app.terminal_reporter();
    let outcome = if args.all {
        let ctx = workspace_svc::CleanupContext {
            provisioner: app.provisioner(),
            state_store: app.state_store(),
            local_fs: app.fs(),
            paths: app.fs(),
            ssh: app.ssh(),
            reporter: &reporter,
            skip_backup: args.no_backup,
        };
        workspace_svc::delete_all(&ctx).await?;
        DeleteOutcome::Deleted
    } else {
        workspace_svc::delete(app.provisioner(), app.state_store(), &reporter, args.no_backup).await?
    };

    app.renderer().render_delete_outcome(&outcome, args.all)?;
    Ok(ExitCode::SUCCESS)
}
