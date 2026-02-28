//! JSON output helpers.
//!
//! Provides the `JsonRenderer` struct and the error-object formatter used by
//! all `--json` code paths when a command fails.  The schema is defined in
//! issue 18 §2.7.

use anyhow::{Context, Result};
use polis_common::types::StatusOutput;
use serde_json::Value as JsonValue;

use crate::domain::health::DoctorChecks;

/// Renders domain types as machine-readable JSON output.
///
/// Unit struct — no state needed; all output goes to stdout via `println!`.
pub struct JsonRenderer;

impl JsonRenderer {
    /// Render workspace/agent/security status as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_status(status: &StatusOutput) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(status).context("JSON serialization")?
        );
        Ok(())
    }

    /// Render the list of installed agents as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_agents(agents: &[JsonValue]) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "agents": agents }))
                .context("JSON serialization")?
        );
        Ok(())
    }

    /// Render the current polis configuration as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_config(config: &crate::domain::config::PolisConfig) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(config).context("JSON serialization")?
        );
        Ok(())
    }

    /// Render doctor health check results as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_doctor(checks: &DoctorChecks, issues: &[String]) -> Result<()> {
        let status = if issues.is_empty() {
            "healthy"
        } else {
            "unhealthy"
        };
        let out = serde_json::json!({
            "status": status,
            "checks": {
                "prerequisites": {
                    "multipass_found": checks.prerequisites.multipass_found,
                    "multipass_version": checks.prerequisites.multipass_version,
                    "multipass_version_ok": checks.prerequisites.multipass_version_ok,
                },
                "workspace": {
                    "ready": checks.workspace.ready,
                    "disk_space_gb": checks.workspace.disk_space_gb,
                    "disk_space_ok": checks.workspace.disk_space_ok,
                    "image": checks.workspace.image,
                },
                "network": {
                    "internet": checks.network.internet,
                    "dns": checks.network.dns,
                },
                "security": {
                    "process_isolation": checks.security.process_isolation,
                    "traffic_inspection": checks.security.traffic_inspection,
                    "malware_db_current": checks.security.malware_db_current,
                    "malware_db_age_hours": checks.security.malware_db_age_hours,
                    "certificates_valid": checks.security.certificates_valid,
                    "certificates_expire_days": checks.security.certificates_expire_days,
                },
            },
            "issues": issues,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).context("JSON serialization")?
        );
        Ok(())
    }
}

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
