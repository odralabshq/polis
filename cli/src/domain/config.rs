//! Domain types and validators for Polis configuration.
//!
//! Pure functions only — no I/O, no async, no filesystem access.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::domain::error::ConfigError;
use crate::domain::security::SecurityLevel;

// ── Constants ────────────────────────────────────────────────────────────────

pub const VALID_CONFIG_KEYS: &[&str] =
    &["security.level", "control_plane.url", "control_plane.token"];

// ── Config schema ────────────────────────────────────────────────────────────

/// Top-level configuration stored in `~/.polis/config.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PolisConfig {
    /// Security settings.
    #[serde(default)]
    pub security: SecurityConfig,
    /// Control-plane connection settings.
    #[serde(default)]
    pub control_plane: ControlPlaneConfig,
}

/// Security policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    /// Security level: relaxed, balanced (default), or strict.
    #[serde(default)]
    pub level: SecurityLevel,
}

/// Control-plane connection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlPlaneConfig {
    /// Base URL of the control-plane HTTP API (e.g. `http://10.30.1.2:8090`).
    #[serde(default = "default_control_plane_url")]
    pub url: String,
    /// Optional bearer token for authenticated requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl Default for ControlPlaneConfig {
    fn default() -> Self {
        Self {
            url: default_control_plane_url(),
            token: None,
        }
    }
}

fn default_control_plane_url() -> String {
    "http://127.0.0.1:8090".to_string()
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

/// Validates a configuration value for a given key.
///
/// # Errors
///
/// Returns an error if the value is not valid for the given key.
pub fn validate_config_value(key: &str, value: &str) -> Result<()> {
    match key {
        "security.level" => {
            if !matches!(value, "relaxed" | "balanced" | "strict") {
                return Err(ConfigError::InvalidValue {
                    key: key.to_string(),
                    value: value.to_string(),
                    valid: "relaxed, balanced, strict".to_string(),
                }
                .into());
            }
        }
        "control_plane.url" => {
            if value.is_empty() {
                return Err(ConfigError::InvalidValue {
                    key: key.to_string(),
                    value: value.to_string(),
                    valid: "non-empty URL".to_string(),
                }
                .into());
            }
        }
        "control_plane.token" => { /* any non-empty string is valid */ }
        _ => {
            validate_config_key(key)?;
        }
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
        assert_eq!(cfg.security.level, SecurityLevel::Balanced);
    }

    #[test]
    fn test_polis_config_deserialize_full_yaml() {
        let yaml = "security:\n  level: strict\n";
        let cfg: PolisConfig = serde_yaml_ng::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.security.level, SecurityLevel::Strict);
    }

    #[test]
    fn test_polis_config_deserialize_empty_yaml_uses_defaults() {
        let cfg: PolisConfig = serde_yaml_ng::from_str("{}").expect("empty yaml");
        assert_eq!(cfg.security.level, SecurityLevel::Balanced);
    }

    #[test]
    fn test_polis_config_deserialize_ignores_unknown_fields() {
        // Old config files may have defaults.agent - should be silently ignored
        let yaml = "security:\n  level: strict\ndefaults:\n  agent: claude-dev\n";
        let cfg: PolisConfig = serde_yaml_ng::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.security.level, SecurityLevel::Strict);
    }

    #[test]
    fn test_polis_config_serialize_deserialize_roundtrip() {
        let mut cfg = PolisConfig::default();
        cfg.security.level = SecurityLevel::Strict;

        let yaml = serde_yaml_ng::to_string(&cfg).expect("serialize");
        let back: PolisConfig = serde_yaml_ng::from_str(&yaml).expect("deserialize");

        assert_eq!(back.security.level, SecurityLevel::Strict);
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
}
