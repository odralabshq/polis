//! `polis security log` — display recent security event log entries.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;
use crate::output::models::LogEntry;

/// Run the `security log` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the log query fails.
pub async fn run(app: &impl App, gateway: &impl SecurityGateway) -> Result<ExitCode> {
    let lines = security::get_log(gateway).await?;
    let entries = LogEntry::parse_lines(&lines);
    app.renderer().render_security_log(&entries)?;
    Ok(ExitCode::SUCCESS)
}
