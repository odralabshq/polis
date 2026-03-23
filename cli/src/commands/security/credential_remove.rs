//! `polis security credential-remove` — remove a persistent credential allow rule.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;

/// Run the `security credential-remove` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the rule doesn't exist.
pub async fn run(
    app: &impl App,
    gateway: &impl SecurityGateway,
    pattern: &str,
    host: &str,
    fingerprint: &str,
) -> Result<ExitCode> {
    let msg = security::remove_credential_allow(gateway, pattern, host, fingerprint).await?;
    app.renderer().render_security_action(&msg)?;
    Ok(ExitCode::SUCCESS)
}
