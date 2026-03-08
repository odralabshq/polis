//! `polis security approve` — approve a blocked request.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;

/// Run the `security approve` subcommand.
///
/// # Errors
///
/// Returns an error if the request ID is invalid or the gateway is unreachable.
pub async fn run(app: &impl App, gateway: &impl SecurityGateway, request_id: &str) -> Result<ExitCode> {
    let msg = security::approve(gateway, request_id).await?;
    app.renderer().render_security_action(&msg)?;
    Ok(ExitCode::SUCCESS)
}
