//! Dashboard command handler.

use std::process::ExitCode;

use anyhow::Result;

use crate::app::AppContext;

pub use crate::dashboard::DashboardArgs;

/// Run the interactive control-plane dashboard.
///
/// # Errors
///
/// Returns an error if the dashboard cannot initialize or exits with a failure.
pub async fn run(args: &DashboardArgs, app: &AppContext) -> Result<ExitCode> {
    crate::dashboard::run(args, app).await
}
