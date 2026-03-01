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
    let quiet = app.output.quiet;
    let reporter = app.terminal_reporter();

    if args.all {
        if !quiet {
            app.output.info("");
            app.output.info("This will permanently remove:");
            app.output.info("  • Your workspace");
            app.output.info("  • Generated certificates");
            app.output.info("  • Configuration");
            app.output.info("  • Cached workspace image (~3.5 GB)");
            app.output.info("");
        }

        if !args.yes && !app.confirm("Continue?", false)? {
            app.output.info("Cancelled.");
            return Ok(std::process::ExitCode::SUCCESS);
        }

        match cleanup_service::delete_all(
            &app.provisioner,
            &app.state_mgr,
            &app.local_fs,
            &app.local_fs,
            &app.ssh,
        )
        .await
        {
            Ok(()) => {}
            Err(e) => {
                app.output.error(&e.to_string());
                return Ok(std::process::ExitCode::FAILURE);
            }
        }
    } else {
        if !quiet {
            app.output.info("");
            app.output.info("This will remove your workspace.");
            app.output
                .info("Configuration, certificates, and cached downloads will be preserved.");
            app.output.info("");
        }

        if !args.yes && !app.confirm("Continue?", false)? {
            app.output.info("Cancelled.");
            return Ok(std::process::ExitCode::SUCCESS);
        }

        match cleanup_service::delete_workspace(&app.provisioner, &app.state_mgr, &reporter).await {
            Ok(()) => {}
            Err(e) => {
                app.output.error(&e.to_string());
                return Ok(std::process::ExitCode::FAILURE);
            }
        }
    }

    if !quiet {
        app.output.info("\nStart fresh: polis start");
    }

    Ok(std::process::ExitCode::SUCCESS)
}
