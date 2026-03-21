//! `polis security rule-remove` — remove an auto-approve rule.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;

/// Run the `security rule-remove` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the rule doesn't exist.
pub async fn run(
    app: &impl App,
    gateway: &impl SecurityGateway,
    pattern: &str,
) -> Result<ExitCode> {
    let msg = security::remove_rule(gateway, pattern).await?;
    app.renderer().render_security_action(&msg)?;
    Ok(ExitCode::SUCCESS)
}
