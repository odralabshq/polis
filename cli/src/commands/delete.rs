//! `polis delete [--all]` — remove workspace.

use anyhow::Result;

use crate::app::AppContext;
use crate::application::services::cleanup_service;
use crate::commands::DeleteArgs;

/// Run `polis delete [--all]`.
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

        cleanup_service::delete_all(
            &app.provisioner,
            &app.state_mgr,
            &app.local_fs,
            &app.local_fs,
        )
        .await?;
    } else {
        if !quiet {
            println!();
            println!("This will remove your workspace.");
            println!("Configuration, certificates, and cached downloads will be preserved.");
            println!();
        }

        if !args.yes && !app.confirm("Continue?", false)? {
            app.output.info("Cancelled.");
            return Ok(std::process::ExitCode::SUCCESS);
        }

        cleanup_service::delete_workspace(&app.provisioner, &app.state_mgr, &reporter).await?;
    }

    if !quiet {
        println!("\nStart fresh: polis start");
    }

    Ok(std::process::ExitCode::SUCCESS)
}
