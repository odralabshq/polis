//! JSON output helpers.

use anyhow::{Context, Result};
use polis_common::types::StatusOutput;

use crate::domain::health::DoctorChecks;

/// Renders domain types as machine-readable JSON output.
pub struct JsonRenderer;

impl JsonRenderer {
    /// Render the CLI version information.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_version(version: &str, build_date: &str) -> Result<()> {
        let val = serde_json::json!({
            "version": version,
            "build_date": build_date
        });
        println!("{}", serde_json::to_string_pretty(&val)?);
        Ok(())
    }
    /// Render workspace/agent/security status as JSON.
    ///
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
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
    /// This function will return an error if the underlying operations fail.
    pub fn render_agent_list(agents: &[crate::domain::agent::AgentInfo]) -> Result<()> {
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
    /// This function will return an error if the underlying operations fail.
    pub fn render_config(config: &crate::domain::config::PolisConfig) -> Result<()> {
        let polis_config_env = std::env::var("POLIS_CONFIG").ok();
        let no_color_env = std::env::var("NO_COLOR").ok();
        let val = serde_json::json!({
            "security": {
                "level": config.security.level
            },
            "environment": {
                "polis_config": polis_config_env,
                "no_color": no_color_env
            }
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&val).context("JSON serialization")?
        );
        Ok(())
    }

    /// Render doctor health check results as JSON.
    ///
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
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
/// # Errors
///
/// This function will return an error if the underlying operations fail.
pub fn format_error(message: &str, code: &str) -> Result<String> {
    let obj = serde_json::json!({
        "error": true,
        "message": message,
        "code": code,
    });
    serde_json::to_string_pretty(&obj).context("JSON serialization failed")
}
