//! `polis security level` — set the security level (updates Valkey + local config).

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;
use crate::domain::security::SecurityLevel;

/// Run the `security level` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the config cannot be saved.
pub async fn run(
    app: &impl App,
    gateway: &impl SecurityGateway,
    level: SecurityLevel,
) -> Result<ExitCode> {
    let msg = security::set_level(app.config(), gateway, level).await?;
    app.renderer().render_security_action(&msg)?;
    Ok(ExitCode::SUCCESS)
}
