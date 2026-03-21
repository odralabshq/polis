//! `polis security credentials` — list persistent credential allow rules.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::App;
use crate::application::ports::SecurityGateway;
use crate::application::services::security;

/// Run the `security credentials` subcommand.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable or the query fails.
pub async fn run(app: &impl App, gateway: &impl SecurityGateway) -> Result<ExitCode> {
    let lines = security::list_credential_allows(gateway).await?;
    app.renderer().render_security_list("Credential Allow Rules", "No credential allow rules configured.", &lines)?;
    Ok(ExitCode::SUCCESS)
}
