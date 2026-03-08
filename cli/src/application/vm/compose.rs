//! Shared compose helpers used by `workspace_start` and `agent_activate`.
//!
//! These functions manage the active overlay symlink and the ready marker
//! that gates `polis.service` auto-start. They are pure I/O wrappers with
//! no domain logic.

use anyhow::{Context, Result};

use crate::application::ports::ShellExecutor;
use crate::domain::workspace::{ACTIVE_OVERLAY_PATH, READY_MARKER_PATH};

/// Set or remove the active compose overlay symlink.
///
/// # Errors
///
/// Returns an error if the symlink operation fails inside the VM.
pub async fn set_active_overlay(
    provisioner: &impl ShellExecutor,
    overlay_path: Option<&str>,
) -> Result<()> {
    match overlay_path {
        Some(path) => {
            provisioner
                .exec(&["ln", "-sf", path, ACTIVE_OVERLAY_PATH])
                .await
                .context("creating overlay symlink")?;
        }
        None => {
            provisioner
                .exec(&["rm", "-f", ACTIVE_OVERLAY_PATH])
                .await
                .context("removing overlay symlink")?;
        }
    }
    Ok(())
}

/// Set or clear the ready marker that gates `polis.service` auto-start.
///
/// # Errors
///
/// Returns an error if the marker file operation fails inside the VM.
pub async fn set_ready_marker(provisioner: &impl ShellExecutor, enabled: bool) -> Result<()> {
    if enabled {
        provisioner
            .exec(&["touch", READY_MARKER_PATH])
            .await
            .context("creating ready marker")?;
    } else {
        provisioner
            .exec(&["rm", "-f", READY_MARKER_PATH])
            .await
            .context("removing ready marker")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use anyhow::Result;
    use super::*;
    use crate::application::ports::ShellExecutor;
    use crate::application::vm::test_support::{impl_shell_executor_stubs, ok_output};
    use crate::domain::workspace::{ACTIVE_OVERLAY_PATH, READY_MARKER_PATH};

    struct ExecSpy(Mutex<Vec<Vec<String>>>);
    impl ExecSpy {
        fn new() -> Self { Self(Mutex::new(vec![])) }
        fn calls(&self) -> Vec<Vec<String>> { self.0.lock().expect("spy mutex poisoned").clone() }
    }
    impl ShellExecutor for ExecSpy {
        async fn exec(&self, args: &[&str]) -> Result<std::process::Output> {
            self.0.lock().expect("spy mutex poisoned").push(args.iter().map(std::string::ToString::to_string).collect());
            Ok(ok_output(b""))
        }
        impl_shell_executor_stubs!(exec_with_stdin, exec_spawn, exec_status);
    }

    #[tokio::test]
    async fn set_active_overlay_some_runs_ln_sf() {
        let spy = ExecSpy::new();
        set_active_overlay(&spy, Some("/opt/polis/agents/foo/.generated/compose.agent.yaml")).await.expect("set_active_overlay failed");
        let calls = spy.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "ln");
        assert_eq!(calls[0][1], "-sf");
        assert_eq!(calls[0][3], ACTIVE_OVERLAY_PATH);
    }

    #[tokio::test]
    async fn set_active_overlay_none_runs_rm_f() {
        let spy = ExecSpy::new();
        set_active_overlay(&spy, None).await.expect("set_active_overlay failed");
        let calls = spy.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "rm");
        assert_eq!(calls[0][1], "-f");
        assert_eq!(calls[0][2], ACTIVE_OVERLAY_PATH);
    }

    #[tokio::test]
    async fn set_ready_marker_enabled_runs_touch() {
        let spy = ExecSpy::new();
        set_ready_marker(&spy, true).await.expect("set_ready_marker failed");
        let calls = spy.calls();
        assert_eq!(calls[0][0], "touch");
        assert_eq!(calls[0][1], READY_MARKER_PATH);
    }

    #[tokio::test]
    async fn set_ready_marker_disabled_runs_rm_f() {
        let spy = ExecSpy::new();
        set_ready_marker(&spy, false).await.expect("set_ready_marker failed");
        let calls = spy.calls();
        assert_eq!(calls[0][0], "rm");
        assert_eq!(calls[0][1], "-f");
        assert_eq!(calls[0][2], READY_MARKER_PATH);
    }
}
