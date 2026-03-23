//! `polis security status` — show security status (level, pending count, feature health).

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;
use crate::output::models::SecurityStatus;

/// Run the `security status` subcommand.
///
/// # Errors
///
/// Returns an error if the config cannot be loaded or the gateway is unreachable.
pub async fn run(app: &impl App, gateway: &impl SecurityGateway) -> Result<ExitCode> {
    let s = security::get_status(app.config(), gateway).await?;
    let status = SecurityStatus::from_service(&s);
    app.renderer().render_security_status(&status)?;
    Ok(ExitCode::SUCCESS)
}
