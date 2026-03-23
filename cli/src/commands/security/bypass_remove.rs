//! `polis security bypass-remove` — remove a bypass domain.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;

/// Run the `security bypass-remove` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the domain doesn't exist.
pub async fn run(app: &impl App, gateway: &impl SecurityGateway, domain: &str) -> Result<ExitCode> {
    let msg = security::remove_bypass_domain(gateway, domain).await?;
    app.renderer().render_security_action(&msg)?;
    Ok(ExitCode::SUCCESS)
}
