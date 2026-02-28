//! `polis version` — show version and diagnostic info.

use crate::app::AppContext;
use anyhow::Result;
use std::process::ExitCode;

/// Run the version command.
///
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub fn run(app: &AppContext) -> Result<ExitCode> {
    let version = env!("CARGO_PKG_VERSION");
    let commit = option_env!("VERGEN_GIT_SHA").unwrap_or("unknown");
    let build_date = option_env!("VERGEN_BUILD_DATE").unwrap_or("unknown");

    app.renderer().render_version(version, commit, build_date)?;
    Ok(ExitCode::SUCCESS)
}
