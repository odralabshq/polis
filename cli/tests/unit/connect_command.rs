//! Unit tests for `polis connect` command handler.

use anyhow::Result;
use polis_cli::commands::connect::{ConnectArgs, run};
use polis_cli::test_utils::MockAppContext;
use std::process::ExitCode;

#[tokio::test]
async fn test_connect_info_flag_returns_success() -> Result<()> {
    let app = MockAppContext::new();
    // Default mock: VM is Running. --info returns connection strings without SSH.
    let args = ConnectArgs { info: true };
    let result = run(&app, &args).await?;
    assert_eq!(result, ExitCode::SUCCESS);
    Ok(())
}

#[tokio::test]
async fn test_connect_vm_not_running_returns_err() {
    let app = MockAppContext::new();
    app.provisioner.set_info_not_found();
    let args = ConnectArgs { info: false };
    let result = run(&app, &args).await;
    assert!(result.is_err());
    assert!(
        result
            .err()
            .is_some_and(|e| e.to_string().to_lowercase().contains("not running"))
    );
}
