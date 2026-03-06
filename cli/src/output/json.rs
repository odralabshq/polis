//! JSON output helpers.

use anyhow::{Context, Result};
use polis_common::agent::OnboardingStep;
use polis_common::types::StatusOutput;

use crate::application::services::update::UpdateInfo;
use crate::application::services::workspace_delete::DeleteOutcome;
use crate::application::services::workspace_start::StartOutcome;
use crate::application::services::workspace_stop::StopOutcome;
use crate::domain::health::DiagnosticReport;
use crate::output::models::{ConnectionInfo, LogEntry, PendingRequest, SecurityStatus};

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
    pub fn render_diagnostics(checks: &DiagnosticReport, issues: &[String]) -> Result<()> {
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
        println!(
            "{}",
            serde_json::to_string_pretty(&out).context("JSON serialization")?
        );
        Ok(())
    }

    /// Render connection info as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_connection_info(info: &ConnectionInfo) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(info).context("JSON serialization")?
        );
        Ok(())
    }

    /// Render stop command outcome as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_stop_outcome(outcome: &StopOutcome) -> Result<()> {
        let status = match outcome {
            StopOutcome::NotFound => "not_found",
            StopOutcome::AlreadyStopped => "already_stopped",
            StopOutcome::Stopped => "stopped",
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": status
            }))
            .context("JSON serialization")?
        );
        Ok(())
    }

    /// Render delete command outcome as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_delete_outcome(outcome: &DeleteOutcome, all: bool) -> Result<()> {
        let status = match outcome {
            DeleteOutcome::NotFound => "not_found",
            DeleteOutcome::Deleted => "deleted",
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": status,
                "scope": if all { "all" } else { "workspace" }
            }))
            .context("JSON serialization")?
        );
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
        let (status, agent) = match outcome {
            StartOutcome::AlreadyRunning { active_agent } => {
                ("already_running", active_agent.clone())
            }
            StartOutcome::Created { .. } => ("created", None),
            StartOutcome::Restarted { .. } => ("restarted", None),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": status,
                "active_agent": agent,
                "onboarding": onboarding
            }))
            .context("JSON serialization")?
        );
        Ok(())
    }

    /// Render update info as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_update_info(current: &str, info: &UpdateInfo) -> Result<()> {
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
        println!(
            "{}",
            serde_json::to_string_pretty(&output).context("JSON serialization")?
        );
        Ok(())
    }

    /// Render security status as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_status(status: &SecurityStatus) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "level": status.level,
                "pending_count": status.pending_count,
                "pending_error": status.pending_error
            }))
            .context("JSON serialization")?
        );
        Ok(())
    }

    /// Render security pending requests as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_pending(requests: &[PendingRequest]) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "pending_requests": requests
            }))
            .context("JSON serialization")?
        );
        Ok(())
    }

    /// Render security log entries as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_log(entries: &[LogEntry]) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "log_entries": entries
            }))
            .context("JSON serialization")?
        );
        Ok(())
    }

    /// Render security action result as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_action(message: &str) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "success": true,
                "message": message
            }))
            .context("JSON serialization")?
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
