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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// format_error always produces valid JSON for any message and code.
        #[test]
        fn prop_format_error_always_valid_json(
            message in "\\PC{0,200}",
            code in "\\PC{0,50}",
        ) {
            let out = format_error(&message, &code).expect("format_error must not fail");
            serde_json::from_str::<serde_json::Value>(&out)
                .expect("output must be valid JSON");
        }

        /// format_error always sets `error: true`.
        #[test]
        fn prop_format_error_always_has_error_true(
            message in "\\PC{0,200}",
            code in "\\PC{0,50}",
        ) {
            let out = format_error(&message, &code).expect("format_error must not fail");
            let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
            prop_assert_eq!(&v["error"], &serde_json::Value::Bool(true));
        }

        /// format_error always preserves the message verbatim.
        #[test]
        fn prop_format_error_always_preserves_message(
            message in "\\PC{0,200}",
            code in "\\PC{0,50}",
        ) {
            let out = format_error(&message, &code).expect("format_error must not fail");
            let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
            prop_assert_eq!(v["message"].as_str(), Some(message.as_str()));
        }

        /// format_error always preserves the code verbatim.
        #[test]
        fn prop_format_error_always_preserves_code(
            message in "\\PC{0,200}",
            code in "\\PC{0,50}",
        ) {
            let out = format_error(&message, &code).expect("format_error must not fail");
            let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
            prop_assert_eq!(v["code"].as_str(), Some(code.as_str()));
        }

        /// format_error always produces pretty-printed (multi-line) JSON.
        #[test]
        fn prop_format_error_always_pretty_printed(
            message in "\\PC{0,200}",
            code in "\\PC{0,50}",
        ) {
            let out = format_error(&message, &code).expect("format_error must not fail");
            prop_assert!(out.trim().contains('\n'), "must be pretty-printed");
        }
    }

    #[test]
    fn test_format_error_is_valid_json() {
        let out = format_error("Failed to connect to workspace", "WORKSPACE_UNREACHABLE")
            .expect("format_error must not fail");
        serde_json::from_str::<serde_json::Value>(&out).expect("output must be valid JSON");
    }

    #[test]
    fn test_format_error_has_error_true() {
        let out = format_error("msg", "CODE").expect("format_error must not fail");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["error"], serde_json::Value::Bool(true));
    }

    #[test]
    fn test_format_error_has_message() {
        let out = format_error("Failed to connect to workspace", "CODE")
            .expect("format_error must not fail");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["message"].as_str(), Some("Failed to connect to workspace"));
    }

    #[test]
    fn test_format_error_has_code() {
        let out = format_error("msg", "WORKSPACE_UNREACHABLE")
            .expect("format_error must not fail");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["code"].as_str(), Some("WORKSPACE_UNREACHABLE"));
    }

    #[test]
    fn test_format_error_is_pretty_printed() {
        let out = format_error("msg", "CODE").expect("format_error must not fail");
        // Pretty-printed JSON has internal newlines; compact JSON does not.
        assert!(
            out.trim().contains('\n'),
            "error JSON must be pretty-printed (contains internal newlines)"
        );
    }
}
