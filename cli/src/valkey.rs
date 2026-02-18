//! Valkey client for CLI metrics operations.
//!
//! This module provides a client for reading metrics from Valkey/Redis.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use polis_common::types::{ActivityEvent, ActivityEventType, BlockReason, InspectionStatus, MetricsSnapshot};
use std::collections::HashMap;

/// Valkey key names for metrics.
pub mod keys {
    pub const WINDOW_START: &str = "polis:metrics:window_start";
    pub const REQUESTS_INSPECTED: &str = "polis:metrics:requests_inspected";
    pub const BLOCKED_CREDENTIALS: &str = "polis:metrics:blocked_credentials";
    pub const BLOCKED_MALWARE: &str = "polis:metrics:blocked_malware";
    pub const ACTIVITY_STREAM: &str = "polis:activity";
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

/// Parse an activity event type string.
///
/// # Errors
///
/// Returns an error if the string is not a known event type.
fn parse_event_type(s: &str) -> Result<ActivityEventType> {
    match s {
        "request" => Ok(ActivityEventType::Request),
        "response" => Ok(ActivityEventType::Response),
        "scan" => Ok(ActivityEventType::Scan),
        "block" => Ok(ActivityEventType::Block),
        "agent" => Ok(ActivityEventType::Agent),
        _ => Err(anyhow::anyhow!("unknown event type: {s}")),
    }
}

/// Parse an inspection status string.
///
/// # Errors
///
/// Returns an error if the string is not a known status.
fn parse_inspection_status(s: &str) -> Result<InspectionStatus> {
    match s {
        "inspected" => Ok(InspectionStatus::Inspected),
        "clean" => Ok(InspectionStatus::Clean),
        "blocked" => Ok(InspectionStatus::Blocked),
        _ => Err(anyhow::anyhow!("unknown inspection status: {s}")),
    }
}

/// Parse a block reason string.
///
/// # Errors
///
/// Returns an error if the string is not a known block reason.
fn parse_block_reason(s: &str) -> Result<BlockReason> {
    match s {
        "credential_detected" => Ok(BlockReason::CredentialDetected),
        "malware_domain" => Ok(BlockReason::MalwareDomain),
        "url_blocked" => Ok(BlockReason::UrlBlocked),
        "file_infected" => Ok(BlockReason::FileInfected),
        _ => Err(anyhow::anyhow!("unknown block reason: {s}")),
    }
}

/// Parse a Valkey stream entry map into an [`ActivityEvent`].
///
/// # Errors
///
/// Returns an error if required fields (`type`, `status`) are missing or invalid.
fn parse_activity_event(map: &HashMap<String, redis::Value>) -> Result<ActivityEvent> {
    let get_str = |key: &str| -> Option<String> {
        map.get(key)
            .and_then(|v| redis::from_redis_value(v).ok())
    };

    let ts = get_str("ts")
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map_or_else(Utc::now, |dt| dt.with_timezone(&Utc));

    let event_type = parse_event_type(&get_str("type").unwrap_or_default())?;
    let status = parse_inspection_status(&get_str("status").unwrap_or_default())?;

    Ok(ActivityEvent {
        ts,
        event_type,
        dest: get_str("dest"),
        method: get_str("method"),
        path: get_str("path"),
        status,
        reason: get_str("reason")
            .filter(|s| !s.is_empty())
            .and_then(|r| parse_block_reason(&r).ok()),
        detail: get_str("detail").filter(|s| !s.is_empty()),
    })
}

impl ValkeyClient {
    /// Read recent activity events in reverse chronological order.
    ///
    /// # Errors
    ///
    /// Returns an error if the Valkey connection fails or the stream cannot be read.
    pub async fn get_activity(&self, count: usize) -> Result<Vec<ActivityEvent>> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("failed to connect to Valkey")?;

        let reply: redis::streams::StreamRangeReply = redis::cmd("XREVRANGE")
            .arg(keys::ACTIVITY_STREAM)
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg(count)
            .query_async(&mut conn)
            .await
            .context("failed to read activity stream")?;

        reply
            .ids
            .into_iter()
            .map(|entry| parse_activity_event(&entry.map))
            .collect()
    }

    /// Stream new activity events using a blocking read.
    ///
    /// Returns entries with IDs newer than `last_id`. Use `"$"` to read only
    /// new entries. Blocks up to `timeout_ms` milliseconds before returning an
    /// empty vec if no new entries arrive.
    ///
    /// # Errors
    ///
    /// Returns an error if the Valkey connection fails or the stream cannot be read.
    pub async fn stream_activity(
        &self,
        last_id: &str,
        timeout_ms: u64,
    ) -> Result<Vec<(String, ActivityEvent)>> {
        use redis::AsyncCommands as _;

        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .context("failed to connect to Valkey")?;

        let opts = redis::streams::StreamReadOptions::default()
            .block(usize::try_from(timeout_ms).unwrap_or(usize::MAX))
            .count(100);

        let reply: redis::streams::StreamReadReply = conn
            .xread_options(&[keys::ACTIVITY_STREAM], &[last_id], &opts)
            .await
            .context("failed to read activity stream")?;

        reply
            .keys
            .into_iter()
            .flat_map(|k| k.ids)
            .map(|entry| {
                let event = parse_activity_event(&entry.map)?;
                Ok((entry.id, event))
            })
            .collect()
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
    use polis_common::types::{ActivityEventType, BlockReason, InspectionStatus};

    // =========================================================================
    // Test helper
    // =========================================================================

    fn stream_map(pairs: &[(&str, &str)]) -> HashMap<String, redis::Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), redis::Value::BulkString(v.as_bytes().to_vec())))
            .collect()
    }

    // =========================================================================
    // keys::ACTIVITY_STREAM
    // =========================================================================

    #[test]
    fn test_activity_stream_key_matches_spec() {
        assert_eq!(keys::ACTIVITY_STREAM, "polis:activity");
    }

    // =========================================================================
    // parse_event_type
    // =========================================================================

    #[test]
    fn test_parse_event_type_request_returns_request() {
        assert_eq!(
            parse_event_type("request").expect("'request' is valid"),
            ActivityEventType::Request
        );
    }

    #[test]
    fn test_parse_event_type_response_returns_response() {
        assert_eq!(
            parse_event_type("response").expect("'response' is valid"),
            ActivityEventType::Response
        );
    }

    #[test]
    fn test_parse_event_type_scan_returns_scan() {
        assert_eq!(
            parse_event_type("scan").expect("'scan' is valid"),
            ActivityEventType::Scan
        );
    }

    #[test]
    fn test_parse_event_type_block_returns_block() {
        assert_eq!(
            parse_event_type("block").expect("'block' is valid"),
            ActivityEventType::Block
        );
    }

    #[test]
    fn test_parse_event_type_agent_returns_agent() {
        assert_eq!(
            parse_event_type("agent").expect("'agent' is valid"),
            ActivityEventType::Agent
        );
    }

    #[test]
    fn test_parse_event_type_unknown_returns_error() {
        assert!(parse_event_type("unknown").is_err());
    }

    #[test]
    fn test_parse_event_type_empty_returns_error() {
        assert!(parse_event_type("").is_err());
    }

    // =========================================================================
    // parse_inspection_status
    // =========================================================================

    #[test]
    fn test_parse_inspection_status_inspected_returns_inspected() {
        assert_eq!(
            parse_inspection_status("inspected").expect("'inspected' is valid"),
            InspectionStatus::Inspected
        );
    }

    #[test]
    fn test_parse_inspection_status_clean_returns_clean() {
        assert_eq!(
            parse_inspection_status("clean").expect("'clean' is valid"),
            InspectionStatus::Clean
        );
    }

    #[test]
    fn test_parse_inspection_status_blocked_returns_blocked() {
        assert_eq!(
            parse_inspection_status("blocked").expect("'blocked' is valid"),
            InspectionStatus::Blocked
        );
    }

    #[test]
    fn test_parse_inspection_status_unknown_returns_error() {
        assert!(parse_inspection_status("unknown").is_err());
    }

    // =========================================================================
    // parse_block_reason
    // =========================================================================

    #[test]
    fn test_parse_block_reason_credential_detected_returns_variant() {
        assert_eq!(
            parse_block_reason("credential_detected").expect("'credential_detected' is valid"),
            BlockReason::CredentialDetected
        );
    }

    #[test]
    fn test_parse_block_reason_malware_domain_returns_variant() {
        assert_eq!(
            parse_block_reason("malware_domain").expect("'malware_domain' is valid"),
            BlockReason::MalwareDomain
        );
    }

    #[test]
    fn test_parse_block_reason_url_blocked_returns_variant() {
        assert_eq!(
            parse_block_reason("url_blocked").expect("'url_blocked' is valid"),
            BlockReason::UrlBlocked
        );
    }

    #[test]
    fn test_parse_block_reason_file_infected_returns_variant() {
        assert_eq!(
            parse_block_reason("file_infected").expect("'file_infected' is valid"),
            BlockReason::FileInfected
        );
    }

    #[test]
    fn test_parse_block_reason_unknown_returns_error() {
        assert!(parse_block_reason("unknown").is_err());
    }

    // =========================================================================
    // parse_activity_event
    // =========================================================================

    #[test]
    fn test_parse_activity_event_request_event_all_fields() {
        let map = stream_map(&[
            ("ts", "2026-02-17T14:32:05.123Z"),
            ("type", "request"),
            ("dest", "api.anthropic.com"),
            ("method", "POST"),
            ("path", "/v1/messages"),
            ("status", "inspected"),
        ]);
        let event = parse_activity_event(&map).expect("valid request event should parse");
        assert_eq!(event.event_type, ActivityEventType::Request);
        assert_eq!(event.dest.as_deref(), Some("api.anthropic.com"));
        assert_eq!(event.method.as_deref(), Some("POST"));
        assert_eq!(event.path.as_deref(), Some("/v1/messages"));
        assert_eq!(event.status, InspectionStatus::Inspected);
        assert!(event.reason.is_none());
        assert!(event.detail.is_none());
    }

    #[test]
    fn test_parse_activity_event_block_event_with_reason_and_detail() {
        let map = stream_map(&[
            ("ts", "2026-02-17T14:32:05.123Z"),
            ("type", "block"),
            ("dest", "evil.example.com"),
            ("status", "blocked"),
            ("reason", "malware_domain"),
            ("detail", "known C2 host"),
        ]);
        let event = parse_activity_event(&map).expect("valid block event should parse");
        assert_eq!(event.event_type, ActivityEventType::Block);
        assert_eq!(event.status, InspectionStatus::Blocked);
        assert_eq!(event.reason, Some(BlockReason::MalwareDomain));
        assert_eq!(event.detail.as_deref(), Some("known C2 host"));
    }

    #[test]
    fn test_parse_activity_event_missing_optional_fields_are_none() {
        let map = stream_map(&[
            ("ts", "2026-02-17T14:32:05.123Z"),
            ("type", "scan"),
            ("status", "clean"),
        ]);
        let event = parse_activity_event(&map).expect("event with only required fields should parse");
        assert!(event.dest.is_none());
        assert!(event.method.is_none());
        assert!(event.path.is_none());
        assert!(event.reason.is_none());
        assert!(event.detail.is_none());
    }

    #[test]
    fn test_parse_activity_event_empty_reason_treated_as_none() {
        let map = stream_map(&[
            ("ts", "2026-02-17T14:32:05.123Z"),
            ("type", "scan"),
            ("status", "clean"),
            ("reason", ""),
        ]);
        let event = parse_activity_event(&map).expect("empty reason should not fail");
        assert!(event.reason.is_none(), "empty reason string should become None");
    }

    #[test]
    fn test_parse_activity_event_empty_detail_treated_as_none() {
        let map = stream_map(&[
            ("ts", "2026-02-17T14:32:05.123Z"),
            ("type", "response"),
            ("status", "inspected"),
            ("detail", ""),
        ]);
        let event = parse_activity_event(&map).expect("empty detail should not fail");
        assert!(event.detail.is_none(), "empty detail string should become None");
    }

    #[test]
    fn test_parse_activity_event_missing_type_returns_error() {
        let map = stream_map(&[
            ("ts", "2026-02-17T14:32:05.123Z"),
            ("status", "inspected"),
        ]);
        assert!(
            parse_activity_event(&map).is_err(),
            "missing 'type' field should return error"
        );
    }

    #[test]
    fn test_parse_activity_event_missing_status_returns_error() {
        let map = stream_map(&[
            ("ts", "2026-02-17T14:32:05.123Z"),
            ("type", "request"),
        ]);
        assert!(
            parse_activity_event(&map).is_err(),
            "missing 'status' field should return error"
        );
    }

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

    fn arb_event_type() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            Just("request"),
            Just("response"),
            Just("scan"),
            Just("block"),
            Just("agent"),
        ]
    }

    fn arb_status() -> impl Strategy<Value = &'static str> {
        prop_oneof![Just("inspected"), Just("clean"), Just("blocked")]
    }

    fn arb_block_reason() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            Just("credential_detected"),
            Just("malware_domain"),
            Just("url_blocked"),
            Just("file_infected"),
        ]
    }

    fn make_map(pairs: &[(&str, &str)]) -> HashMap<String, redis::Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), redis::Value::BulkString(v.as_bytes().to_vec())))
            .collect()
    }

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

        /// Every valid event type string parses successfully
        #[test]
        fn prop_parse_event_type_valid_strings_succeed(s in arb_event_type()) {
            prop_assert!(parse_event_type(s).is_ok());
        }

        /// Strings that are not valid event types always return an error
        #[test]
        fn prop_parse_event_type_unknown_string_returns_error(
            s in "[a-zA-Z_]{1,20}".prop_filter("not a valid event type", |s| {
                !matches!(s.as_str(), "request" | "response" | "scan" | "block" | "agent")
            })
        ) {
            prop_assert!(parse_event_type(&s).is_err());
        }

        /// Every valid inspection status string parses successfully
        #[test]
        fn prop_parse_inspection_status_valid_strings_succeed(s in arb_status()) {
            prop_assert!(parse_inspection_status(s).is_ok());
        }

        /// Strings that are not valid statuses always return an error
        #[test]
        fn prop_parse_inspection_status_unknown_string_returns_error(
            s in "[a-zA-Z_]{1,20}".prop_filter("not a valid status", |s| {
                !matches!(s.as_str(), "inspected" | "clean" | "blocked")
            })
        ) {
            prop_assert!(parse_inspection_status(&s).is_err());
        }

        /// Every valid block reason string parses successfully
        #[test]
        fn prop_parse_block_reason_valid_strings_succeed(s in arb_block_reason()) {
            prop_assert!(parse_block_reason(s).is_ok());
        }

        /// A map with valid type and status always parses successfully
        #[test]
        fn prop_parse_activity_event_valid_required_fields_succeeds(
            event_type in arb_event_type(),
            status in arb_status(),
        ) {
            let map = make_map(&[
                ("ts", "2026-02-17T14:32:05.123Z"),
                ("type", event_type),
                ("status", status),
            ]);
            prop_assert!(parse_activity_event(&map).is_ok());
        }

        /// Empty reason and detail fields are always treated as None
        #[test]
        fn prop_parse_activity_event_empty_optional_fields_are_none(
            event_type in arb_event_type(),
            status in arb_status(),
        ) {
            let map = make_map(&[
                ("ts", "2026-02-17T14:32:05.123Z"),
                ("type", event_type),
                ("status", status),
                ("reason", ""),
                ("detail", ""),
            ]);
            let event = parse_activity_event(&map).expect("should parse with empty optional fields");
            prop_assert!(event.reason.is_none());
            prop_assert!(event.detail.is_none());
        }

        /// A valid block reason round-trips through parse_activity_event
        #[test]
        fn prop_parse_activity_event_block_reason_roundtrip(reason in arb_block_reason()) {
            let map = make_map(&[
                ("ts", "2026-02-17T14:32:05.123Z"),
                ("type", "block"),
                ("status", "blocked"),
                ("reason", reason),
            ]);
            let event = parse_activity_event(&map).expect("block event should parse");
            prop_assert!(event.reason.is_some(), "known reason should not be None");
        }
    }
}
