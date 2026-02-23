//! JSON output helpers.
//!
//! Provides the error-object formatter used by all `--json` code paths when
//! a command fails.  The schema is defined in issue 18 §2.7.

use anyhow::{Context, Result};

/// Format a JSON error object per the spec error schema (issue 18 §2.7).
///
/// Output (pretty-printed):
/// ```json
/// {
///   "error": true,
///   "message": "...",
///   "code": "..."
/// }
/// ```
///
/// # Errors
///
/// Returns an error if JSON serialization fails (should not happen in
/// practice — `serde_json` only fails on non-finite floats and maps with
/// non-string keys, neither of which appear here).
pub fn format_error(message: &str, code: &str) -> Result<String> {
    let obj = serde_json::json!({
        "error": true,
        "message": message,
        "code": code,
    });
    serde_json::to_string_pretty(&obj).context("JSON serialization failed")
}
