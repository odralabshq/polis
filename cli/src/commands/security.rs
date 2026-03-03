//! `polis security` — manage security policy, blocked requests, and domain rules.

use anyhow::{Context, Result};
use clap::Subcommand;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::ports::ShellExecutor;

/// Container name for the toolbox service (runs polis-approve CLI).
const TOOLBOX_CONTAINER: &str = "polis-toolbox";

/// Security subcommands.
#[derive(Subcommand)]
pub enum SecurityCommand {
    /// Show security status (level, pending count, feature health)
    Status,
    /// List pending blocked requests awaiting approval
    Pending,
    /// Approve a blocked request
    Approve {
        /// Request ID to approve (format: req-[a-f0-9]{8})
        request_id: String,
    },
    /// Deny a blocked request
    Deny {
        /// Request ID to deny (format: req-[a-f0-9]{8})
        request_id: String,
    },
    /// Show recent security events
    Log,
    /// Auto-approve a domain pattern
    Allow {
        /// Domain pattern to allow (e.g. "cli.kiro.dev" or "*.example.com")
        pattern: String,
        /// Action: allow (default), prompt, or block
        #[arg(long, default_value = "allow")]
        action: String,
    },
    /// Set the security level
    Level {
        /// Security level: relaxed, balanced, or strict
        level: String,
    },
}

/// Run a security command.
///
/// # Errors
///
/// Returns an error if the underlying operations fail.
pub async fn run(
    cmd: SecurityCommand,
    app: &AppContext,
    mp: &impl ShellExecutor,
) -> Result<ExitCode> {
    match cmd {
        SecurityCommand::Status => status(app, mp).await,
        SecurityCommand::Pending => pending(app, mp).await,
        SecurityCommand::Approve { request_id } => approve(app, mp, &request_id).await,
        SecurityCommand::Deny { request_id } => deny(app, mp, &request_id).await,
        SecurityCommand::Log => log(app, mp).await,
        SecurityCommand::Allow { pattern, action } => allow(app, mp, &pattern, &action).await,
        SecurityCommand::Level { level } => set_level(app, mp, &level).await,
    }
}

/// Run polis-approve inside the toolbox container and capture output.
///
/// Reads the mcp-admin password from the mounted Docker secret and injects it
/// as `polis_VALKEY_PASS` so `polis-approve` can authenticate to Valkey.
async fn toolbox_approve(mp: &impl ShellExecutor, args: &[&str]) -> Result<String> {
    // Read the mcp-admin password from the mounted secret
    let pass_output = mp
        .exec(&[
            "docker", "exec", TOOLBOX_CONTAINER,
            "cat", "/run/secrets/valkey_mcp_admin_password",
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

    let pass = String::from_utf8_lossy(&pass_output.stdout).trim().to_string();
    let env_arg = format!("polis_VALKEY_PASS={pass}");

    let mut cmd: Vec<&str> = vec![
        "docker", "exec",
        "-e", &env_arg,
        TOOLBOX_CONTAINER, "polis-approve",
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

async fn status(app: &AppContext, mp: &impl ShellExecutor) -> Result<ExitCode> {
    // Get security level from local config
    let config =
        crate::application::services::config_service::load_config(&app.config_store)?;
    app.output
        .info(&format!("Security level: {}", config.security.level));

    // Get pending count from toolbox
    match toolbox_approve(mp, &["list-pending"]).await {
        Ok(output) => {
            let trimmed = output.trim();
            if trimmed == "no pending requests" {
                app.output.success("No pending blocked requests");
            } else {
                let count = trimmed.lines().count();
                app.output
                    .warn(&format!("{count} pending blocked request(s)"));
            }
        }
        Err(e) => {
            app.output
                .warn(&format!("Could not query pending requests: {e}"));
        }
    }

    Ok(ExitCode::SUCCESS)
}

async fn pending(app: &AppContext, mp: &impl ShellExecutor) -> Result<ExitCode> {
    let output = toolbox_approve(mp, &["list-pending"]).await?;
    let trimmed = output.trim();

    if trimmed == "no pending requests" {
        app.output.success("No pending blocked requests");
    } else {
        for line in trimmed.lines() {
            app.output.info(line);
        }
    }

    Ok(ExitCode::SUCCESS)
}

async fn approve(app: &AppContext, mp: &impl ShellExecutor, request_id: &str) -> Result<ExitCode> {
    let output = toolbox_approve(mp, &["approve", request_id]).await?;
    app.output.success(output.trim());
    Ok(ExitCode::SUCCESS)
}

async fn deny(app: &AppContext, mp: &impl ShellExecutor, request_id: &str) -> Result<ExitCode> {
    let output = toolbox_approve(mp, &["deny", request_id]).await?;
    app.output.success(output.trim());
    Ok(ExitCode::SUCCESS)
}

async fn log(_app: &AppContext, mp: &impl ShellExecutor) -> Result<ExitCode> {
    // Query the Valkey event log via the state container
    let output = mp
        .exec(&[
            "docker", "exec", "polis-state", "sh", "-c",
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
        println!("No recent security events");
    } else {
        for line in trimmed.lines() {
            println!("{line}");
        }
    }

    Ok(ExitCode::SUCCESS)
}

async fn allow(
    app: &AppContext,
    mp: &impl ShellExecutor,
    pattern: &str,
    action: &str,
) -> Result<ExitCode> {
    let output = toolbox_approve(mp, &["auto-approve", pattern, action]).await?;
    app.output.success(output.trim());
    Ok(ExitCode::SUCCESS)
}

async fn set_level(app: &AppContext, mp: &impl ShellExecutor, level: &str) -> Result<ExitCode> {
    // Validate level
    match level {
        "relaxed" | "balanced" | "strict" => {}
        _ => anyhow::bail!("Invalid security level '{level}': expected relaxed, balanced, or strict"),
    }

    // Set via polis-approve (updates Valkey directly)
    let output = toolbox_approve(mp, &["set-security-level", level]).await?;
    app.output.success(output.trim());

    // Also update local config to keep it in sync
    let mut config =
        crate::application::services::config_service::load_config(&app.config_store)?;
    config.security.level = level.to_string();
    crate::application::services::config_service::save_config(&app.config_store, &config)?;

    Ok(ExitCode::SUCCESS)
}
