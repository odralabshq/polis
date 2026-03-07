//! Configuration loading for the control-plane server.

#![cfg_attr(test, allow(clippy::expect_used))]

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:9080";
const DEFAULT_VALKEY_URL: &str = "rediss://valkey:6379";
const DEFAULT_VALKEY_USER: &str = "cp-server";
const DEFAULT_VALKEY_PASS_FILE: &str = "/run/secrets/valkey_cp_server_password";
const DEFAULT_VALKEY_CA: &str = "/etc/valkey/tls/ca.crt";
const DEFAULT_VALKEY_CLIENT_CERT: &str = "/etc/valkey/tls/client.crt";
const DEFAULT_VALKEY_CLIENT_KEY: &str = "/etc/valkey/tls/client.key";

/// Control-plane server configuration loaded from `POLIS_CP_*` env vars.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    #[serde(default = "default_valkey_url")]
    pub valkey_url: String,
    #[serde(default = "default_valkey_user")]
    pub valkey_user: String,
    #[serde(default = "default_valkey_pass_file")]
    pub valkey_pass_file: String,
    #[serde(default = "default_valkey_ca")]
    pub valkey_ca: String,
    #[serde(default = "default_valkey_client_cert")]
    pub valkey_client_cert: String,
    #[serde(default = "default_valkey_client_key")]
    pub valkey_client_key: String,
}

impl Config {
    /// Load configuration from the current process environment.
    ///
    /// # Errors
    ///
    /// Returns an error if env var deserialization fails.
    pub fn from_env() -> Result<Self> {
        envy::prefixed("POLIS_CP_")
            .from_env()
            .context("failed to load control-plane config from POLIS_CP_* env vars")
    }

    /// Load configuration from an explicit iterator of environment-style pairs.
    ///
    /// # Errors
    ///
    /// Returns an error if env var deserialization fails.
    pub fn from_env_pairs<I, K, V>(iter: I) -> std::result::Result<Self, envy::Error>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let normalized = iter
            .into_iter()
            .map(|(key, value)| (key.as_ref().to_string(), value.as_ref().to_string()))
            .collect::<Vec<_>>();
        envy::prefixed("POLIS_CP_").from_iter(normalized)
    }

    /// Read the Valkey ACL password from the configured password file.
    ///
    /// # Errors
    ///
    /// Returns an error if the password file cannot be read.
    pub fn read_password(&self) -> Result<String> {
        let password = std::fs::read_to_string(&self.valkey_pass_file)
            .with_context(|| format!("failed to read {}", self.valkey_pass_file))?;
        Ok(password.trim().to_string())
    }
}

fn default_listen_addr() -> String {
    DEFAULT_LISTEN_ADDR.to_string()
}

fn default_valkey_url() -> String {
    DEFAULT_VALKEY_URL.to_string()
}

fn default_valkey_user() -> String {
    DEFAULT_VALKEY_USER.to_string()
}

fn default_valkey_pass_file() -> String {
    DEFAULT_VALKEY_PASS_FILE.to_string()
}

fn default_valkey_ca() -> String {
    DEFAULT_VALKEY_CA.to_string()
}

fn default_valkey_client_cert() -> String {
    DEFAULT_VALKEY_CLIENT_CERT.to_string()
}

fn default_valkey_client_key() -> String {
    DEFAULT_VALKEY_CLIENT_KEY.to_string()
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn config_defaults_match_design() {
        let config = Config::from_env_pairs(Vec::<(String, String)>::new()).expect("defaults load");

        assert_eq!(config.listen_addr, "0.0.0.0:9080");
        assert_eq!(config.valkey_url, "rediss://valkey:6379");
        assert_eq!(config.valkey_user, "cp-server");
        assert_eq!(
            config.valkey_pass_file,
            "/run/secrets/valkey_cp_server_password"
        );
        assert_eq!(config.valkey_ca, "/etc/valkey/tls/ca.crt");
        assert_eq!(config.valkey_client_cert, "/etc/valkey/tls/client.crt");
        assert_eq!(config.valkey_client_key, "/etc/valkey/tls/client.key");
    }

    #[test]
    fn config_parses_explicit_env_values() {
        let config = Config::from_env_pairs([
            ("POLIS_CP_LISTEN_ADDR", "127.0.0.1:19080"),
            ("POLIS_CP_VALKEY_URL", "rediss://state:6379"),
            ("POLIS_CP_VALKEY_USER", "custom-user"),
            ("POLIS_CP_VALKEY_PASS_FILE", "/tmp/pass"),
            ("POLIS_CP_VALKEY_CA", "/tmp/ca.crt"),
            ("POLIS_CP_VALKEY_CLIENT_CERT", "/tmp/client.crt"),
            ("POLIS_CP_VALKEY_CLIENT_KEY", "/tmp/client.key"),
        ])
        .expect("explicit env parses");

        assert_eq!(config.listen_addr, "127.0.0.1:19080");
        assert_eq!(config.valkey_url, "rediss://state:6379");
        assert_eq!(config.valkey_user, "custom-user");
        assert_eq!(config.valkey_pass_file, "/tmp/pass");
        assert_eq!(config.valkey_ca, "/tmp/ca.crt");
        assert_eq!(config.valkey_client_cert, "/tmp/client.crt");
        assert_eq!(config.valkey_client_key, "/tmp/client.key");
    }
}
