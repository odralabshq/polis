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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_version_json_returns_valid_json() {
        let out = version_json("1.2.3").expect("must not fail");
        serde_json::from_str::<serde_json::Value>(&out).expect("must be valid JSON");
    }

    #[test]
    fn test_version_json_has_version_field() {
        let out = version_json("1.2.3").expect("must not fail");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert!(v["version"].is_string(), "must have a 'version' string field");
    }

    #[test]
    fn test_version_json_version_value_matches_input() {
        let out = version_json("0.1.0").expect("must not fail");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["version"].as_str(), Some("0.1.0"));
    }

    #[test]
    fn test_version_json_is_pretty_printed() {
        let out = version_json("1.0.0").expect("must not fail");
        assert!(
            out.trim().contains('\n'),
            "version JSON must be pretty-printed; got: {out:?}"
        );
    }

    #[test]
    fn test_version_json_no_extra_fields() {
        let out = version_json("1.0.0").expect("must not fail");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let obj = v.as_object().expect("must be a JSON object");
        assert_eq!(obj.len(), 1, "version JSON must have exactly one field");
    }

    // ── property tests ────────────────────────────────────────────────────────

    proptest! {
        /// version_json always produces valid JSON for any version string.
        #[test]
        fn prop_version_json_always_valid_json(version in "\\PC{0,50}") {
            let out = version_json(&version).expect("must not fail");
            serde_json::from_str::<serde_json::Value>(&out).expect("must be valid JSON");
        }

        /// version_json always has a `version` string field.
        #[test]
        fn prop_version_json_always_has_version_field(version in "\\PC{0,50}") {
            let out = version_json(&version).expect("must not fail");
            let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
            prop_assert!(v["version"].is_string());
        }

        /// version_json always preserves the version value verbatim.
        #[test]
        fn prop_version_json_always_preserves_version(version in "\\PC{0,50}") {
            let out = version_json(&version).expect("must not fail");
            let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
            prop_assert_eq!(v["version"].as_str(), Some(version.as_str()));
        }

        /// version_json always produces pretty-printed output.
        #[test]
        fn prop_version_json_always_pretty_printed(version in "\\PC{0,50}") {
            let out = version_json(&version).expect("must not fail");
            prop_assert!(out.trim().contains('\n'), "must be pretty-printed");
        }
    }
}
