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
    let pb = if app.mode == crate::app::OutputMode::Human && app.output.show_progress() {
        Some(crate::output::progress::spinner("gathering status..."))
    } else {
        None
    };

    let output = gather_status(mp).await;

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    app.renderer().render_status(&output)?;
    Ok(std::process::ExitCode::SUCCESS)
}
