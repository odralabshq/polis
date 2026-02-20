//! Application state wrapping a redis `MultiplexedConnection`.
//!
//! All Valkey operations go through `AppState`. Namespace iteration
//! uses `SCAN` with `MATCH`/`COUNT` — never `KEYS` (disabled in the
//! `mcp-agent` ACL user).
//!
//! ## mTLS Support
//!
//! When the Valkey URL uses the `rediss://` scheme AND client cert/key
//! files exist at the configured paths, the connection is established
//! with mutual TLS (mTLS). This uses `redis::Client::build_with_tls`
//! with `TlsCertificates` containing the CA cert, client cert, and
//! client key — all loaded from PEM files at startup.

use anyhow::{Context, Result};
use deadpool_redis::redis::{self, AsyncCommands};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

use polis_mcp_common::{
    blocked_key, approved_key,
    redis_keys::{keys, ttl},
    BlockedRequest, RequestStatus, SecurityLevel, SecurityLogEntry,
};

/// Default path to Valkey CA certificate (mounted in container).
const DEFAULT_VALKEY_CA_PATH: &str = "/etc/valkey/tls/ca.crt";

/// Default path to Valkey client certificate (mounted in container).
const DEFAULT_VALKEY_CLIENT_CERT_PATH: &str = "/etc/valkey/tls/client.crt";

/// Default path to Valkey client key (mounted in container).
const DEFAULT_VALKEY_CLIENT_KEY_PATH: &str = "/etc/valkey/tls/client.key";

/// Shared application state holding the Valkey connection.
///
/// Uses a `MultiplexedConnection` which supports concurrent requests
/// on a single TCP connection. The connection is `Clone` — each clone
/// shares the same underlying socket.
#[derive(Clone)]
pub struct AppState {
    conn: redis::aio::MultiplexedConnection,
}

impl AppState {
    /// Create a new `AppState`, connect to Valkey, and verify with PING.
    ///
    /// # mTLS
    ///
    /// For `rediss://` URLs, if client cert and key files exist at the
    /// configured paths (env vars or defaults), the connection uses
    /// mutual TLS via `Client::build_with_tls`. Otherwise, falls back
    /// to server-only TLS using the system CA store.
    ///
    /// # Environment Variables (optional)
    ///
    /// - `polis_AGENT_VALKEY_CA`          — CA cert path (default: `/etc/valkey/tls/ca.crt`)
    /// - `polis_AGENT_VALKEY_CLIENT_CERT` — Client cert path (default: `/etc/valkey/tls/client.crt`)
    /// - `polis_AGENT_VALKEY_CLIENT_KEY`  — Client key path (default: `/etc/valkey/tls/client.key`)
    pub async fn new(
        valkey_url: &str,
        user: &str,
        password: &str,
    ) -> Result<Self> {
        let url_with_auth = build_auth_url(valkey_url, user, password)?;

        let client = build_client(&url_with_auth)?;

        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .context("failed to connect to Valkey")?;

        // Verify connectivity at startup (Requirement 3.4).
        redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
            .context("Valkey startup PING failed — is Valkey reachable?")?;

        tracing::info!("Valkey connection ready (mTLS={})", is_mtls_configured());

        Ok(Self { conn })
    }

    // ---------------------------------------------------------------
    // store_blocked_request  (Requirement 2.5, 5.1)
    // ---------------------------------------------------------------

    /// Store a blocked request in Valkey with a 1-hour TTL.
    pub async fn store_blocked_request(
        &self,
        request: &BlockedRequest,
    ) -> Result<()> {
        let key = blocked_key(&request.request_id);
        let json = serde_json::to_string(request)
            .context("failed to serialize BlockedRequest")?;

        let mut conn = self.conn.clone();
        conn.set_ex::<_, _, ()>(
            &key,
            &json,
            ttl::BLOCKED_REQUEST_SECS,
        )
        .await
        .context("SETEX failed for blocked request")?;

        tracing::info!(
            request_id = %request.request_id,
            "stored blocked request (TTL={}s)",
            ttl::BLOCKED_REQUEST_SECS,
        );
        Ok(())
    }

    // ---------------------------------------------------------------
    // count_pending_approvals  (Requirement 2.7)
    // ---------------------------------------------------------------

    /// Count keys matching `polis:blocked:*` using SCAN (never KEYS).
    pub async fn count_pending_approvals(&self) -> Result<usize> {
        self.scan_count(&format!("{}*", keys::BLOCKED)).await
    }

    // ---------------------------------------------------------------
    // count_recent_approvals  (Requirement 2.7)
    // ---------------------------------------------------------------

    /// Count keys matching `polis:approved:*` using SCAN (never KEYS).
    pub async fn count_recent_approvals(&self) -> Result<usize> {
        self.scan_count(&format!("{}*", keys::APPROVED)).await
    }

    // ---------------------------------------------------------------
    // get_security_level  (Requirement 2.7)
    // ---------------------------------------------------------------

    /// Retrieve the current security level from Valkey.
    pub async fn get_security_level(&self) -> Result<SecurityLevel> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn
            .get(keys::SECURITY_LEVEL)
            .await
            .context("GET security_level failed")?;

        match raw {
            Some(val) => {
                let level: SecurityLevel =
                    serde_json::from_str(&format!("\"{}\"", val))
                        .unwrap_or_default();
                Ok(level)
            }
            None => Ok(SecurityLevel::default()),
        }
    }

    // ---------------------------------------------------------------
    // get_pending_approvals  (Requirement 2.8)
    // ---------------------------------------------------------------

    /// Return all blocked requests with `pattern` redacted (CWE-200).
    pub async fn get_pending_approvals(&self) -> Result<Vec<BlockedRequest>> {
        let matched_keys = self
            .scan_keys(&format!("{}*", keys::BLOCKED))
            .await?;

        if matched_keys.is_empty() {
            return Ok(Vec::new());
        }

        let mut conn = self.conn.clone();

        // Pipeline all GETs into a single round-trip.
        let mut pipe = redis::pipe();
        for key in &matched_keys {
            pipe.cmd("GET").arg(key);
        }

        let raw_values: Vec<Option<String>> = pipe
            .query_async(&mut conn)
            .await
            .context("pipelined GET for blocked requests failed")?;

        let mut results = Vec::with_capacity(raw_values.len());
        for (i, maybe_json) in raw_values.into_iter().enumerate() {
            if let Some(json) = maybe_json {
                match serde_json::from_str::<BlockedRequest>(&json) {
                    Ok(mut req) => {
                        req.pattern = None;
                        results.push(req);
                    }
                    Err(e) => {
                        tracing::warn!(
                            key = %matched_keys[i],
                            error = %e,
                            "skipping malformed blocked request",
                        );
                    }
                }
            }
        }
        Ok(results)
    }

    // ---------------------------------------------------------------
    // get_security_log  (Requirement 2.9)
    // ---------------------------------------------------------------

    /// Retrieve the most recent `limit` entries from the security log.
    pub async fn get_security_log(
        &self,
        limit: usize,
    ) -> Result<Vec<SecurityLogEntry>> {
        let mut conn = self.conn.clone();
        let stop = if limit == 0 { 0 } else { limit - 1 };

        let raw: Vec<String> = redis::cmd("ZREVRANGE")
            .arg(keys::EVENT_LOG)
            .arg(0_isize)
            .arg(stop as isize)
            .query_async(&mut conn)
            .await
            .context("ZREVRANGE on event log failed")?;

        let mut entries = Vec::with_capacity(raw.len());
        for json in &raw {
            match serde_json::from_str::<SecurityLogEntry>(json) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    tracing::warn!(error = %e, "skipping malformed log entry");
                }
            }
        }
        Ok(entries)
    }

    // ---------------------------------------------------------------
    // get_request_status  (Requirement 2.10)
    // ---------------------------------------------------------------

    /// Check whether a request is approved, pending, or not found.
    pub async fn get_request_status(
        &self,
        request_id: &str,
    ) -> Result<RequestStatus> {
        let mut conn = self.conn.clone();

        let approved_exists: bool = conn
            .exists(approved_key(request_id))
            .await
            .context("EXISTS approved key failed")?;

        if approved_exists {
            return Ok(RequestStatus::Approved);
        }

        let blocked_exists: bool = conn
            .exists(blocked_key(request_id))
            .await
            .context("EXISTS blocked key failed")?;

        if blocked_exists {
            return Ok(RequestStatus::Pending);
        }

        Ok(RequestStatus::Denied)
    }

    // ---------------------------------------------------------------
    // log_event  (MVP: tracing only)
    // ---------------------------------------------------------------

    /// Log a security event (MVP: tracing only, no Valkey write).
    pub fn log_event(
        &self,
        event_type: &str,
        request_id: &str,
        details: &str,
    ) {
        tracing::info!(
            event_type = %event_type,
            request_id = %request_id,
            details = %details,
            "security event (MVP: tracing only)",
        );
    }

    // ---------------------------------------------------------------
    // Private helpers
    // ---------------------------------------------------------------

    async fn scan_count(&self, pattern: &str) -> Result<usize> {
        let matched = self.scan_keys(pattern).await?;
        Ok(matched.len())
    }

    async fn scan_keys(&self, pattern: &str) -> Result<Vec<String>> {
        let mut conn = self.conn.clone();
        let mut all_keys: Vec<String> = Vec::new();
        let mut cursor: u64 = 0;

        loop {
            let (next_cursor, batch): (u64, Vec<String>) =
                redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(pattern)
                    .arg("COUNT")
                    .arg(100)
                    .query_async(&mut conn)
                    .await
                    .context("SCAN failed")?;

            all_keys.extend(batch);
            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }
        Ok(all_keys)
    }
}

// -------------------------------------------------------------------
// TLS / mTLS helpers
// -------------------------------------------------------------------

/// Check if mTLS cert files are available at the configured paths.
fn is_mtls_configured() -> bool {
    let cert_path = std::env::var("polis_AGENT_VALKEY_CLIENT_CERT")
        .unwrap_or_else(|_| DEFAULT_VALKEY_CLIENT_CERT_PATH.to_string());
    let key_path = std::env::var("polis_AGENT_VALKEY_CLIENT_KEY")
        .unwrap_or_else(|_| DEFAULT_VALKEY_CLIENT_KEY_PATH.to_string());
    std::path::Path::new(&cert_path).exists()
        && std::path::Path::new(&key_path).exists()
}

/// Build a `redis::Client`, using mTLS when certs are available.
fn build_client(url_with_auth: &str) -> Result<redis::Client> {
    let is_tls = url_with_auth.starts_with("rediss://");

    if is_tls && is_mtls_configured() {
        build_mtls_client(url_with_auth)
    } else {
        redis::Client::open(url_with_auth)
            .context("failed to create Valkey client")
    }
}

/// Build a `redis::Client` with mTLS (mutual TLS).
///
/// Loads PEM-encoded CA cert, client cert, and client key from disk,
/// then calls `Client::build_with_tls` with `TlsCertificates`.
///
/// # Paths (env var → default)
///
/// | Env var                          | Default                          |
/// |----------------------------------|----------------------------------|
/// | `polis_AGENT_VALKEY_CA`          | `/etc/valkey/tls/ca.crt`         |
/// | `polis_AGENT_VALKEY_CLIENT_CERT` | `/etc/valkey/tls/client.crt`     |
/// | `polis_AGENT_VALKEY_CLIENT_KEY`  | `/etc/valkey/tls/client.key`     |
fn build_mtls_client(url_with_auth: &str) -> Result<redis::Client> {
    use deadpool_redis::redis::{ClientTlsConfig, TlsCertificates};

    let ca_path = std::env::var("polis_AGENT_VALKEY_CA")
        .unwrap_or_else(|_| DEFAULT_VALKEY_CA_PATH.to_string());
    let cert_path = std::env::var("polis_AGENT_VALKEY_CLIENT_CERT")
        .unwrap_or_else(|_| DEFAULT_VALKEY_CLIENT_CERT_PATH.to_string());
    let key_path = std::env::var("polis_AGENT_VALKEY_CLIENT_KEY")
        .unwrap_or_else(|_| DEFAULT_VALKEY_CLIENT_KEY_PATH.to_string());

    let ca_cert = std::fs::read(&ca_path)
        .with_context(|| format!("failed to read CA cert: {ca_path}"))?;
    let client_cert = std::fs::read(&cert_path)
        .with_context(|| format!("failed to read client cert: {cert_path}"))?;
    let client_key = std::fs::read(&key_path)
        .with_context(|| format!("failed to read client key: {key_path}"))?;

    tracing::info!(
        ca = %ca_path,
        cert = %cert_path,
        key = %key_path,
        "loading mTLS certificates for Valkey connection",
    );

    let tls_certs = TlsCertificates {
        client_tls: Some(ClientTlsConfig {
            client_cert,
            client_key,
        }),
        root_cert: Some(ca_cert),
    };

    redis::Client::build_with_tls(url_with_auth, tls_certs)
        .map_err(|e| anyhow::anyhow!("failed to build mTLS Valkey client: {e}"))
}

// -------------------------------------------------------------------
// URL helper
// -------------------------------------------------------------------

/// Insert ACL credentials into a Redis URL.
///
/// Transforms `redis://host:port` → `redis://user:password@host:port`.
/// If the URL already contains credentials they are replaced.
///
/// Credentials are percent-encoded (RFC 3986 §3.2.1) so that special
/// characters (`@`, `:`, `/`, `#`, `?`, etc.) do not break the URL.
fn build_auth_url(
    base_url: &str,
    user: &str,
    password: &str,
) -> Result<String> {
    let scheme_end = base_url
        .find("://")
        .context("invalid Valkey URL: missing '://'")?;

    let scheme = &base_url[..scheme_end];
    let rest = &base_url[scheme_end + 3..];

    // Strip any existing credentials.
    let host_and_path = match rest.find('@') {
        Some(at) => &rest[at + 1..],
        None => rest,
    };

    let enc_user = utf8_percent_encode(user, NON_ALPHANUMERIC);
    let enc_pass = utf8_percent_encode(password, NON_ALPHANUMERIC);

    Ok(format!("{scheme}://{enc_user}:{enc_pass}@{host_and_path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_url_basic() {
        let url = build_auth_url(
            "redis://valkey:6379",
            "mcp-agent",
            "s3cret",
        )
        .unwrap();
        assert_eq!(url, "redis://mcp%2Dagent:s3cret@valkey:6379");
    }

    #[test]
    fn auth_url_replaces_existing_creds() {
        let url = build_auth_url(
            "redis://old:pass@valkey:6379",
            "mcp-agent",
            "new-pass",
        )
        .unwrap();
        assert_eq!(url, "redis://mcp%2Dagent:new%2Dpass@valkey:6379");
    }

    #[test]
    fn auth_url_with_db() {
        let url = build_auth_url(
            "redis://valkey:6379/2",
            "mcp-agent",
            "pw",
        )
        .unwrap();
        assert_eq!(url, "redis://mcp%2Dagent:pw@valkey:6379/2");
    }

    #[test]
    fn auth_url_rediss_scheme() {
        let url = build_auth_url(
            "rediss://valkey:6380",
            "user",
            "pass",
        )
        .unwrap();
        assert_eq!(url, "rediss://user:pass@valkey:6380");
    }

    #[test]
    fn auth_url_rejects_missing_scheme() {
        let result = build_auth_url("valkey:6379", "u", "p");
        assert!(result.is_err());
    }

    #[test]
    #[allow(clippy::hardcoded_credentials)] // test fixture for URL encoding, not a real credential
    fn auth_url_encodes_special_chars() {
        let url = build_auth_url(
            "redis://valkey:6379",
            "admin",
            "p@ss:w/rd#1?",
        )
        .unwrap();
        assert_eq!(
            url,
            "redis://admin:p%40ss%3Aw%2Frd%231%3F@valkey:6379"
        );
    }

    #[test]
    fn mtls_configured_returns_false_when_no_certs() {
        // In test env, default paths don't exist.
        std::env::remove_var("polis_AGENT_VALKEY_CLIENT_CERT");
        std::env::remove_var("polis_AGENT_VALKEY_CLIENT_KEY");
        assert!(!is_mtls_configured());
    }

    #[test]
    fn build_client_plain_url_succeeds() {
        let url = "redis://localhost:6379";
        let client = build_client(url);
        assert!(client.is_ok());
    }

    #[test]
    fn build_client_rediss_without_certs_falls_back() {
        // rediss:// but no cert files → should still create a client
        // (server-only TLS, no mTLS).
        std::env::remove_var("polis_AGENT_VALKEY_CLIENT_CERT");
        std::env::remove_var("polis_AGENT_VALKEY_CLIENT_KEY");
        let url = "rediss://localhost:6380";
        let client = build_client(url);
        assert!(client.is_ok());
    }
}
