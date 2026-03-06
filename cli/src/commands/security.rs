//! `polis security` — manage security policy, blocked requests, and domain rules.

use anyhow::Result;
use clap::Subcommand;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;
use crate::application::services::security::PendingResult;
use crate::domain::security::{AllowAction, SecurityLevel};

/// Security subcommands.
#[derive(Subcommand)]
pub enum SecurityCommand {
    /// Show security status (level, pending count, feature health)
    Status,
    /// List pending blocked requests awaiting approval
    Pending,
    /// Approve a blocked request
    Approve {
        /// Request ID to approve (format: req-[a-f0-9]{8})
        request_id: String,
    },
    /// Deny a blocked request
    Deny {
        /// Request ID to deny (format: req-[a-f0-9]{8})
        request_id: String,
    },
    /// Show recent security events
    Log,
    /// Add a domain rule for auto-approve/prompt/block behavior
    Rule {
        /// Domain pattern (e.g. "cli.kiro.dev" or "*.example.com")
        pattern: String,
        /// Action: allow (default), prompt, or block
        #[arg(long, default_value_t = AllowAction::Allow, value_enum)]
        action: AllowAction,
    },
    /// Set the security level
    Level {
        /// Security level: relaxed, balanced, or strict
        #[arg(value_enum)]
        level: SecurityLevel,
    },
}

/// Run a security command.
///
/// # Errors
///
/// Returns an error if the underlying operations fail.
pub async fn run(
    cmd: SecurityCommand,
    app: &AppContext,
    gateway: &impl SecurityGateway,
) -> Result<ExitCode> {
    match cmd {
        SecurityCommand::Status => {
            let s = security::get_status(&app.config_store, gateway).await?;
            app.output.info(&format!("Security level: {}", s.level));
            match s.pending {
                PendingResult::Err(err) => {
                    app.output
                        .warn(&format!("Could not query pending requests: {err}"));
                }
                PendingResult::Ok(requests) if requests.is_empty() => {
                    app.output.success("No pending blocked requests");
                }
                PendingResult::Ok(requests) => {
                    app.output.warn(&format!(
                        "{} pending blocked request(s)",
                        requests.len()
                    ));
                }
            }
        }
        SecurityCommand::Pending => {
            let lines = security::list_pending(gateway).await?;
            if lines.is_empty() {
                app.output.success("No pending blocked requests");
            } else {
                for line in &lines {
                    app.output.info(line);
                }
            }
        }
        SecurityCommand::Approve { request_id } => {
            let msg = security::approve(gateway, &request_id).await?;
            app.output.success(&msg);
        }
        SecurityCommand::Deny { request_id } => {
            let msg = security::deny(gateway, &request_id).await?;
            app.output.success(&msg);
        }
        SecurityCommand::Log => {
            let lines = security::get_log(gateway).await?;
            if lines.is_empty() {
                app.output.info("No recent security events");
            } else {
                for line in &lines {
                    app.output.info(line);
                }
            }
        }
        SecurityCommand::Rule { pattern, action } => {
            let msg = security::add_domain_rule(gateway, &pattern, action).await?;
            app.output.success(&msg);
        }
        SecurityCommand::Level { level } => {
            let msg = security::set_level(&app.config_store, gateway, level).await?;
            app.output.success(&msg);
        }
    }
    Ok(ExitCode::SUCCESS)
}
