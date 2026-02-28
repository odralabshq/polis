//! Version command

use anyhow::{Context, Result};

use crate::app::{AppContext, OutputMode};

/// Build the pretty-printed JSON string for `version --json`.
fn version_json(version: &str) -> Result<String> {
    serde_json::to_string_pretty(&serde_json::json!({ "version": version }))
        .context("JSON serialization")
}

/// Run the version command.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub fn run(app: &AppContext) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    if app.mode == OutputMode::Json {
        println!("{}", version_json(version)?);
    } else {
        println!("polis {version}");
    }
    Ok(())
}
