//! `polis security pending` — list pending blocked requests awaiting approval.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;
use crate::output::models::PendingRequest;

/// Run the `security pending` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the pending request query fails.
pub async fn run(app: &impl App, gateway: &impl SecurityGateway) -> Result<ExitCode> {
    let lines = security::list_pending(gateway).await?;
    let requests = PendingRequest::parse_lines(&lines);
    app.renderer().render_security_pending(&requests)?;
    Ok(ExitCode::SUCCESS)
}
