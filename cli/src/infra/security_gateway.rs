//! Infrastructure implementation of the `SecurityGateway` port.
//!
//! Executes `polis-approve` commands inside the toolbox container via Docker exec.
//! Handles password retrieval, environment setup, and toolbox output parsing.

use anyhow::{Context, Result};

use crate::application::ports::{SecurityGateway, ShellExecutor};
use crate::domain::security::{AllowAction, SecurityLevel};

/// Container name for the toolbox service (runs polis-approve CLI).
const TOOLBOX_CONTAINER: &str = "polis-toolbox";

/// Production implementation of `SecurityGateway` using Docker exec
/// to run commands in the toolbox container.
pub struct ToolboxSecurityGateway<'a, E: ShellExecutor> {
    executor: &'a E,
}

impl<'a, E: ShellExecutor> ToolboxSecurityGateway<'a, E> {
    pub fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

/// Executes arbitrary commands in the toolbox container.
///
/// Reads the mcp-admin password from the mounted Docker secret and injects it
/// as `VALKEY_MCP_ADMIN_PASSWORD` so `polis-approve` can authenticate to Valkey.
///
/// Memory efficiency:
/// - Password bytes are formatted directly into the env var string (single allocation)
/// - Uses `String::from_utf8` with explicit error on invalid UTF-8
/// - stderr is only allocated on the failure path
///
/// # Errors
///
/// Returns an error if the toolbox container is not running, credentials are missing,
/// the command fails, or the output contains invalid UTF-8.
pub async fn exec_in_toolbox(executor: &impl ShellExecutor, args: &[&str]) -> Result<String> {
    let pass_output = executor
        .exec(&[
            "docker",
            "exec",
            TOOLBOX_CONTAINER,
            "cat",
            "/run/secrets/valkey_mcp_admin_password",
        ])
        .await
        .context("failed to read mcp-admin password from toolbox container")?;

    if !pass_output.status.success() {
        let stderr = String::from_utf8_lossy(&pass_output.stderr);
        if stderr.contains("No such container") {
            anyhow::bail!(
                "Toolbox container is not running. Start the workspace first: polis start"
            );
        }
        anyhow::bail!(
            "Toolbox container missing Valkey credentials. Try restarting: polis stop && polis start"
        );
    }

    // Convert password bytes to string — explicit error on invalid UTF-8 (not lossy)
    let password = String::from_utf8(pass_output.stdout)
        .context("non-UTF-8 output from polis-approve: password contains invalid UTF-8")?;

    // Format env var as single string: one allocation, not three
    let pass_env = format!("VALKEY_MCP_ADMIN_PASSWORD={}", password.trim());

    let mut cmd: Vec<&str> = vec![
        "docker",
        "exec",
        "-e",
        &pass_env,
        TOOLBOX_CONTAINER,
        "polis-approve",
    ];
    cmd.extend_from_slice(args);

    let output = executor
        .exec(&cmd)
        .await
        .context("failed to exec polis-approve in toolbox container")?;

    if !output.status.success() {
        // Only allocate stderr on the failure path
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        anyhow::bail!("polis-approve failed: {}", msg.trim());
    }

    // On success: use String::from_utf8 (explicit error, not lossy)
    String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("non-UTF-8 output from polis-approve: {e}"))
}

impl<E: ShellExecutor> SecurityGateway for ToolboxSecurityGateway<'_, E> {
    async fn list_pending(&self) -> Result<Vec<String>> {
        let output = exec_in_toolbox(self.executor, &["list"]).await?;
        let trimmed = output.trim();

        // Handle toolbox sentinel value for empty queue (Req 28.1, 28.2)
        if trimmed == "no pending requests" || trimmed.is_empty() {
            return Ok(vec![]);
        }

        Ok(trimmed.lines().map(ToString::to_string).collect())
    }

    async fn approve(&self, request_id: &str) -> Result<String> {
        let output = exec_in_toolbox(self.executor, &["approve", request_id]).await?;
        Ok(output.trim().to_string())
    }

    async fn deny(&self, request_id: &str) -> Result<String> {
        let output = exec_in_toolbox(self.executor, &["deny", request_id]).await?;
        Ok(output.trim().to_string())
    }

    async fn set_level(&self, level: SecurityLevel) -> Result<String> {
        let level_str = level.to_string();
        let output = exec_in_toolbox(self.executor, &["set-level", &level_str]).await?;
        Ok(output.trim().to_string())
    }

    async fn add_domain_rule(&self, pattern: &str, action: AllowAction) -> Result<String> {
        let action_str = action.to_string();
        let output = exec_in_toolbox(
            self.executor,
            &["add-rule", pattern, "--action", &action_str],
        )
        .await?;
        Ok(output.trim().to_string())
    }

    async fn get_log(&self) -> Result<Vec<String>> {
        let output = exec_in_toolbox(self.executor, &["list-events"]).await?;
        let trimmed = output.trim();

        if trimmed.is_empty() {
            return Ok(vec![]);
        }

        Ok(trimmed.lines().map(ToString::to_string).collect())
    }
}
