//! Application service — workspace stop use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use anyhow::Result;

use crate::application::ports::{
    InstanceInspector, InstanceLifecycle, ProgressReporter, ShellExecutor,
};
use crate::application::vm::lifecycle::{self as vm, VmState};

/// Outcome of the `stop` use-case.
#[derive(Debug, PartialEq, Eq)]
pub enum StopOutcome {
    /// Workspace was stopped successfully.
    Stopped,
    /// Workspace was already stopped.
    AlreadyStopped,
    /// No workspace found.
    NotFound,
}

/// Stop the workspace.
///
/// # Errors
///
/// Returns an error if the stop command fails.
pub async fn stop(
    provisioner: &(impl InstanceInspector + InstanceLifecycle + ShellExecutor),
    reporter: &impl ProgressReporter,
) -> Result<StopOutcome> {
    match vm::state(provisioner).await? {
        VmState::NotFound => Ok(StopOutcome::NotFound),
        VmState::Stopped => Ok(StopOutcome::AlreadyStopped),
        VmState::Running | VmState::Starting => {
            // Clear ready marker so polis.service won't auto-start on next boot.
            let _ = provisioner
                .exec(&["rm", "-f", crate::domain::workspace::READY_MARKER_PATH])
                .await;

            reporter.begin_stage("stopping workspace...");
            vm::stop(provisioner).await?;
            reporter.complete_stage();
            Ok(StopOutcome::Stopped)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::application::ports::{
        InstanceInspector, InstanceLifecycle, InstanceSpec, ShellExecutor,
    };
    use crate::application::vm::test_support::{
        NoopReporter, fail_output, impl_shell_executor_stubs, ok_output,
    };
    use anyhow::Result;
    use std::process::Output;

    struct StopStub {
        info_json: &'static [u8],
        info_success: bool,
        stop_fails: bool,
    }

    impl InstanceInspector for StopStub {
        async fn info(&self) -> Result<Output> {
            if self.info_success {
                Ok(ok_output(self.info_json))
            } else {
                Ok(fail_output())
            }
        }
        async fn version(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }

    impl InstanceLifecycle for StopStub {
        async fn launch(&self, _: &InstanceSpec<'_>) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn start(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn stop(&self) -> Result<Output> {
            if self.stop_fails {
                Ok(fail_output())
            } else {
                Ok(ok_output(b""))
            }
        }
        async fn delete(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn purge(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }

    impl ShellExecutor for StopStub {
        async fn exec(&self, _: &[&str]) -> Result<Output> {
            Ok(ok_output(b""))
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    #[tokio::test]
    async fn stop_not_found() {
        let stub = StopStub {
            info_json: b"",
            info_success: false,
            stop_fails: false,
        };
        assert_eq!(
            stop(&stub, &NoopReporter).await.unwrap(),
            StopOutcome::NotFound
        );
    }

    #[tokio::test]
    async fn stop_already_stopped() {
        let stub = StopStub {
            info_json: br#"{"info":{"polis":{"state":"Stopped","ipv4":[]}}}"#,
            info_success: true,
            stop_fails: false,
        };
        assert_eq!(
            stop(&stub, &NoopReporter).await.unwrap(),
            StopOutcome::AlreadyStopped
        );
    }

    #[tokio::test]
    async fn stop_running_returns_stopped() {
        let stub = StopStub {
            info_json: br#"{"info":{"polis":{"state":"Running","ipv4":[]}}}"#,
            info_success: true,
            stop_fails: false,
        };
        assert_eq!(
            stop(&stub, &NoopReporter).await.unwrap(),
            StopOutcome::Stopped
        );
    }

    #[tokio::test]
    async fn stop_running_stop_fails_returns_error() {
        let stub = StopStub {
            info_json: br#"{"info":{"polis":{"state":"Running","ipv4":[]}}}"#,
            info_success: true,
            stop_fails: true,
        };
        assert!(stop(&stub, &NoopReporter).await.is_err());
    }
}
