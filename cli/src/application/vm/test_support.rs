//! Shared test helpers for VM service tests.
//!
//! Provides cross-platform `exit_status()` and a macro to generate
//! `ShellExecutor` stub methods that bail with "not expected".

/// Build an `ExitStatus` from a logical exit code (cross-platform).
#[cfg(unix)]
pub fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
pub fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    #[allow(clippy::cast_sign_loss)]
    std::process::ExitStatus::from_raw(code as u32)
}

pub fn ok_output(stdout: &[u8]) -> std::process::Output {
    std::process::Output {
        status: exit_status(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

pub fn fail_output() -> std::process::Output {
    std::process::Output {
        status: exit_status(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

/// Generate `ShellExecutor` stub methods that bail with "not expected".
///
/// Usage: `impl_shell_executor_stubs!(exec, exec_with_stdin, exec_spawn, exec_status);`
/// Omit any method you implement yourself.
macro_rules! impl_shell_executor_stubs {
    ($($method:ident),* $(,)?) => {
        $(impl_shell_executor_stubs!(@one $method);)*
    };
    (@one exec) => {
        /// # Errors
        /// Stub — always bails.
        async fn exec(&self, _: &[&str]) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("not expected")
        }
    };
    (@one exec_with_stdin) => {
        /// # Errors
        /// Stub — always bails.
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> anyhow::Result<std::process::Output> {
            anyhow::bail!("not expected")
        }
    };
    (@one exec_spawn) => {
        /// # Errors
        /// Stub — always bails.
        fn exec_spawn(&self, _: &[&str]) -> anyhow::Result<tokio::process::Child> {
            anyhow::bail!("not expected")
        }
    };
    (@one exec_status) => {
        /// # Errors
        /// Stub — always bails.
        async fn exec_status(&self, _: &[&str]) -> anyhow::Result<std::process::ExitStatus> {
            anyhow::bail!("not expected")
        }
    };
}

pub(crate) use impl_shell_executor_stubs;
