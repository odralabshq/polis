//! `polis config` — show and set configuration values.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::ports::{ConfigStore, InstanceInspector, ShellExecutor};
use crate::application::services::config_service;
use crate::application::services::vm::lifecycle as vm;
use crate::domain::config::{validate_config_key, validate_config_value};

use clap::Subcommand;

/// Config subcommands.
#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Show current configuration
    Show,
    /// Set configuration value
    Set {
        /// Configuration key
        key: String,
        /// Configuration value
        value: String,
    },
}

/// Run the config command.
pub async fn run(
    app: &AppContext,
    cmd: ConfigCommand,
    _mp: &(impl InstanceInspector + ShellExecutor),
) -> Result<ExitCode> {
    match cmd {
        ConfigCommand::Show => show_config(app),
        ConfigCommand::Set { key, value } => set_config(app, &key, &value).await,
    }
}

fn show_config(app: &AppContext) -> Result<ExitCode> {
    let config = config_service::load_config(&app.config_store)?;
    let path = app.config_store.path()?;
    app.renderer().render_config(&config, &path)?;
    Ok(ExitCode::SUCCESS)
}

/// Path to the mcp-admin password on the VM filesystem.
const VM_MCP_ADMIN_PASS: &str = "/opt/polis/secrets/valkey_mcp_admin_password.txt";

async fn set_config(app: &AppContext, key: &str, value: &str) -> Result<ExitCode> {
    validate_config_key(key)?;
    validate_config_value(key, value)?;

    let mut config = config_service::load_config(&app.config_store)?;

    match key {
        "security.level" => config.security.level = value.to_string(),
        _ => anyhow::bail!("Unknown setting: {key}"),
    }

    config_service::save_config(&app.config_store, &config)?;

    app.output.success(&format!("Set {key} = {value}"));

    if key == "security.level" {
        propagate_security_level(app, value).await?;
    }

    Ok(ExitCode::SUCCESS)
}

async fn propagate_security_level(app: &AppContext, level: &str) -> Result<()> {
    let mp = &app.provisioner;

    // Fast check: skip if VM is not running
    if vm::state(mp).await.ok() != Some(vm::VmState::Running) {
        app.output
            .warn("Could not propagate to workspace (is it running?)");
        return Ok(());
    }
    let pass = match mp.exec(&["cat", VM_MCP_ADMIN_PASS]).await {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => {
            app.output
                .warn("Could not propagate to workspace (is it running?)");
            return Ok(());
        }
    };

    let env_arg = format!("REDISCLI_AUTH={pass}");
    match mp
        .exec(&[
            "docker",
            "exec",
            "-e",
            &env_arg,
            "polis-state",
            "valkey-cli",
            "--tls",
            "--cert",
            "/etc/valkey/tls/client.crt",
            "--key",
            "/etc/valkey/tls/client.key",
            "--cacert",
            "/etc/valkey/tls/ca.crt",
            "--user",
            "mcp-admin",
            "SET",
            "polis:config:security_level",
            level,
        ])
        .await
    {
        Ok(output) if output.status.success() => {
            app.output.success("Security level active in workspace");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            app.output.warn(&format!(
                "Could not propagate to workspace: {}",
                stderr.trim()
            ));
        }
        Err(e) => {
            app.output
                .warn(&format!("Could not propagate to workspace: {e}"));
        }
    }
    Ok(())
}
