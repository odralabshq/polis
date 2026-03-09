//! Unit tests for `polis version` command handler.

use anyhow::Result;
use polis_cli::app::{AppContext, AppFlags, BehaviourFlags, OutputFlags};
use std::process::ExitCode;

fn app(json: bool) -> Result<AppContext> {
    AppContext::new(&AppFlags {
        output: OutputFlags {
            no_color: true,
            quiet: true,
            json,
        },
        behaviour: BehaviourFlags { yes: true },
    })
}

#[test]
fn test_version_run_returns_success() -> Result<()> {
    let result = polis_cli::commands::version::run(&app(false)?)?;
    assert_eq!(result, ExitCode::SUCCESS);
    Ok(())
}

#[test]
fn test_version_run_json_returns_success() -> Result<()> {
    let result = polis_cli::commands::version::run(&app(true)?)?;
    assert_eq!(result, ExitCode::SUCCESS);
    Ok(())
}
