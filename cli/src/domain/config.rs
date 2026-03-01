//! Domain types and validators for Polis configuration.
//!
//! Pure functions only — no I/O, no async, no filesystem access.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::domain::error::ConfigError;

// ── Constants ────────────────────────────────────────────────────────────────

pub const VALID_CONFIG_KEYS: &[&str] = &["security.level"];
pub const VALID_SECURITY_LEVELS: &[&str] = &["relaxed", "balanced", "strict"];

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
    /// Security level: `relaxed`, `balanced` (default), or `strict`.
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

// ── Validators ───────────────────────────────────────────────────────────────

/// Validates a configuration key against the whitelist.
///
/// # Errors
///
/// Returns an error if the key is not in the allowed list.
pub fn validate_config_key(key: &str) -> Result<()> {
    if !VALID_CONFIG_KEYS.contains(&key) {
        return Err(ConfigError::UnknownKey {
            key: key.to_string(),
            valid: VALID_CONFIG_KEYS.join(", "),
        }
        .into());
    }
    Ok(())
}

/// Validates a configuration value for the given key.
///
/// # Errors
///
/// Returns an error if the value is not valid for the key.
pub fn validate_config_value(key: &str, value: &str) -> Result<()> {
    if key == "security.level" && !VALID_SECURITY_LEVELS.contains(&value) {
        return Err(ConfigError::InvalidValue {
            key: key.to_string(),
            value: value.to_string(),
            valid: VALID_SECURITY_LEVELS.join(", "),
        }
        .into());
    }
    Ok(())
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

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
    fn test_polis_config_deserialize_ignores_unknown_fields() {
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
    fn test_validate_config_value_relaxed_ok() {
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
}
