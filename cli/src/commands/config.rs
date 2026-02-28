//! `polis config` — show and set configuration values.

use anyhow::Result;
use std::process::ExitCode;

use crate::app::AppContext;
use crate::application::ports::{ConfigStore, InstanceInspector, ShellExecutor};
use crate::application::services::config_service;
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
                if !crate::application::services::config_service::propagate_security_level(&app.provisioner, value).await? {
            app.output.warn("Could not propagate to workspace (is it running?)");
        } else {
            app.output.success("Security level active in workspace");
        }
    }

    Ok(ExitCode::SUCCESS)
}

