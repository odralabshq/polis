//! Version command

use anyhow::{Context, Result};

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
pub fn run(json: bool) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    if json {
        println!("{}", version_json(version)?);
    } else {
        println!("polis {version}");
    }
    Ok(())
}
