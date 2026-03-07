//! `polis delete [--all]` — remove workspace.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;

use crate::app::AppContext;
use crate::application::services::workspace::{self as workspace_svc, DeleteOutcome};

/// Arguments for the delete command.
#[derive(Args)]
pub struct DeleteArgs {
    /// Remove everything including certificates, cache, and configuration
    #[arg(long)]
    pub all: bool,
}

/// Run `polis delete [--all]`.
///
/// # Errors
///
/// Returns an error if the delete operation fails.
pub async fn run(app: &AppContext, args: &DeleteArgs) -> Result<ExitCode> {
    let confirmed = if args.all {
        app.non_interactive || app.confirm("Remove all data?", false)?
    } else {
        if !app.output.quiet {
            app.output.info("This will remove your workspace.");
            app.output
                .info("Configuration, certificates, and cached downloads will be preserved.");
        }
        app.non_interactive || app.confirm("Continue?", false)?
    };

    if !confirmed {
        app.output.info("Cancelled.");
        return Ok(ExitCode::SUCCESS);
    }

    let reporter = app.terminal_reporter();
    let outcome = if args.all {
        let ctx = workspace_svc::CleanupContext {
            provisioner: &app.provisioner,
            state_store: &app.state_mgr,
            local_fs: &app.local_fs,
            paths: &app.local_fs,
            ssh: &app.ssh,
            reporter: &reporter,
        };
        workspace_svc::delete_all(&ctx).await?;
        DeleteOutcome::Deleted
    } else {
        workspace_svc::delete(&app.provisioner, &app.state_mgr, &reporter).await?
    };

    app.renderer().render_delete_outcome(&outcome, args.all)?;
    Ok(ExitCode::SUCCESS)
}
