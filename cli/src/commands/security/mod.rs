//! `polis security` — manage security policy, blocked requests, and domain rules.

mod approve;
mod deny;
mod level;
mod log;
mod pending;
mod rule;
mod status;

use anyhow::Result;
use clap::Subcommand;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
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
    app: &impl App,
    cmd: SecurityCommand,
    gateway: &impl SecurityGateway,
) -> Result<ExitCode> {
    match cmd {
        SecurityCommand::Status => status::run(app, gateway).await,
        SecurityCommand::Pending => pending::run(app, gateway).await,
        SecurityCommand::Approve { request_id } => approve::run(app, gateway, &request_id).await,
        SecurityCommand::Deny { request_id } => deny::run(app, gateway, &request_id).await,
        SecurityCommand::Log => log::run(app, gateway).await,
        SecurityCommand::Rule { pattern, action } => {
            rule::run(app, gateway, &pattern, action).await
        }
        SecurityCommand::Level { level } => level::run(app, gateway, level).await,
    }
}
