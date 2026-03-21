//! `polis security bypass` — list bypass domains (traffic not inspected).

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;

/// Run the `security bypass` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the query fails.
pub async fn run(app: &impl App, gateway: &impl SecurityGateway) -> Result<ExitCode> {
    let lines = security::list_bypass_domains(gateway).await?;
    app.renderer().render_security_list("Bypass Domains", "No bypass domains configured.", &lines)?;
    Ok(ExitCode::SUCCESS)
}
