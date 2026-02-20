//! `polis config` — show and set configuration values.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};

use crate::output::OutputContext;

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

// ── Config schema ────────────────────────────────────────────────────────────

/// Top-level configuration stored in `~/.polis/config.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PolisConfig {
    /// Security settings.
    #[serde(default)]
    pub security: SecurityConfig,
}

/// Security configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Security level: `balanced` (default) or `strict`.
    #[serde(default = "default_security_level")]
    pub level: String,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            level: default_security_level(),
        }
    }
}

fn default_security_level() -> String {
    "balanced".to_string()
}

// ── Entry point ──────────────────────────────────────────────────────────────

/// Run the config command.
///
/// # Errors
///
/// Returns an error if the config file cannot be read or written, or if
/// the key or value fails validation.
pub fn run(ctx: &OutputContext, cmd: ConfigCommand) -> Result<()> {
    match cmd {
        ConfigCommand::Show => show_config(ctx),
        ConfigCommand::Set { key, value } => set_config(ctx, &key, &value),
    }
}

// ── Subcommand handlers ──────────────────────────────────────────────────────

fn show_config(ctx: &OutputContext) -> Result<()> {
    let path = get_config_path()?;
    let config = load_config(&path)?;

    println!();
    println!(
        "  {}",
        format!("Configuration ({})", path.display()).style(ctx.styles.header)
    );
    println!();
    println!("  {:<20} {}", "security.level:", config.security.level);
    println!();
    println!("  {}", "Environment:".style(ctx.styles.bold));
    println!(
        "    {:<18} {}",
        "POLIS_CONFIG:",
        std::env::var("POLIS_CONFIG").unwrap_or_else(|_| "(not set)".to_string())
    );
    println!(
        "    {:<18} {}",
        "NO_COLOR:",
        std::env::var("NO_COLOR").unwrap_or_else(|_| "(not set)".to_string())
    );
    println!();
    Ok(())
}

fn set_config(ctx: &OutputContext, key: &str, value: &str) -> Result<()> {
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
    println!();
    Ok(())
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

fn validate_config_key(key: &str) -> Result<()> {
    const VALID: &[&str] = &["security.level"];
    if !VALID.contains(&key) {
        anyhow::bail!(
            "Unknown setting: {key}\n\nValid settings: {}",
            VALID.join(", ")
        );
    }
    Ok(())
}

fn validate_config_value(key: &str, value: &str) -> Result<()> {
    if key == "security.level" {
        const VALID: &[&str] = &["balanced", "strict"];
        if !VALID.contains(&value) {
            anyhow::bail!(
                "Invalid value for security.level: {value}\n\nValid values: {}",
                VALID.join(", ")
            );
        }
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
        assert!(validate_config_value("security.level", "relaxed").is_err());
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
