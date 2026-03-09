//! Unit tests for `polis stop` command handler.

use anyhow::Result;
use polis_cli::commands::stop;
use polis_cli::test_utils::MockAppContext;
use std::process::ExitCode;

#[tokio::test]
async fn test_stop_vm_running_returns_success() -> Result<()> {
    let app = MockAppContext::new();
    // Default mock provisioner reports VM as Running; stop() succeeds.
    let result = stop::run(&app).await?;
    assert_eq!(result, ExitCode::SUCCESS);
    Ok(())
}

#[tokio::test]
async fn test_stop_vm_not_found_returns_success() {
    let app = MockAppContext::new();
    app.provisioner.set_info_not_found();
    // stop service returns NotRunning outcome — command still returns Ok.
    let result = stop::run(&app).await;
    assert!(result.is_ok());
}
