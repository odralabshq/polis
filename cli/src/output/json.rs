//! JSON output helpers.

use anyhow::{Context, Result};
use polis_common::agent::OnboardingStep;
use polis_common::types::StatusOutput;

use crate::application::ports::UpdateInfo;
use crate::application::services::workspace::DeleteOutcome;
use crate::application::services::workspace::start::StartOutcome;
use crate::application::services::workspace::stop::StopOutcome;
use crate::domain::health::DiagnosticReport;
use crate::output::ConfigEnv;
use crate::output::models::{ConnectionInfo, LogEntry, PendingRequest, SecurityStatus};

/// Renders domain types as machine-readable JSON output.
pub struct JsonRenderer;

impl JsonRenderer {
    // ── render_*_to_string pure functions ────────────────────────────────────

    /// Serialize version info to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_version_to_string(version: &str, build_date: &str) -> Result<String> {
        let val = serde_json::json!({
            "version": version,
            "build_date": build_date
        });
        serde_json::to_string_pretty(&val).context("JSON serialization")
    }

    /// Serialize workspace/agent/security status to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_status_to_string(status: &StatusOutput) -> Result<String> {
        serde_json::to_string_pretty(status).context("JSON serialization")
    }

    /// Serialize agent list to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_agent_list_to_string(
        agents: &[crate::domain::agent::AgentInfo],
    ) -> Result<String> {
        serde_json::to_string_pretty(&serde_json::json!({ "agents": agents }))
            .context("JSON serialization")
    }

    /// Serialize polis configuration to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_config_to_string(
        config: &crate::domain::config::PolisConfig,
        config_env: &ConfigEnv,
    ) -> Result<String> {
        let val = serde_json::json!({
            "security": {
                "level": config.security.level
            },
            "environment": {
                "polis_config": config_env.polis_config,
                "no_color": config_env.no_color
            }
        });
        serde_json::to_string_pretty(&val).context("JSON serialization")
    }

    /// Serialize diagnostic health check results to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_diagnostics_to_string(
        checks: &DiagnosticReport,
        issues: &[String],
    ) -> Result<String> {
        let status = if issues.is_empty() {
            "healthy"
        } else {
            "unhealthy"
        };
        let checks_value = serde_json::to_value(checks).context("serializing diagnostic report")?;
        let out = serde_json::json!({
            "status": status,
            "checks": checks_value,
            "issues": issues,
        });
        serde_json::to_string_pretty(&out).context("JSON serialization")
    }

    /// Serialize connection info to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_connection_info_to_string(info: &ConnectionInfo) -> Result<String> {
        serde_json::to_string_pretty(info).context("JSON serialization")
    }

    /// Serialize stop outcome to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_stop_outcome_to_string(outcome: &StopOutcome) -> Result<String> {
        let status = match outcome {
            StopOutcome::NotFound => "not_found",
            StopOutcome::AlreadyStopped => "already_stopped",
            StopOutcome::Stopped => "stopped",
        };
        serde_json::to_string_pretty(&serde_json::json!({ "status": status }))
            .context("JSON serialization")
    }

    /// Serialize delete outcome to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_delete_outcome_to_string(outcome: &DeleteOutcome, all: bool) -> Result<String> {
        let status = match outcome {
            DeleteOutcome::NotFound => "not_found",
            DeleteOutcome::Deleted => "deleted",
        };
        serde_json::to_string_pretty(&serde_json::json!({
            "status": status,
            "scope": if all { "all" } else { "workspace" }
        }))
        .context("JSON serialization")
    }

    /// Serialize start outcome to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_start_outcome_to_string(
        outcome: &StartOutcome,
        onboarding: &[OnboardingStep],
    ) -> Result<String> {
        let (status, agent) = match outcome {
            StartOutcome::AlreadyRunning { active_agent } => {
                ("already_running", active_agent.clone())
            }
            StartOutcome::Created { .. } => ("created", None),
            StartOutcome::Restarted { .. } => ("restarted", None),
        };
        serde_json::to_string_pretty(&serde_json::json!({
            "status": status,
            "active_agent": agent,
            "onboarding": onboarding
        }))
        .context("JSON serialization")
    }

    /// Serialize update info to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_update_info_to_string(current: &str, info: &UpdateInfo) -> Result<String> {
        let output = match info {
            UpdateInfo::UpToDate => serde_json::json!({
                "current_version": current,
                "status": "up_to_date"
            }),
            UpdateInfo::Available {
                version,
                release_notes,
                download_url,
            } => serde_json::json!({
                "current_version": current,
                "status": "update_available",
                "new_version": version,
                "release_notes": release_notes,
                "download_url": download_url
            }),
        };
        serde_json::to_string_pretty(&output).context("JSON serialization")
    }

    /// Serialize security status to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_status_to_string(status: &SecurityStatus) -> Result<String> {
        serde_json::to_string_pretty(&serde_json::json!({
            "level": status.level,
            "pending_count": status.pending_count,
            "pending_error": status.pending_error
        }))
        .context("JSON serialization")
    }

    /// Serialize security pending requests to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_pending_to_string(requests: &[PendingRequest]) -> Result<String> {
        serde_json::to_string_pretty(&serde_json::json!({
            "pending_requests": requests
        }))
        .context("JSON serialization")
    }

    /// Serialize security log entries to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_log_to_string(entries: &[LogEntry]) -> Result<String> {
        serde_json::to_string_pretty(&serde_json::json!({
            "log_entries": entries
        }))
        .context("JSON serialization")
    }

    /// Serialize security action result to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_action_to_string(message: &str) -> Result<String> {
        serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "message": message
        }))
        .context("JSON serialization")
    }

    // ── render_* thin wrappers ────────────────────────────────────────────────

    /// Render the CLI version information.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_version(version: &str, build_date: &str) -> Result<()> {
        println!("{}", Self::render_version_to_string(version, build_date)?);
        Ok(())
    }

    /// Render workspace/agent/security status as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_status(status: &StatusOutput) -> Result<()> {
        println!("{}", Self::render_status_to_string(status)?);
        Ok(())
    }

    /// Render the list of installed agents as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_agent_list(agents: &[crate::domain::agent::AgentInfo]) -> Result<()> {
        println!("{}", Self::render_agent_list_to_string(agents)?);
        Ok(())
    }

    /// Render the current polis configuration as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_config(
        config: &crate::domain::config::PolisConfig,
        config_env: &ConfigEnv,
    ) -> Result<()> {
        println!("{}", Self::render_config_to_string(config, config_env)?);
        Ok(())
    }

    /// Render doctor health check results as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_diagnostics(checks: &DiagnosticReport, issues: &[String]) -> Result<()> {
        println!("{}", Self::render_diagnostics_to_string(checks, issues)?);
        Ok(())
    }

    /// Render connection info as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_connection_info(info: &ConnectionInfo) -> Result<()> {
        println!("{}", Self::render_connection_info_to_string(info)?);
        Ok(())
    }

    /// Render stop command outcome as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_stop_outcome(outcome: &StopOutcome) -> Result<()> {
        println!("{}", Self::render_stop_outcome_to_string(outcome)?);
        Ok(())
    }

    /// Render delete command outcome as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_delete_outcome(outcome: &DeleteOutcome, all: bool) -> Result<()> {
        println!("{}", Self::render_delete_outcome_to_string(outcome, all)?);
        Ok(())
    }

    /// Render start command outcome as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_start_outcome(
        outcome: &StartOutcome,
        onboarding: &[OnboardingStep],
    ) -> Result<()> {
        println!(
            "{}",
            Self::render_start_outcome_to_string(outcome, onboarding)?
        );
        Ok(())
    }

    /// Render update info as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_update_info(current: &str, info: &UpdateInfo) -> Result<()> {
        println!("{}", Self::render_update_info_to_string(current, info)?);
        Ok(())
    }

    /// Render security status as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_status(status: &SecurityStatus) -> Result<()> {
        println!("{}", Self::render_security_status_to_string(status)?);
        Ok(())
    }

    /// Render security pending requests as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_pending(requests: &[PendingRequest]) -> Result<()> {
        println!("{}", Self::render_security_pending_to_string(requests)?);
        Ok(())
    }

    /// Render security log entries as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_log(entries: &[LogEntry]) -> Result<()> {
        println!("{}", Self::render_security_log_to_string(entries)?);
        Ok(())
    }

    /// Render security action result as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_action(message: &str) -> Result<()> {
        println!("{}", Self::render_security_action_to_string(message)?);
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::output::ConfigEnv;
    use crate::domain::config::PolisConfig;

    // ── render_version_to_string ──────────────────────────────────────────────

    #[test]
    fn test_render_version_to_string_contains_fields() {
        let result = JsonRenderer::render_version_to_string("1.2.3", "2024-06-01")
            .expect("serialize");
        let val: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(val["version"], "1.2.3");
        assert_eq!(val["build_date"], "2024-06-01");
    }

    #[test]
    fn test_render_version_to_string_is_valid_json() {
        let result = JsonRenderer::render_version_to_string("0.1.0", "2025-01-01")
            .expect("serialize");
        assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
    }

    // ── render_config_to_string ───────────────────────────────────────────────

    #[test]
    fn test_render_config_to_string_none_fields_produce_null() {
        let config = PolisConfig::default();
        let config_env = ConfigEnv {
            polis_config: None,
            no_color: None,
        };
        let result = JsonRenderer::render_config_to_string(&config, &config_env)
            .expect("serialize");
        let val: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert!(val["environment"]["polis_config"].is_null());
        assert!(val["environment"]["no_color"].is_null());
    }

    #[test]
    fn test_render_config_to_string_with_values_serializes_correctly() {
        let config = PolisConfig::default();
        let config_env = ConfigEnv {
            polis_config: Some("/path/to/config.yaml".to_string()),
            no_color: Some("1".to_string()),
        };
        let result = JsonRenderer::render_config_to_string(&config, &config_env)
            .expect("serialize");
        let val: serde_json::Value = serde_json::from_str(&result).expect("parse");
        assert_eq!(val["environment"]["polis_config"], "/path/to/config.yaml");
        assert_eq!(val["environment"]["no_color"], "1");
    }

    #[test]
    fn test_render_config_to_string_contains_security_level() {
        let config = PolisConfig::default();
        let config_env = ConfigEnv {
            polis_config: None,
            no_color: None,
        };
        let result = JsonRenderer::render_config_to_string(&config, &config_env)
            .expect("serialize");
        let val: serde_json::Value = serde_json::from_str(&result).expect("parse");
        // Default security level should be present
        assert!(!val["security"]["level"].is_null());
    }
}
