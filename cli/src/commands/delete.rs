//! `polis delete [--all]` — remove workspace.

use anyhow::Result;

use crate::app::AppContext;
use crate::application::services::cleanup_service;
use crate::commands::DeleteArgs;

/// Run `polis delete [--all]`.
pub async fn run(args: &DeleteArgs, app: &AppContext) -> Result<()> {
    let quiet = app.output.quiet;
    let reporter = app.terminal_reporter();

    if args.all {
        if !quiet {
            println!();
            println!("This will permanently remove:");
            println!("  • Your workspace");
            println!("  • Generated certificates");
            println!("  • Configuration");
            println!("  • Cached workspace image (~3.5 GB)");
            println!();
        }

        if !args.yes && !app.confirm("Continue?", false)? {
            println!("Cancelled.");
            return Ok(());
        }

        cleanup_service::delete_all(
            &app.provisioner,
            &app.state_mgr,
            &app.local_fs,
            &app.local_fs,
            &reporter,
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
            println!("Cancelled.");
            return Ok(());
        }

        cleanup_service::delete_workspace(&app.provisioner, &app.state_mgr, &reporter).await?;
    }

    if !quiet {
        println!("\nStart fresh: polis start");
    }

    Ok(())
}
