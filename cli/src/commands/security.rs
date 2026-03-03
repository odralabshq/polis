//! `polis security` — manage security policy, blocked requests, and domain rules.

use anyhow::Result;
use clap::Subcommand;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::ports::ShellExecutor;
use crate::application::services::security_service;

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
    /// Auto-approve a domain pattern
    Allow {
        /// Domain pattern to allow (e.g. "cli.kiro.dev" or "*.example.com")
        pattern: String,
        /// Action: allow (default), prompt, or block
        #[arg(long, default_value = "allow")]
        action: String,
    },
    /// Set the security level
    Level {
        /// Security level: relaxed, balanced, or strict
        level: String,
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
    mp: &impl ShellExecutor,
) -> Result<ExitCode> {
    match cmd {
        SecurityCommand::Status => {
            let s = security_service::get_status(&app.config_store, mp).await?;
            app.output.info(&format!("Security level: {}", s.level));
            if let Some(err) = s.pending_error {
                app.output
                    .warn(&format!("Could not query pending requests: {err}"));
            } else if s.pending_lines.is_empty() {
                app.output.success("No pending blocked requests");
            } else {
                app.output.warn(&format!(
                    "{} pending blocked request(s)",
                    s.pending_lines.len()
                ));
            }
            Ok(ExitCode::SUCCESS)
        }
        SecurityCommand::Pending => {
            let lines = security_service::list_pending(mp).await?;
            if lines.is_empty() {
                app.output.success("No pending blocked requests");
            } else {
                for line in &lines {
                    app.output.info(line);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        SecurityCommand::Approve { request_id } => {
            let msg = security_service::approve(mp, &request_id).await?;
            app.output.success(&msg);
            Ok(ExitCode::SUCCESS)
        }
        SecurityCommand::Deny { request_id } => {
            let msg = security_service::deny(mp, &request_id).await?;
            app.output.success(&msg);
            Ok(ExitCode::SUCCESS)
        }
        SecurityCommand::Log => {
            let lines = security_service::get_log(mp).await?;
            if lines.is_empty() {
                println!("No recent security events");
            } else {
                for line in &lines {
                    println!("{line}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        SecurityCommand::Allow { pattern, action } => {
            let msg = security_service::auto_allow(mp, &pattern, &action).await?;
            app.output.success(&msg);
            Ok(ExitCode::SUCCESS)
        }
        SecurityCommand::Level { level } => {
            let msg = security_service::set_level(&app.config_store, mp, &level).await?;
            app.output.success(&msg);
            Ok(ExitCode::SUCCESS)
        }
    }
}
