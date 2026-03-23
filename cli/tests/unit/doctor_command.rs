//! Unit tests for `polis doctor` command handler.

use polis_cli::commands::doctor::{DoctorArgs, run};
use polis_cli::test_utils::MockAppContext;

#[tokio::test]
async fn test_doctor_no_issues_returns_success() {
    let app = MockAppContext::new();
    // All mocks return success. Some checks may still report issues (e.g. version
    // parsing from empty output), but the handler itself must not error.
    let args = DoctorArgs {
        verbose: false,
        fix: false,
    };
    let result = run(&app, &args).await;
    assert!(
        result.is_ok(),
        "doctor handler must not return Err: {result:?}"
    );
}

#[tokio::test]
async fn test_doctor_verbose_no_issues_returns_success() {
    let app = MockAppContext::new();
    let args = DoctorArgs {
        verbose: true,
        fix: false,
    };
    let result = run(&app, &args).await;
    assert!(result.is_ok());
}
