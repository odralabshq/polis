//! Application service — security management use-cases.
//!
//! Wraps `polis-approve` inside the toolbox container to manage blocked
//! requests, domain rules, and security levels via Valkey.

use anyhow::{Context, Result};

use crate::application::ports::{ConfigStore, ShellExecutor};

/// Container name for the toolbox service (runs polis-approve CLI).
const TOOLBOX_CONTAINER: &str = "polis-toolbox";

/// Run polis-approve inside the toolbox container and capture output.
///
/// Reads the mcp-admin password from the mounted Docker secret and injects it
/// as `polis_VALKEY_PASS` so `polis-approve` can authenticate to Valkey.
async fn toolbox_approve(mp: &impl ShellExecutor, args: &[&str]) -> Result<String> {
    let pass_output = mp
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

    let pass = String::from_utf8_lossy(&pass_output.stdout)
        .trim()
        .to_string();
    let pass_env = format!("polis_VALKEY_PASS={pass}");

    let mut cmd: Vec<&str> = vec![
        "docker",
        "exec",
        "-e",
        &pass_env,
        TOOLBOX_CONTAINER,
        "polis-approve",
    ];
    cmd.extend_from_slice(args);

    let output = mp
        .exec(&cmd)
        .await
        .context("failed to exec polis-approve in toolbox container")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let msg = if stderr.is_empty() { &stdout } else { &stderr };
        anyhow::bail!("polis-approve failed: {}", msg.trim());
    }

    Ok(stdout)
}

/// Result of a security status query.
pub struct SecurityStatus {
    /// Current security level from local config.
    pub level: String,
    /// Pending request lines (empty if none).
    pub pending_lines: Vec<String>,
    /// Error message if pending query failed.
    pub pending_error: Option<String>,
}

/// Query security status: level + pending count.
///
/// # Errors
///
/// Returns an error if the local config cannot be loaded.
pub async fn get_status(
    store: &impl ConfigStore,
    mp: &impl ShellExecutor,
) -> Result<SecurityStatus> {
    let config = crate::application::services::config_service::load_config(store)?;
    let level = config.security.level;

    let (pending_lines, pending_error) = match toolbox_approve(mp, &["list-pending"]).await {
        Ok(output) => {
            let trimmed = output.trim().to_string();
            if trimmed == "no pending requests" {
                (vec![], None)
            } else {
                (trimmed.lines().map(String::from).collect(), None)
            }
        }
        Err(e) => (vec![], Some(format!("{e}"))),
    };

    Ok(SecurityStatus {
        level,
        pending_lines,
        pending_error,
    })
}

/// List pending blocked requests. Returns lines of output.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or polis-approve fails.
pub async fn list_pending(mp: &impl ShellExecutor) -> Result<Vec<String>> {
    let output = toolbox_approve(mp, &["list-pending"]).await?;
    let trimmed = output.trim();
    if trimmed == "no pending requests" {
        Ok(vec![])
    } else {
        Ok(trimmed.lines().map(String::from).collect())
    }
}

/// Approve a blocked request. Returns confirmation message.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or the request ID is invalid.
pub async fn approve(mp: &impl ShellExecutor, request_id: &str) -> Result<String> {
    let output = toolbox_approve(mp, &["approve", request_id]).await?;
    Ok(output.trim().to_string())
}

/// Deny a blocked request. Returns confirmation message.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or the request ID is invalid.
pub async fn deny(mp: &impl ShellExecutor, request_id: &str) -> Result<String> {
    let output = toolbox_approve(mp, &["deny", request_id]).await?;
    Ok(output.trim().to_string())
}

/// Query recent security events from the Valkey event log.
///
/// # Errors
///
/// Returns an error if the state container is unreachable.
pub async fn get_log(mp: &impl ShellExecutor) -> Result<Vec<String>> {
    let output = mp
        .exec(&[
            "docker",
            "exec",
            "polis-state",
            "sh",
            "-c",
            "REDISCLI_AUTH=$(cat /run/secrets/valkey_mcp_admin_password) \
             valkey-cli --tls \
             --cert /etc/valkey/tls/client.pem \
             --key /etc/valkey/tls/client.key \
             --cacert /etc/valkey/tls/ca.crt \
             --user mcp-admin --no-auth-warning \
             ZREVRANGEBYSCORE polis:log:events +inf -inf LIMIT 0 20",
        ])
        .await
        .context("failed to query security log")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed.is_empty() || trimmed == "(empty array)" || trimmed == "(empty list or set)" {
        Ok(vec![])
    } else {
        Ok(trimmed.lines().map(String::from).collect())
    }
}

/// Set an auto-approve rule. Returns confirmation message.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or the pattern/action is invalid.
pub async fn auto_allow(mp: &impl ShellExecutor, pattern: &str, action: &str) -> Result<String> {
    let output = toolbox_approve(mp, &["auto-approve", pattern, action]).await?;
    Ok(output.trim().to_string())
}

/// Set the security level (validates, updates Valkey + local config).
///
/// # Errors
///
/// Returns an error if the level is invalid, the toolbox is unreachable, or config save fails.
pub async fn set_level(
    store: &impl ConfigStore,
    mp: &impl ShellExecutor,
    level: &str,
) -> Result<String> {
    match level {
        "relaxed" | "balanced" | "strict" => {}
        _ => {
            anyhow::bail!("Invalid security level '{level}': expected relaxed, balanced, or strict")
        }
    }

    let output = toolbox_approve(mp, &["set-security-level", level]).await?;

    let mut config = crate::application::services::config_service::load_config(store)?;
    config.security.level = level.to_string();
    crate::application::services::config_service::save_config(store, &config)?;

    Ok(output.trim().to_string())
}
