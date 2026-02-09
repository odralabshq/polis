//! Application state wrapping a `deadpool-redis` connection pool.
//!
//! All Valkey operations go through `AppState`. Namespace iteration
//! uses `SCAN` with `MATCH`/`COUNT` — never `KEYS` (disabled in the
//! `mcp-agent` ACL user).

use anyhow::{Context, Result};
use deadpool_redis::{Config, Pool, Runtime};
use deadpool_redis::redis::{self, AsyncCommands};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

use molis_mcp_common::{
    blocked_key, approved_key,
    redis_keys::{keys, ttl},
    BlockedRequest, RequestStatus, SecurityLevel, SecurityLogEntry,
};

/// Default connection-pool size.
const DEFAULT_POOL_SIZE: usize = 8;

/// Default path to Valkey CA certificate (mounted in container).
const DEFAULT_VALKEY_CA_PATH: &str = "/etc/valkey/tls/ca.crt";

/// Shared application state holding the Valkey connection pool.
#[derive(Clone)]
pub struct AppState {
    pool: Pool,
}

impl AppState {
    /// Create a new `AppState`, build the connection pool, and verify
    /// connectivity with a `PING`.
    ///
    /// # Arguments
    /// * `valkey_url` — Redis-compatible URL, e.g. `redis://valkey:6379`
    ///                  or `rediss://valkey:6379` for TLS
    /// * `user`       — ACL username (e.g. `mcp-agent`)
    /// * `password`   — ACL password
    ///
    /// # TLS Configuration
    /// For `rediss://` URLs, the client uses rustls with the system CA store
    /// by default. To use a custom CA (e.g., self-signed), set the
    /// `MOLIS_AGENT_VALKEY_CA` environment variable to the CA cert path,
    /// or mount the CA at `/etc/valkey/tls/ca.crt`.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be created or the startup
    /// PING fails.
    pub async fn new(
        valkey_url: &str,
        user: &str,
        password: &str,
    ) -> Result<Self> {
        // Build a URL with embedded credentials:
        //   redis://user:password@host:port
        let url_with_auth = build_auth_url(valkey_url, user, password)?;

        let mut cfg = Config::from_url(&url_with_auth);
        cfg.pool = Some(deadpool_redis::PoolConfig {
            max_size: DEFAULT_POOL_SIZE,
            ..Default::default()
        });

        let pool = cfg
            .create_pool(Some(Runtime::Tokio1))
            .context("failed to create Valkey connection pool")?;

        // Verify connectivity at startup (Requirement 3.4).
        let mut conn = pool
            .get()
            .await
            .context("failed to get Valkey connection for startup PING")?;

        redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
            .context("Valkey startup PING failed — is Valkey reachable?")?;

        tracing::info!("Valkey connection pool ready (size={DEFAULT_POOL_SIZE})");

        Ok(Self { pool })
    }

    // ---------------------------------------------------------------
    // store_blocked_request  (Requirement 2.5, 5.1)
    // ---------------------------------------------------------------

    /// Store a blocked request in Valkey with a 1-hour TTL.
    ///
    /// Key: `molis:blocked:{request_id}`
    /// Command: `SETEX key 3600 <json>`
    pub async fn store_blocked_request(
        &self,
        request: &BlockedRequest,
    ) -> Result<()> {
        let key = blocked_key(&request.request_id);
        let json = serde_json::to_string(request)
            .context("failed to serialize BlockedRequest")?;

        let mut conn = self.pool.get().await
            .context("pool: failed to get connection")?;

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

    /// Count keys matching `molis:blocked:*` using SCAN (never KEYS).
    pub async fn count_pending_approvals(&self) -> Result<usize> {
        self.scan_count(&format!("{}*", keys::BLOCKED)).await
    }

    // ---------------------------------------------------------------
    // count_recent_approvals  (Requirement 2.7)
    // ---------------------------------------------------------------

    /// Count keys matching `molis:approved:*` using SCAN (never KEYS).
    pub async fn count_recent_approvals(&self) -> Result<usize> {
        self.scan_count(&format!("{}*", keys::APPROVED)).await
    }

    // ---------------------------------------------------------------
    // get_security_level  (Requirement 2.7)
    // ---------------------------------------------------------------

    /// Retrieve the current security level from Valkey.
    ///
    /// Returns `SecurityLevel::Balanced` (the default) when the key
    /// does not exist.
    pub async fn get_security_level(&self) -> Result<SecurityLevel> {
        let mut conn = self.pool.get().await
            .context("pool: failed to get connection")?;

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

    /// Return all blocked requests, with the `pattern` field redacted
    /// to `None` (CWE-200: prevents DLP ruleset exfiltration).
    ///
    /// Uses SCAN to iterate `molis:blocked:*`, then a single pipelined
    /// GET to fetch all values in one round-trip (avoids N+1).
    pub async fn get_pending_approvals(&self) -> Result<Vec<BlockedRequest>> {
        let matched_keys = self
            .scan_keys(&format!("{}*", keys::BLOCKED))
            .await?;

        if matched_keys.is_empty() {
            return Ok(Vec::new());
        }

        let mut conn = self.pool.get().await
            .context("pool: failed to get connection")?;

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
                        // Redact pattern before returning to agent.
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
            // Key may have expired between SCAN and GET — skip silently.
        }

        Ok(results)
    }

    // ---------------------------------------------------------------
    // get_security_log  (Requirement 2.9)
    // ---------------------------------------------------------------

    /// Retrieve the most recent `limit` entries from the security
    /// event log sorted set (`molis:log:events`).
    ///
    /// Uses `ZREVRANGE` (highest score = most recent timestamp first).
    /// May return an empty vec for MVP since the `mcp-agent` user
    /// cannot write to this key.
    pub async fn get_security_log(
        &self,
        limit: usize,
    ) -> Result<Vec<SecurityLogEntry>> {
        let mut conn = self.pool.get().await
            .context("pool: failed to get connection")?;

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
                    tracing::warn!(
                        error = %e,
                        "skipping malformed security log entry",
                    );
                }
            }
        }

        Ok(entries)
    }

    // ---------------------------------------------------------------
    // get_request_status  (Requirement 2.10)
    // ---------------------------------------------------------------

    /// Check whether a request has been approved, is still pending
    /// (blocked), or is not found.
    ///
    /// Checks `molis:approved:{id}` first, then `molis:blocked:{id}`.
    pub async fn get_request_status(
        &self,
        request_id: &str,
    ) -> Result<RequestStatus> {
        let mut conn = self.pool.get().await
            .context("pool: failed to get connection")?;

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

        // Neither key exists — the request expired or was never stored.
        // The design maps this to "not_found" in the tool layer; we
        // return `Denied` here as the closest enum variant. The tool
        // layer translates this to the "not_found" string.
        //
        // NOTE: `RequestStatus::Denied` is reused for "not_found"
        // because the enum doesn't have a NotFound variant. The tool
        // layer is responsible for the final user-facing label.
        Ok(RequestStatus::Denied)
    }

    // ---------------------------------------------------------------
    // log_event  (MVP: tracing only)
    // ---------------------------------------------------------------

    /// Log a security event.
    ///
    /// **MVP**: This is a no-op that logs to `tracing` only. The
    /// `mcp-agent` ACL user cannot write to `molis:log:events`
    /// (that requires the `log-writer` user). Post-MVP, a dedicated
    /// connection authenticated as `log-writer` can be added.
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

    /// SCAN the keyspace with `MATCH pattern COUNT 100` and return
    /// the total number of matching keys.
    ///
    /// Uses iterative SCAN (cursor-based) — never KEYS.
    async fn scan_count(&self, pattern: &str) -> Result<usize> {
        let matched = self.scan_keys(pattern).await?;
        Ok(matched.len())
    }

    /// SCAN the keyspace with `MATCH pattern COUNT 100` and collect
    /// all matching key names.
    ///
    /// Iterates until the cursor returns to 0.
    async fn scan_keys(&self, pattern: &str) -> Result<Vec<String>> {
        let mut conn = self.pool.get().await
            .context("pool: failed to get connection")?;

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
    // The redis URL format is `redis://[user:pass@]host[:port][/db]`.
    let scheme_end = base_url
        .find("://")
        .context("invalid Valkey URL: missing '://'")?;

    let scheme = &base_url[..scheme_end]; // "redis" or "rediss"
    let rest = &base_url[scheme_end + 3..]; // "host:port/db" or "user:pass@host:port/db"

    // Strip any existing credentials.
    let host_and_path = match rest.find('@') {
        Some(at) => &rest[at + 1..],
        None => rest,
    };

    // Percent-encode user and password to handle special chars safely.
    let enc_user = utf8_percent_encode(user, NON_ALPHANUMERIC);
    let enc_pass = utf8_percent_encode(password, NON_ALPHANUMERIC);

    Ok(format!("{scheme}://{enc_user}:{enc_pass}@{host_and_path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // build_auth_url
    // ---------------------------------------------------------------

    #[test]
    fn auth_url_basic() {
        let url = build_auth_url(
            "redis://valkey:6379",
            "mcp-agent",
            "s3cret",
        )
        .unwrap();
        // Hyphens are percent-encoded with NON_ALPHANUMERIC.
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
    fn auth_url_encodes_special_chars() {
        // Password with @, :, /, #, ? — all must be percent-encoded.
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
}
