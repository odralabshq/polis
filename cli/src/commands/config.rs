//! `polis config` — show and set configuration values.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use owo_colors::OwoColorize;

use crate::app::AppContext;
use crate::application::ports::{InstanceInspector, ShellExecutor};
use crate::output::OutputContext;

// ── Re-exports from domain (backward compatibility) ──────────────────────────

#[allow(unused_imports)]
pub use crate::domain::config::{
    PolisConfig, SecurityConfig, validate_config_key, validate_config_value,
};

// ── Subcommand enum ──────────────────────────────────────────────────────────

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

// ── Entry point ──────────────────────────────────────────────────────────────

/// Run the config command.
///
/// # Errors
///
/// Returns an error if the config file cannot be read or written, or if
/// the key or value fails validation.
pub async fn run(
    app: &AppContext,
    cmd: ConfigCommand,
    mp: &(impl InstanceInspector + ShellExecutor),
) -> Result<()> {
    match cmd {
        ConfigCommand::Show => show_config(app),
        ConfigCommand::Set { key, value } => set_config(&app.output, &key, &value, mp).await,
    }
}

// ── Subcommand handlers ──────────────────────────────────────────────────────

fn show_config(app: &AppContext) -> Result<()> {
    let path = get_config_path()?;
    let config = load_config(&path)?;
    app.renderer().render_config(&config, &path)
}

/// Path to the mcp-admin password on the VM filesystem.
const VM_MCP_ADMIN_PASS: &str = "/opt/polis/secrets/valkey_mcp_admin_password.txt";

async fn set_config(
    ctx: &OutputContext,
    key: &str,
    value: &str,
    mp: &(impl InstanceInspector + ShellExecutor),
) -> Result<()> {
    validate_config_key(key)?;
    validate_config_value(key, value)?;

    let path = get_config_path()?;
    let mut config = load_config(&path)?;

    match key {
        "security.level" => config.security.level = value.to_string(),
        _ => anyhow::bail!("Unknown setting: {key}"),
    }

    save_config(&path, &config)?;

    println!();
    println!(
        "  {} Set {} = {}",
        "✓".style(ctx.styles.success),
        key,
        value
    );

    if key == "security.level" {
        propagate_security_level(ctx, value, mp).await;
    }

    println!();
    Ok(())
}

/// Best-effort propagation of `security.level` to Valkey via the state container.
///
/// Reads the mcp-admin password from the VM filesystem, then passes it via
/// `REDISCLI_AUTH` env var to avoid command-line exposure (visible in ps/proc).
///
/// Warns on failure instead of returning an error — the local config is already saved.
async fn propagate_security_level(
    ctx: &OutputContext,
    level: &str,
    mp: &(impl InstanceInspector + ShellExecutor),
) {
    // Fast check: skip if VM is not running (vm_info returns immediately)
    if crate::application::services::vm::lifecycle::state(mp).await.ok() != Some(crate::application::services::vm::lifecycle::VmState::Running) {
        return;
    }
    let pass = match mp.exec(&["cat", VM_MCP_ADMIN_PASS]).await {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => {
            eprintln!(
                "  {} Could not propagate to workspace (is it running?)",
                "⚠".style(ctx.styles.warning),
            );
            return;
        }
    };

    // Pass password via REDISCLI_AUTH env var instead of -a flag to avoid
    // exposing it in process list (ps aux, /proc/*/cmdline)
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
            println!(
                "  {} Security level active in workspace",
                "✓".style(ctx.styles.success),
            );
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "  {} Could not propagate to workspace (is it running?): {}",
                "⚠".style(ctx.styles.warning),
                stderr.trim()
            );
        }
        Err(e) => {
            eprintln!(
                "  {} Could not propagate to workspace (is it running?): {e}",
                "⚠".style(ctx.styles.warning),
            );
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Get the config file path, respecting `POLIS_CONFIG` env var.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub fn get_config_path() -> Result<PathBuf> {
    if let Ok(val) = std::env::var("POLIS_CONFIG") {
        return Ok(PathBuf::from(val));
    }
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    Ok(home.join(".polis").join("config.yaml"))
}

/// Load config from the given path, returning defaults if file doesn't exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load_config(path: &Path) -> Result<PolisConfig> {
    if !path.exists() {
        return Ok(PolisConfig::default());
    }
    let content =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_yaml::from_str(&content).with_context(|| format!("cannot parse {}", path.display()))
}

fn save_config(path: &Path, config: &PolisConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let content = serde_yaml::to_string(config).context("cannot serialize config")?;
    std::fs::write(path, content).with_context(|| format!("cannot write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("cannot set permissions on {}", path.display()))?;
    }
    Ok(())
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── PolisConfig serde ────────────────────────────────────────────────────

    #[test]
    fn test_polis_config_default_security_level_is_balanced() {
        let cfg = PolisConfig::default();
        assert_eq!(cfg.security.level, "balanced");
    }

    #[test]
    fn test_polis_config_deserialize_full_yaml() {
        let yaml = "security:\n  level: strict\n";
        let cfg: PolisConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.security.level, "strict");
    }

    #[test]
    fn test_polis_config_deserialize_empty_yaml_uses_defaults() {
        let cfg: PolisConfig = serde_yaml::from_str("{}").expect("empty yaml");
        assert_eq!(cfg.security.level, "balanced");
    }

    #[test]
    fn test_polis_config_deserialize_ignores_defaults_agent() {
        // Old config files may have defaults.agent - should be silently ignored
        let yaml = "security:\n  level: strict\ndefaults:\n  agent: claude-dev\n";
        let cfg: PolisConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.security.level, "strict");
    }

    #[test]
    fn test_polis_config_serialize_deserialize_roundtrip() {
        let mut cfg = PolisConfig::default();
        cfg.security.level = "strict".to_string();

        let yaml = serde_yaml::to_string(&cfg).expect("serialize");
        let back: PolisConfig = serde_yaml::from_str(&yaml).expect("deserialize");

        assert_eq!(back.security.level, "strict");
    }

    // ── validate_config_key ──────────────────────────────────────────────────

    #[test]
    fn test_validate_config_key_security_level_ok() {
        assert!(validate_config_key("security.level").is_ok());
    }

    #[test]
    fn test_validate_config_key_defaults_agent_rejected() {
        let err = validate_config_key("defaults.agent").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Unknown setting"), "got: {msg}");
    }

    #[test]
    fn test_validate_config_key_unknown_returns_error() {
        let err = validate_config_key("unknown.key").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Unknown setting"), "got: {msg}");
    }

    #[test]
    fn test_validate_config_key_error_lists_valid_keys() {
        let err = validate_config_key("bad").unwrap_err().to_string();
        assert!(err.contains("security.level"), "got: {err}");
    }

    #[test]
    fn test_validate_config_key_empty_string_returns_error() {
        assert!(validate_config_key("").is_err());
    }

    // ── validate_config_value ────────────────────────────────────────────────

    #[test]
    fn test_validate_config_value_balanced_ok() {
        assert!(validate_config_value("security.level", "balanced").is_ok());
    }

    #[test]
    fn test_validate_config_value_strict_ok() {
        assert!(validate_config_value("security.level", "strict").is_ok());
    }

    #[test]
    fn test_validate_config_value_relaxed_returns_error() {
        assert!(validate_config_value("security.level", "relaxed").is_ok());
    }

    #[test]
    fn test_validate_config_value_invalid_level_error_lists_valid_values() {
        let err = validate_config_value("security.level", "permissive")
            .unwrap_err()
            .to_string();
        assert!(err.contains("balanced"), "got: {err}");
        assert!(err.contains("strict"), "got: {err}");
    }

    // ── load_config / save_config ────────────────────────────────────────────

    #[test]
    fn test_load_config_missing_file_returns_default() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nonexistent.yaml");
        let cfg = load_config(&path).expect("missing file should return default");
        assert_eq!(cfg.security.level, "balanced");
    }

    #[test]
    fn test_load_config_corrupt_yaml_returns_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, b"{ not: valid: yaml: [[[").expect("write");
        assert!(load_config(&path).is_err());
    }

    #[test]
    fn test_save_config_creates_parent_directory() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nested").join("dir").join("config.yaml");
        let cfg = PolisConfig::default();
        save_config(&path, &cfg).expect("save should create dirs");
        assert!(path.exists());
    }

    #[test]
    #[cfg(unix)]
    fn test_save_config_sets_0o600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config.yaml");
        save_config(&path, &PolisConfig::default()).expect("save");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "expected 0o600, got {mode:o}");
    }

    #[test]
    fn test_save_then_load_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config.yaml");
        let mut cfg = PolisConfig::default();
        cfg.security.level = "strict".to_string();

        save_config(&path, &cfg).expect("save");
        let loaded = load_config(&path).expect("load");

        assert_eq!(loaded.security.level, "strict");
    }
}
