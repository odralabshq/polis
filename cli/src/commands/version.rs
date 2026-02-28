//! `polis version` — show version and diagnostic info.

use crate::app::AppContext;
use anyhow::Result;
use std::process::ExitCode;

/// Run the version command.
pub async fn run(_app: &AppContext) -> Result<ExitCode> {
    let version = env!("CARGO_PKG_VERSION");
    let commit = option_env!("VERGEN_GIT_SHA").unwrap_or("unknown");
    let build_date = option_env!("VERGEN_BUILD_DATE").unwrap_or("unknown");

    println!("polis v{} ({} {})", version, commit, build_date);
    Ok(ExitCode::SUCCESS)
}
