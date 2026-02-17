//! Application state wrapping a Fred Redis/Valkey client with mTLS via rustls.

use anyhow::{Context, Result};
use fred::prelude::*;
use fred::types::config::{TlsConfig, TlsConnector, TlsHostMapping};
use fred::types::scan::Scanner;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use polis_common::{
    approved_key, blocked_key,
    redis_keys::{keys, ttl},
    BlockedRequest, RequestStatus, SecurityLevel, SecurityLogEntry,
};

const DEFAULT_VALKEY_CA_PATH: &str = "/etc/valkey/tls/ca.crt";
const DEFAULT_VALKEY_CLIENT_CERT_PATH: &str = "/etc/valkey/tls/client.crt";
const DEFAULT_VALKEY_CLIENT_KEY_PATH: &str = "/etc/valkey/tls/client.key";

#[derive(Clone)]
pub struct AppState {
    client: Client,
}

impl AppState {
    pub async fn new(valkey_url: &str, user: &str, password: &str) -> Result<Self> {
        let ca_path = std::env::var("polis_AGENT_VALKEY_CA")
            .unwrap_or_else(|_| DEFAULT_VALKEY_CA_PATH.to_string());
        let cert_path = std::env::var("polis_AGENT_VALKEY_CLIENT_CERT")
            .unwrap_or_else(|_| DEFAULT_VALKEY_CLIENT_CERT_PATH.to_string());
        let key_path = std::env::var("polis_AGENT_VALKEY_CLIENT_KEY")
            .unwrap_or_else(|_| DEFAULT_VALKEY_CLIENT_KEY_PATH.to_string());

        // Load CA certificate
        let ca_file =
            File::open(&ca_path).with_context(|| format!("failed to open CA cert: {}", ca_path))?;
        let mut ca_reader = BufReader::new(ca_file);
        let ca_certs = rustls_pemfile::certs(&mut ca_reader)
            .collect::<Result<Vec<_>, _>>()
            .context("failed to parse CA cert")?;

        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_certs {
            root_store
                .add(cert)
                .context("failed to add CA cert to root store")?;
        }

        // Load client certificate
        let cert_file = File::open(&cert_path)
            .with_context(|| format!("failed to open client cert: {}", cert_path))?;
        let mut cert_reader = BufReader::new(cert_file);
        let client_certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .context("failed to parse client cert")?;

        // Load client private key
        let key_file = File::open(&key_path)
            .with_context(|| format!("failed to open client key: {}", key_path))?;
        let mut key_reader = BufReader::new(key_file);
        let client_key = rustls_pemfile::private_key(&mut key_reader)
            .context("failed to parse client key")?
            .context("no private key found in file")?;

        // Build rustls ClientConfig with mTLS
        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(client_certs, client_key)
            .context("failed to build TLS config with client auth")?;

        // Configure Fred with rustls
        let mut config = Config::from_url(valkey_url)?;
        config.tls = Some(TlsConfig {
            connector: TlsConnector::Rustls(Arc::new(tls_config).into()),
            hostnames: TlsHostMapping::None,
        });
        config.username = Some(user.to_string());
        config.password = Some(password.to_string());

        let client = Builder::from_config(config)
            .with_connection_config(|conn_config| {
                // Set connection timeout to prevent hanging
                conn_config.connection_timeout = std::time::Duration::from_secs(5);
                // Set internal command timeout
                conn_config.internal_command_timeout = std::time::Duration::from_secs(10);
            })
            .set_policy(ReconnectPolicy::new_exponential(0, 100, 5000, 5))
            .build()?;

        client.init().await?;

        client
            .ping::<String>(None)
            .await
            .context("Valkey startup PING failed")?;

        tracing::info!(
            ca = %ca_path,
            cert = %cert_path,
            key = %key_path,
            "Valkey connection ready with mTLS (rustls)"
        );

        Ok(Self { client })
    }

    pub async fn store_blocked_request(&self, request: &BlockedRequest) -> Result<()> {
        let key = blocked_key(&request.request_id);
        let json = serde_json::to_string(request)?;

        self.client
            .set::<(), _, _>(
                &key,
                json,
                Some(Expiration::EX(ttl::BLOCKED_REQUEST_SECS as i64)),
                None,
                false,
            )
            .await?;

        tracing::info!(
            request_id = %request.request_id,
            "stored blocked request (TTL={}s)",
            ttl::BLOCKED_REQUEST_SECS,
        );
        Ok(())
    }

    pub async fn count_pending_approvals(&self) -> Result<usize> {
        self.scan_count(&format!("{}*", keys::BLOCKED)).await
    }

    pub async fn count_recent_approvals(&self) -> Result<usize> {
        self.scan_count(&format!("{}*", keys::APPROVED)).await
    }

    pub async fn get_security_level(&self) -> Result<SecurityLevel> {
        let raw: Option<String> = self.client.get(keys::SECURITY_LEVEL).await?;

        match raw {
            Some(val) => {
                let level: SecurityLevel =
                    serde_json::from_str(&format!("\"{}\"", val)).unwrap_or_default();
                Ok(level)
            }
            None => Ok(SecurityLevel::default()),
        }
    }

    pub async fn get_pending_approvals(&self) -> Result<Vec<BlockedRequest>> {
        let matched_keys = self.scan_keys(&format!("{}*", keys::BLOCKED)).await?;

        if matched_keys.is_empty() {
            return Ok(Vec::new());
        }

        let values: Vec<Value> = self.client.mget(matched_keys.clone()).await?;

        let mut results = Vec::with_capacity(values.len());
        for (i, value) in values.into_iter().enumerate() {
            if let Some(json) = value.as_str() {
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

    pub async fn check_request_status(&self, request_id: &str) -> Result<RequestStatus> {
        let approved_key = approved_key(request_id);
        let blocked_key = blocked_key(request_id);

        let approved_exists: bool = self.client.exists(&approved_key).await?;

        if approved_exists {
            return Ok(RequestStatus::Approved);
        }

        let blocked_exists: bool = self.client.exists(&blocked_key).await?;

        if blocked_exists {
            Ok(RequestStatus::Pending)
        } else {
            Ok(RequestStatus::Denied)
        }
    }

    pub async fn approve_request(&self, request_id: &str) -> Result<()> {
        let blocked_key = blocked_key(request_id);
        let approved_key = approved_key(request_id);

        let json: Option<String> = self.client.get(&blocked_key).await?;
        let json = json.context("blocked request not found")?;

        self.client
            .set::<(), _, _>(
                &approved_key,
                json,
                Some(Expiration::EX(ttl::APPROVED_REQUEST_SECS as i64)),
                None,
                false,
            )
            .await?;
        self.client.del::<(), _>(&blocked_key).await?;

        tracing::info!(request_id, "approved request");
        Ok(())
    }

    pub async fn log_security_event(&self, entry: &SecurityLogEntry) -> Result<()> {
        let json = serde_json::to_string(entry)?;
        let score = entry.timestamp.timestamp() as f64;

        self.client
            .zadd::<(), _, _>(
                keys::EVENT_LOG,
                None,
                None,
                false,
                false,
                (score, json.as_str()),
            )
            .await?;

        // Trim to last 1000 entries (remove oldest by rank)
        let count: i64 = self.client.zcard(keys::EVENT_LOG).await?;
        if count > 1000 {
            self.client
                .zremrangebyrank::<(), _>(keys::EVENT_LOG, 0, count - 1001)
                .await?;
        }

        Ok(())
    }

    pub async fn get_security_log(&self, limit: usize) -> Result<Vec<SecurityLogEntry>> {
        let entries: Vec<String> = self
            .client
            .zrevrange(keys::EVENT_LOG, 0, (limit as i64) - 1, false)
            .await?;

        let mut results = Vec::with_capacity(entries.len());
        for json in entries {
            match serde_json::from_str::<SecurityLogEntry>(&json) {
                Ok(entry) => results.push(entry),
                Err(e) => {
                    tracing::warn!(error = %e, "skipping malformed security log entry");
                }
            }
        }
        Ok(results)
    }

    async fn scan_count(&self, pattern: &str) -> Result<usize> {
        let keys = self.scan_keys(pattern).await?;
        Ok(keys.len())
    }

    async fn scan_keys(&self, pattern: &str) -> Result<Vec<String>> {
        use futures::stream::TryStreamExt;

        let mut keys = Vec::new();
        let mut stream = self.client.scan(pattern, Some(100), None);

        while let Some(mut page) = stream.try_next().await? {
            if let Some(results) = page.take_results() {
                for key in results {
                    keys.push(key.as_str_lossy().to_string());
                }
            }
        }

        Ok(keys)
    }
}
