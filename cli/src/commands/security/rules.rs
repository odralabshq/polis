//! `polis security rules` — list all auto-approve rules.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;

/// Run the `security rules` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the query fails.
pub async fn run(app: &impl App, gateway: &impl SecurityGateway) -> Result<ExitCode> {
    let lines = security::list_rules(gateway).await?;
    app.renderer().render_security_list(
        "Auto-Approve Rules",
        "No auto-approve rules configured.",
        &lines,
    )?;
    Ok(ExitCode::SUCCESS)
}
