//! Status command implementation.
//!
//! Thin handler: delegates status gathering to `application::services::workspace_status`,
//! then renders the result via `app.renderer()`.

use anyhow::Result;

use crate::app::AppContext;
use crate::application::ports::{InstanceInspector, ShellExecutor};
use crate::application::services::workspace_status::gather_status;

/// Run the status command.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
pub async fn run(
    app: &AppContext,
    mp: &(impl InstanceInspector + ShellExecutor),
) -> Result<std::process::ExitCode> {
    let output = gather_status(mp).await;
    app.renderer().render_status(&output)?;
    Ok(std::process::ExitCode::SUCCESS)
}
