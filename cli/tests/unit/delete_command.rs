//! Unit tests for `polis delete` command handler.

use anyhow::Result;
use polis_cli::commands::delete::{DeleteArgs, run};
use polis_cli::test_utils::MockAppContext;
use std::process::ExitCode;

#[tokio::test]
async fn test_delete_cancelled_when_not_confirmed() -> Result<()> {
    let mut app = MockAppContext::new();
    // non_interactive=false → confirm() returns default=false → cancelled
    app.non_interactive = false;
    let args = DeleteArgs { all: false };
    let result = run(&app, &args).await?;
    assert_eq!(result, ExitCode::SUCCESS);
    Ok(())
}

#[tokio::test]
async fn test_delete_proceeds_when_non_interactive() {
    let app = MockAppContext::new(); // non_interactive=true → confirmed
    let args = DeleteArgs { all: false };
    // Mock provisioner returns "Running" → delete proceeds → returns Deleted or NotFound
    let result = run(&app, &args).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_all_cancelled_when_not_confirmed() -> Result<()> {
    let mut app = MockAppContext::new();
    app.non_interactive = false;
    let args = DeleteArgs { all: true };
    let result = run(&app, &args).await?;
    assert_eq!(result, ExitCode::SUCCESS);
    Ok(())
}
