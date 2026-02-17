//! Valkey client for CLI metrics operations.
//!
//! This module provides a client for reading metrics from Valkey/Redis.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use polis_common::types::MetricsSnapshot;

/// Valkey key names for metrics.
pub mod keys {
    pub const WINDOW_START: &str = "polis:metrics:window_start";
    pub const REQUESTS_INSPECTED: &str = "polis:metrics:requests_inspected";
    pub const BLOCKED_CREDENTIALS: &str = "polis:metrics:blocked_credentials";
    pub const BLOCKED_MALWARE: &str = "polis:metrics:blocked_malware";
}

/// Valkey connection configuration.
pub struct ValkeyConfig {
    /// Valkey host address
    pub host: String,
    /// Valkey port
    pub port: u16,
    /// Optional password for authentication
    pub password: Option<String>,
    /// Whether to use TLS (rediss://)
    pub tls: bool,
}

impl Default for ValkeyConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 6379,
            password: None,
            tls: false,
        }
    }
}

impl ValkeyConfig {
    /// Build the connection URL for this config.
    #[must_use]
    pub fn connection_url(&self) -> String {
        let scheme = if self.tls { "rediss" } else { "redis" };
        format!("{scheme}://{}:{}", self.host, self.port)
    }
}

/// Valkey client for CLI operations.
pub struct ValkeyClient {
    client: redis::Client,
}

impl ValkeyClient {
    /// Create a new Valkey client.
    ///
    /// # Errors
    /// Returns error if the connection URL is invalid.
    pub fn new(config: &ValkeyConfig) -> Result<Self> {
        let url = config.connection_url();
        let client = redis::Client::open(url).context("failed to create Valkey client")?;
        Ok(Self { client })
    }

    /// Fetch current metrics snapshot.
    ///
    /// # Errors
    /// Returns error if connection fails or data cannot be parsed.
    pub async fn get_metrics(&self) -> Result<MetricsSnapshot> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("failed to connect to Valkey")?;

        let (window_start, requests, creds, malware): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = redis::pipe()
            .get(keys::WINDOW_START)
            .get(keys::REQUESTS_INSPECTED)
            .get(keys::BLOCKED_CREDENTIALS)
            .get(keys::BLOCKED_MALWARE)
            .query_async(&mut conn)
            .await
            .context("failed to fetch metrics")?;

        let window_start =
            parse_window_start(window_start).unwrap_or_else(Utc::now);

        Ok(MetricsSnapshot {
            window_start,
            requests_inspected: parse_counter(requests),
            blocked_credentials: parse_counter(creds),
            blocked_malware: parse_counter(malware),
        })
    }
}

/// Parse a Unix timestamp string into a `DateTime`, returning `None` on failure.
#[must_use]
pub fn parse_window_start(value: Option<String>) -> Option<DateTime<Utc>> {
    value
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|ts| DateTime::from_timestamp(ts, 0))
}

/// Parse a counter value, returning 0 for `None` or invalid values.
#[must_use]
pub fn parse_counter(value: Option<String>) -> u64 {
    value
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests use expect for clarity
mod tests {
    use super::*;

    // =========================================================================
    // ValkeyConfig::default tests
    // =========================================================================

    #[test]
    fn test_valkey_config_default_host_is_localhost() {
        let config = ValkeyConfig::default();
        assert_eq!(config.host, "127.0.0.1");
    }

    #[test]
    fn test_valkey_config_default_port_is_6379() {
        let config = ValkeyConfig::default();
        assert_eq!(config.port, 6379);
    }

    #[test]
    fn test_valkey_config_default_password_is_none() {
        let config = ValkeyConfig::default();
        assert!(config.password.is_none());
    }

    #[test]
    fn test_valkey_config_default_tls_is_false() {
        let config = ValkeyConfig::default();
        assert!(!config.tls);
    }

    // =========================================================================
    // ValkeyConfig::connection_url tests
    // =========================================================================

    #[test]
    fn test_valkey_config_connection_url_without_tls() {
        let config = ValkeyConfig {
            host: "127.0.0.1".to_string(),
            port: 6379,
            password: None,
            tls: false,
        };
        assert_eq!(config.connection_url(), "redis://127.0.0.1:6379");
    }

    #[test]
    fn test_valkey_config_connection_url_with_tls() {
        let config = ValkeyConfig {
            host: "127.0.0.1".to_string(),
            port: 6379,
            password: None,
            tls: true,
        };
        assert_eq!(config.connection_url(), "rediss://127.0.0.1:6379");
    }

    #[test]
    fn test_valkey_config_connection_url_with_custom_host_port() {
        let config = ValkeyConfig {
            host: "valkey.example.com".to_string(),
            port: 6380,
            password: None,
            tls: false,
        };
        assert_eq!(config.connection_url(), "redis://valkey.example.com:6380");
    }

    // =========================================================================
    // ValkeyClient::new tests
    // =========================================================================

    #[test]
    fn test_valkey_client_new_succeeds_with_valid_config() {
        let config = ValkeyConfig {
            host: "127.0.0.1".to_string(),
            port: 6379,
            password: None,
            tls: false,
        };
        let result = ValkeyClient::new(&config);
        assert!(result.is_ok(), "should create client with valid config");
    }

    #[test]
    fn test_valkey_client_new_succeeds_with_tls_config() {
        let config = ValkeyConfig {
            host: "127.0.0.1".to_string(),
            port: 6379,
            password: None,
            tls: true,
        };
        let result = ValkeyClient::new(&config);
        assert!(result.is_ok(), "should create client with TLS config");
    }

    // =========================================================================
    // parse_window_start tests
    // =========================================================================

    #[test]
    fn test_parse_window_start_valid_timestamp() {
        let ts = "1700000000".to_string();
        let result = parse_window_start(Some(ts));
        assert!(result.is_some());
        let dt = result.expect("should parse valid timestamp");
        assert_eq!(dt.timestamp(), 1_700_000_000);
    }

    #[test]
    fn test_parse_window_start_none_returns_none() {
        let result = parse_window_start(None);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_window_start_invalid_string_returns_none() {
        let result = parse_window_start(Some("not-a-number".to_string()));
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_window_start_empty_string_returns_none() {
        let result = parse_window_start(Some(String::new()));
        assert!(result.is_none());
    }

    // =========================================================================
    // parse_counter tests
    // =========================================================================

    #[test]
    fn test_parse_counter_valid_number() {
        let result = parse_counter(Some("42".to_string()));
        assert_eq!(result, 42);
    }

    #[test]
    fn test_parse_counter_none_returns_zero() {
        let result = parse_counter(None);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_parse_counter_invalid_string_returns_zero() {
        let result = parse_counter(Some("not-a-number".to_string()));
        assert_eq!(result, 0);
    }

    #[test]
    fn test_parse_counter_empty_string_returns_zero() {
        let result = parse_counter(Some(String::new()));
        assert_eq!(result, 0);
    }

    #[test]
    fn test_parse_counter_negative_returns_zero() {
        // Counters should be non-negative; negative values treated as invalid
        let result = parse_counter(Some("-5".to_string()));
        assert_eq!(result, 0);
    }

    // =========================================================================
    // Key constants tests
    // =========================================================================

    #[test]
    fn test_key_window_start_matches_spec() {
        assert_eq!(keys::WINDOW_START, "polis:metrics:window_start");
    }

    #[test]
    fn test_key_requests_inspected_matches_spec() {
        assert_eq!(keys::REQUESTS_INSPECTED, "polis:metrics:requests_inspected");
    }

    #[test]
    fn test_key_blocked_credentials_matches_spec() {
        assert_eq!(keys::BLOCKED_CREDENTIALS, "polis:metrics:blocked_credentials");
    }

    #[test]
    fn test_key_blocked_malware_matches_spec() {
        assert_eq!(keys::BLOCKED_MALWARE, "polis:metrics:blocked_malware");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Any valid u64 string parses to that value
        #[test]
        fn prop_parse_counter_valid_u64(n in 0u64..=u64::MAX) {
            let result = parse_counter(Some(n.to_string()));
            prop_assert_eq!(result, n);
        }

        /// Any non-numeric string returns 0
        #[test]
        fn prop_parse_counter_non_numeric_returns_zero(s in "[a-zA-Z_-]+") {
            let result = parse_counter(Some(s));
            prop_assert_eq!(result, 0);
        }

        /// Any valid i64 timestamp parses and roundtrips
        #[test]
        fn prop_parse_window_start_roundtrip(ts in 0i64..=253_402_300_799i64) {
            // Max valid timestamp is 9999-12-31T23:59:59Z
            let result = parse_window_start(Some(ts.to_string()));
            prop_assert!(result.is_some());
            prop_assert_eq!(result.unwrap().timestamp(), ts);
        }

        /// connection_url always starts with correct scheme
        #[test]
        fn prop_connection_url_scheme(tls in proptest::bool::ANY, port in 1u16..=65535) {
            let config = ValkeyConfig {
                host: "localhost".to_string(),
                port,
                password: None,
                tls,
            };
            let url = config.connection_url();
            if tls {
                prop_assert!(url.starts_with("rediss://"));
            } else {
                prop_assert!(url.starts_with("redis://"));
            }
        }

        /// connection_url contains host and port
        #[test]
        fn prop_connection_url_contains_host_port(
            host in "[a-z][a-z0-9.-]{0,20}",
            port in 1u16..=65535
        ) {
            let config = ValkeyConfig {
                host: host.clone(),
                port,
                password: None,
                tls: false,
            };
            let url = config.connection_url();
            prop_assert!(url.contains(&host));
            prop_assert!(url.contains(&port.to_string()));
        }
    }
}
