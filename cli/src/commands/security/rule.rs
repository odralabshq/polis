//! `polis security rule` — add a domain rule for auto-approve/prompt/block behavior.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;
use crate::domain::security::AllowAction;

/// Run the `security rule` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the rule cannot be added.
pub async fn run(
    app: &impl App,
    gateway: &impl SecurityGateway,
    pattern: &str,
    action: AllowAction,
) -> Result<ExitCode> {
    let msg = security::add_domain_rule(gateway, pattern, action).await?;
    app.renderer().render_security_action(&msg)?;
    Ok(ExitCode::SUCCESS)
}
