//! Application service — workspace exec use-case.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use anyhow::Result;
use std::process::ExitStatus;

use crate::application::ports::{ContainerExecutor, InstanceInspector};

/// Execute a command inside the workspace container.
///
/// Validates that the VM is running before attempting execution.
///
/// # Arguments
///
/// * `provisioner` - Provides VM inspection and container execution
/// * `args` - Command and arguments to run inside the container
/// * `interactive` - Whether stdin is a terminal
///
/// # Errors
///
/// Returns `WorkspaceError::NotRunning` if the VM is not in Running state.
/// Returns an error if the container exec command fails.
pub async fn exec(
    provisioner: &(impl InstanceInspector + ContainerExecutor),
    args: &[&str],
    interactive: bool,
) -> Result<ExitStatus> {
    super::ensure_running(provisioner).await?;
    provisioner.container_exec_status(args, interactive).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::process::Output;

    use crate::application::vm::lifecycle::VmState;
    use crate::application::vm::test_support::ok_output;

    /// Stub provisioner that returns a configurable VM state.
    struct VmStateStub {
        state: VmState,
    }

    impl InstanceInspector for VmStateStub {
        async fn info(&self) -> anyhow::Result<Output> {
            // Return JSON that matches the expected state
            let state_str = match self.state {
                VmState::Running => "Running",
                VmState::Stopped => "Stopped",
                VmState::Starting => "Starting",
                VmState::NotFound => {
                    // Return a failed output to simulate VM not found
                    return Ok(Output {
                        status: crate::application::vm::test_support::exit_status(1),
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    });
                }
            };
            let json = format!(r#"{{"info":{{"polis":{{"state":"{state_str}","ipv4":[]}}}}}}"#);
            Ok(ok_output(json.as_bytes()))
        }

        async fn version(&self) -> anyhow::Result<Output> {
            anyhow::bail!("not expected")
        }
    }

    impl ContainerExecutor for VmStateStub {
        async fn container_exec_status(
            &self,
            _args: &[&str],
            _interactive: bool,
        ) -> anyhow::Result<std::process::ExitStatus> {
            // Should not be called when VM is not running
            Ok(crate::application::vm::test_support::exit_status(0))
        }
    }

    /// Strategy to generate non-Running VM states.
    fn non_running_state() -> impl Strategy<Value = VmState> {
        prop_oneof![
            Just(VmState::NotFound),
            Just(VmState::Stopped),
            Just(VmState::Starting),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 2.1, 2.2**
        ///
        /// Property 1: VM State Guard
        ///
        /// For any VM state that is not `Running` (i.e., `NotFound`, `Stopped`,
        /// or `Starting`), when the exec service is invoked, it SHALL return
        /// `WorkspaceError::NotRunning` without attempting container execution.
        #[test]
        fn prop_vm_state_guard_returns_not_running(state in non_running_state()) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");

            let stub = VmStateStub { state };
            let result = rt.block_on(exec(&stub, &["echo", "test"], false));

            // Should return an error
            prop_assert!(result.is_err());

            // The error should be WorkspaceError::NotRunning
            let err = result.expect_err("Expected error for non-Running state");
            let err_msg = err.to_string();
            prop_assert!(
                err_msg.contains("not running"),
                "Expected NotRunning error, got: {err_msg}"
            );
        }
    }

    #[tokio::test]
    async fn exec_succeeds_when_vm_is_running() {
        let stub = VmStateStub {
            state: VmState::Running,
        };
        let result = exec(&stub, &["echo", "test"], false).await;
        assert!(result.is_ok());
    }
}
