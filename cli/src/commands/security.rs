//! `polis security` — manage security policy, blocked requests, and domain rules.

use anyhow::Result;
use clap::Subcommand;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;
use crate::domain::security::{AllowAction, SecurityLevel};
use crate::output::models::{LogEntry, PendingRequest, SecurityStatus};

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
    app: &AppContext,
    cmd: SecurityCommand,
    gateway: &impl SecurityGateway,
) -> Result<ExitCode> {
    match cmd {
        SecurityCommand::Status => {
            let s = security::get_status(&app.config_store, gateway).await?;
            let status = SecurityStatus::from_service(&s);
            app.renderer().render_security_status(&status)?;
        }
        SecurityCommand::Pending => {
            let lines = security::list_pending(gateway).await?;
            let requests = PendingRequest::parse_lines(&lines);
            app.renderer().render_security_pending(&requests)?;
        }
        SecurityCommand::Approve { request_id } => {
            let msg = security::approve(gateway, &request_id).await?;
            app.renderer().render_security_action(&msg)?;
        }
        SecurityCommand::Deny { request_id } => {
            let msg = security::deny(gateway, &request_id).await?;
            app.renderer().render_security_action(&msg)?;
        }
        SecurityCommand::Log => {
            let lines = security::get_log(gateway).await?;
            let entries = LogEntry::parse_lines(&lines);
            app.renderer().render_security_log(&entries)?;
        }
        SecurityCommand::Rule { pattern, action } => {
            let msg = security::add_domain_rule(gateway, &pattern, action).await?;
            app.renderer().render_security_action(&msg)?;
        }
        SecurityCommand::Level { level } => {
            let msg = security::set_level(&app.config_store, gateway, level).await?;
            app.renderer().render_security_action(&msg)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
