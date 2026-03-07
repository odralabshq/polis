//! Governance state access for the control-plane server.
//!
//! Valkey key patterns (from `polis_common::redis_keys`):
//! - `polis:blocked:{request_id}` — JSON `BlockedRequest` values waiting for approval.
//! - `polis:approved:{request_id}` — temporary approved markers with a short TTL.
//! - `polis:config:security_level` — global security level string.
//! - `polis:config:auto_approve:{pattern}` — rule action string for a destination pattern.
//! - `polis:log:events` — sorted set of JSON security log entries.

#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::{fs::File, io::BufReader, sync::Arc};

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cp_api_types::{
    ActionResponse, BlockedItem, BlockedListResponse, EventItem, EventsResponse, LevelResponse,
    RuleCreateRequest, RuleItem, RulesResponse, StatusResponse,
};
use fred::types::scan::Scanner;
use fred::{
    interfaces::{ClientLike, KeysInterface, SortedSetsInterface},
    prelude::{Builder, Client, Config as FredConfig, Expiration, ReconnectPolicy, Value},
    types::config::{TlsConfig, TlsConnector, TlsHostMapping},
};
use polis_common::{
    AutoApproveAction, BlockReason, BlockedRequest, RequestStatus, SecurityLevel, SecurityLogEntry,
    approved_key, auto_approve_key, blocked_key,
    redis_keys::{keys, ttl},
};

use crate::{
    config::Config,
    error::{AppError, AppResult},
};

const EVENT_LOG_MAX_ENTRIES: usize = 1_000;

#[async_trait]
pub trait ValkeyClient: Clone + Send + Sync + 'static {
    async fn get_string(&self, key: &str) -> Result<Option<String>>;
    async fn set_string(&self, key: &str, value: &str) -> Result<()>;
    async fn set_string_ex(&self, key: &str, value: &str, ttl_secs: u64) -> Result<()>;
    async fn del(&self, key: &str) -> Result<()>;
    async fn exists(&self, key: &str) -> Result<bool>;
    async fn mget_strings(&self, keys: Vec<String>) -> Result<Vec<Option<String>>>;
    async fn scan_keys(&self, pattern: &str) -> Result<Vec<String>>;
    async fn zadd(&self, key: &str, score: f64, value: &str) -> Result<()>;
    async fn zrevrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<String>>;
    async fn zcard(&self, key: &str) -> Result<i64>;
    async fn zremrangebyrank(&self, key: &str, start: i64, stop: i64) -> Result<()>;
    async fn ping(&self) -> Result<()>;
}

#[async_trait]
pub trait GovernanceStore: Clone + Send + Sync + 'static {
    async fn get_status(&self) -> AppResult<StatusResponse>;
    async fn list_blocked(&self) -> AppResult<BlockedListResponse>;
    async fn approve(&self, request_id: &str) -> AppResult<ActionResponse>;
    async fn deny(&self, request_id: &str) -> AppResult<ActionResponse>;
    async fn list_events(&self, limit: usize) -> AppResult<EventsResponse>;
    async fn get_security_level(&self) -> AppResult<LevelResponse>;
    async fn set_security_level(&self, level: &str) -> AppResult<LevelResponse>;
    async fn list_rules(&self) -> AppResult<RulesResponse>;
    async fn add_rule(&self, pattern: &str, action: &str) -> AppResult<ActionResponse>;
    async fn add_rule_from_request(
        &self,
        request: &RuleCreateRequest,
    ) -> AppResult<ActionResponse> {
        self.add_rule(&request.pattern, &request.action).await
    }
    async fn delete_rule(&self, pattern: &str) -> AppResult<ActionResponse>;
}

#[derive(Clone)]
pub struct FredValkeyClient {
    client: Client,
}

impl FredValkeyClient {
    /// Create a Fred client configured for Valkey mTLS.
    ///
    /// # Errors
    ///
    /// Returns an error if certificates cannot be loaded or the connection
    /// cannot be established.
    pub async fn connect(config: &Config) -> Result<Self> {
        let password = config.read_password()?;

        let mut ca_reader = BufReader::new(
            File::open(&config.valkey_ca)
                .with_context(|| format!("failed to open {}", config.valkey_ca))?,
        );
        let ca_certs = rustls_pemfile::certs(&mut ca_reader)
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to parse Valkey CA certificate")?;

        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_certs {
            root_store
                .add(cert)
                .context("failed to add CA certificate to root store")?;
        }

        let mut cert_reader = BufReader::new(
            File::open(&config.valkey_client_cert)
                .with_context(|| format!("failed to open {}", config.valkey_client_cert))?,
        );
        let client_certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to parse Valkey client certificate")?;

        let mut key_reader = BufReader::new(
            File::open(&config.valkey_client_key)
                .with_context(|| format!("failed to open {}", config.valkey_client_key))?,
        );
        let client_key = rustls_pemfile::private_key(&mut key_reader)
            .context("failed to parse Valkey client key")?
            .context("no private key found in Valkey client key file")?;

        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(client_certs, client_key)
            .context("failed to build rustls client config")?;

        let mut fred_config =
            FredConfig::from_url(&config.valkey_url).context("invalid Valkey URL")?;
        fred_config.tls = Some(TlsConfig {
            connector: TlsConnector::Rustls(Arc::new(tls_config).into()),
            hostnames: TlsHostMapping::None,
        });
        fred_config.username = Some(config.valkey_user.clone());
        fred_config.password = Some(password);

        let client = Builder::from_config(fred_config)
            .with_connection_config(|connection| {
                connection.connection_timeout = std::time::Duration::from_secs(5);
                connection.internal_command_timeout = std::time::Duration::from_secs(10);
            })
            .set_policy(ReconnectPolicy::new_exponential(0, 100, 5_000, 5))
            .build()
            .context("failed to build Fred client")?;

        let _connection_task = client
            .init()
            .await
            .context("failed to initialize Fred client")?;
        let _: String = client
            .ping(None)
            .await
            .context("Valkey startup PING failed")?;

        Ok(Self { client })
    }
}

#[async_trait]
impl ValkeyClient for FredValkeyClient {
    async fn get_string(&self, key: &str) -> Result<Option<String>> {
        let value: Option<String> = self
            .client
            .get(key)
            .await
            .with_context(|| format!("failed to GET {key}"))?;
        Ok(value)
    }

    async fn set_string(&self, key: &str, value: &str) -> Result<()> {
        self.client
            .set::<(), _, _>(key, value, None, None, false)
            .await
            .with_context(|| format!("failed to SET {key}"))
    }

    async fn set_string_ex(&self, key: &str, value: &str, ttl_secs: u64) -> Result<()> {
        let ttl_secs = i64::try_from(ttl_secs).context("ttl too large for Valkey expiration")?;
        self.client
            .set::<(), _, _>(key, value, Some(Expiration::EX(ttl_secs)), None, false)
            .await
            .with_context(|| format!("failed to SETEX {key}"))
    }

    async fn del(&self, key: &str) -> Result<()> {
        self.client
            .del::<(), _>(key)
            .await
            .with_context(|| format!("failed to DEL {key}"))
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let exists: bool = self
            .client
            .exists(key)
            .await
            .with_context(|| format!("failed to EXISTS {key}"))?;
        Ok(exists)
    }

    async fn mget_strings(&self, keys: Vec<String>) -> Result<Vec<Option<String>>> {
        let values: Vec<Value> = self
            .client
            .mget(keys.clone())
            .await
            .context("failed to MGET keys")?;

        Ok(values
            .into_iter()
            .map(|value| value.as_str().map(|text| text.to_string()))
            .collect())
    }

    async fn scan_keys(&self, pattern: &str) -> Result<Vec<String>> {
        use futures::stream::TryStreamExt;

        let mut keys = Vec::new();
        let mut stream = self.client.scan(pattern, Some(100), None);

        while let Some(mut page) = stream.try_next().await.context("failed to SCAN keys")? {
            if let Some(results) = page.take_results() {
                for key in results {
                    keys.push(key.as_str_lossy().to_string());
                }
            }
        }

        Ok(keys)
    }

    async fn zadd(&self, key: &str, score: f64, value: &str) -> Result<()> {
        self.client
            .zadd::<(), _, _>(key, None, None, false, false, (score, value))
            .await
            .with_context(|| format!("failed to ZADD {key}"))
    }

    async fn zrevrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<String>> {
        self.client
            .zrevrange(key, start, stop, false)
            .await
            .with_context(|| format!("failed to ZREVRANGE {key}"))
    }

    async fn zcard(&self, key: &str) -> Result<i64> {
        self.client
            .zcard(key)
            .await
            .with_context(|| format!("failed to ZCARD {key}"))
    }

    async fn zremrangebyrank(&self, key: &str, start: i64, stop: i64) -> Result<()> {
        self.client
            .zremrangebyrank::<(), _>(key, start, stop)
            .await
            .with_context(|| format!("failed to ZREMRANGEBYRANK {key}"))
    }

    async fn ping(&self) -> Result<()> {
        self.client
            .ping::<String>(None)
            .await
            .context("failed to PING")?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct GovernanceState<C> {
    client: C,
}

pub type AppState = GovernanceState<FredValkeyClient>;

impl<C> GovernanceState<C> {
    #[must_use]
    pub fn new_with_client(client: C) -> Self {
        Self { client }
    }
}

impl GovernanceState<FredValkeyClient> {
    /// Build the production app state from the process configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the Valkey client cannot be initialized.
    pub async fn new(config: &Config) -> Result<Self> {
        let client = FredValkeyClient::connect(config).await?;
        Ok(Self { client })
    }
}

impl<C> GovernanceState<C>
where
    C: ValkeyClient,
{
    fn dependency_error(operation: &str, error: &anyhow::Error) -> AppError {
        AppError::DependencyUnavailable(format!("{operation}: {error}"))
    }

    fn internal_error<E>(operation: &'static str, error: E) -> AppError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        AppError::Internal(anyhow::Error::new(error).context(operation))
    }

    fn event_count(value: i64) -> AppResult<usize> {
        usize::try_from(value)
            .map_err(|error| Self::internal_error("event count exceeded supported range", error))
    }

    fn sorted_set_index(value: usize, operation: &'static str) -> AppResult<i64> {
        i64::try_from(value).map_err(|error| Self::internal_error(operation, error))
    }

    fn sorted_set_score(value: i64) -> AppResult<f64> {
        value.to_string().parse::<f64>().map_err(|error| {
            Self::internal_error("failed to convert event timestamp to score", error)
        })
    }

    async fn scan_count(&self, pattern: &str) -> AppResult<usize> {
        let keys = self
            .client
            .scan_keys(pattern)
            .await
            .map_err(|error| Self::dependency_error("failed to scan keys", &error))?;
        Ok(keys.len())
    }

    async fn append_event(&self, entry: &SecurityLogEntry) -> AppResult<()> {
        let serialized = serde_json::to_string(entry).map_err(|error| {
            AppError::Internal(anyhow::Error::new(error).context("failed to serialize event entry"))
        })?;
        let score = Self::sorted_set_score(entry.timestamp.timestamp())?;

        self.client
            .zadd(keys::EVENT_LOG, score, &serialized)
            .await
            .map_err(|error| Self::dependency_error("failed to append event log entry", &error))?;

        let count = self
            .client
            .zcard(keys::EVENT_LOG)
            .await
            .map_err(|error| Self::dependency_error("failed to count event log entries", &error))
            .and_then(Self::event_count)?;
        if count > EVENT_LOG_MAX_ENTRIES {
            let trim_stop = Self::sorted_set_index(
                count - EVENT_LOG_MAX_ENTRIES - 1,
                "event log trim index exceeded supported range",
            )?;
            self.client
                .zremrangebyrank(keys::EVENT_LOG, 0, trim_stop)
                .await
                .map_err(|error| {
                    Self::dependency_error("failed to trim event log entries", &error)
                })?;
        }

        Ok(())
    }

    async fn fetch_blocked_request(&self, request_id: &str) -> AppResult<BlockedRequest> {
        polis_common::validate_request_id(request_id)
            .map_err(|message| AppError::Validation(message.to_string()))?;

        let json = self
            .client
            .get_string(&blocked_key(request_id))
            .await
            .map_err(|error| Self::dependency_error("failed to fetch blocked request", &error))?
            .ok_or_else(|| {
                AppError::NotFound(format!("no blocked request found for {request_id}"))
            })?;

        serde_json::from_str(&json).map_err(|error| {
            AppError::Internal(anyhow::Error::new(error).context("failed to parse blocked request"))
        })
    }

    fn parse_action(action: &str) -> AppResult<AutoApproveAction> {
        match action.to_ascii_lowercase().as_str() {
            "allow" => Ok(AutoApproveAction::Allow),
            "prompt" => Ok(AutoApproveAction::Prompt),
            "block" => Ok(AutoApproveAction::Block),
            _ => Err(AppError::Validation(
                "invalid auto-approve action: expected allow, prompt, or block".to_string(),
            )),
        }
    }

    fn parse_security_level(level: &str) -> AppResult<SecurityLevel> {
        match level.to_ascii_lowercase().as_str() {
            "relaxed" => Ok(SecurityLevel::Relaxed),
            "balanced" => Ok(SecurityLevel::Balanced),
            "strict" => Ok(SecurityLevel::Strict),
            _ => Err(AppError::Validation(
                "invalid security level: expected relaxed, balanced, or strict".to_string(),
            )),
        }
    }

    fn level_to_string(level: SecurityLevel) -> String {
        match level {
            SecurityLevel::Relaxed => "relaxed".to_string(),
            SecurityLevel::Balanced => "balanced".to_string(),
            SecurityLevel::Strict => "strict".to_string(),
        }
    }

    fn action_to_string(action: &AutoApproveAction) -> String {
        match action {
            AutoApproveAction::Allow => "allow".to_string(),
            AutoApproveAction::Prompt => "prompt".to_string(),
            AutoApproveAction::Block => "block".to_string(),
        }
    }

    fn reason_to_string(reason: &BlockReason) -> String {
        match reason {
            BlockReason::CredentialDetected => "credential_detected".to_string(),
            BlockReason::MalwareDomain => "malware_domain".to_string(),
            BlockReason::UrlBlocked => "url_blocked".to_string(),
            BlockReason::FileInfected => "file_infected".to_string(),
        }
    }

    fn status_to_string(status: RequestStatus) -> String {
        match status {
            RequestStatus::Pending => "pending".to_string(),
            RequestStatus::Approved => "approved".to_string(),
            RequestStatus::Denied => "denied".to_string(),
        }
    }

    fn blocked_item_from_request(request: BlockedRequest) -> BlockedItem {
        BlockedItem {
            request_id: request.request_id,
            reason: Self::reason_to_string(&request.reason),
            destination: request.destination,
            blocked_at: request.blocked_at,
            status: Self::status_to_string(request.status),
        }
    }

    fn parse_event_item(raw: &str) -> Option<EventItem> {
        if let Ok(entry) = serde_json::from_str::<SecurityLogEntry>(raw) {
            return Some(EventItem {
                timestamp: entry.timestamp,
                event_type: entry.event_type,
                request_id: entry.request_id,
                details: entry.details,
            });
        }

        let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
        let event_type = value.get("event_type")?.as_str()?.to_string();
        let request_id = value
            .get("request_id")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);

        let timestamp = value
            .get("timestamp")
            .and_then(parse_event_timestamp)
            .unwrap_or_else(Utc::now);

        let details = value
            .get("details")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string)
            .or_else(|| legacy_details(&value))
            .unwrap_or_else(|| event_type.clone());

        Some(EventItem {
            timestamp,
            event_type,
            request_id,
            details,
        })
    }
}

#[async_trait]
impl<C> GovernanceStore for GovernanceState<C>
where
    C: ValkeyClient,
{
    async fn get_status(&self) -> AppResult<StatusResponse> {
        let security_level = self.get_security_level().await?.level;
        let pending_count = self.scan_count(&format!("{}:*", keys::BLOCKED)).await?;
        let recent_approvals = self.scan_count(&format!("{}:*", keys::APPROVED)).await?;
        let events_count = self
            .client
            .zcard(keys::EVENT_LOG)
            .await
            .map_err(|error| Self::dependency_error("failed to count event log entries", &error))
            .and_then(Self::event_count)?;

        Ok(StatusResponse {
            security_level,
            pending_count,
            recent_approvals,
            events_count,
        })
    }

    async fn list_blocked(&self) -> AppResult<BlockedListResponse> {
        let keys = self
            .client
            .scan_keys(&format!("{}:*", keys::BLOCKED))
            .await
            .map_err(|error| Self::dependency_error("failed to scan blocked requests", &error))?;
        if keys.is_empty() {
            return Ok(BlockedListResponse { items: Vec::new() });
        }

        let values =
            self.client.mget_strings(keys).await.map_err(|error| {
                Self::dependency_error("failed to read blocked requests", &error)
            })?;

        let mut items = values
            .into_iter()
            .flatten()
            .filter_map(|json| match serde_json::from_str::<BlockedRequest>(&json) {
                Ok(mut request) => {
                    request.pattern = None;
                    Some(Self::blocked_item_from_request(request))
                }
                Err(error) => {
                    tracing::warn!(%error, "skipping malformed blocked request");
                    None
                }
            })
            .collect::<Vec<_>>();

        items.sort_by(|left, right| right.blocked_at.cmp(&left.blocked_at));

        Ok(BlockedListResponse { items })
    }

    async fn approve(&self, request_id: &str) -> AppResult<ActionResponse> {
        let blocked_request = self.fetch_blocked_request(request_id).await?;

        let entry = SecurityLogEntry {
            timestamp: Utc::now(),
            event_type: "approved_via_control_plane".to_string(),
            request_id: Some(request_id.to_string()),
            details: format!(
                "Approved blocked request to {}",
                blocked_request.destination
            ),
        };
        self.append_event(&entry).await?;

        self.client
            .set_string_ex(
                &approved_key(request_id),
                "approved",
                ttl::APPROVED_REQUEST_SECS,
            )
            .await
            .map_err(|error| {
                Self::dependency_error("failed to create approved marker for request", &error)
            })?;
        self.client
            .del(&blocked_key(request_id))
            .await
            .map_err(|error| {
                Self::dependency_error("failed to remove blocked request after approval", &error)
            })?;

        Ok(ActionResponse {
            message: format!("approved {request_id}"),
        })
    }

    async fn deny(&self, request_id: &str) -> AppResult<ActionResponse> {
        let blocked_request = self.fetch_blocked_request(request_id).await?;

        let entry = SecurityLogEntry {
            timestamp: Utc::now(),
            event_type: "denied_via_control_plane".to_string(),
            request_id: Some(request_id.to_string()),
            details: format!("Denied blocked request to {}", blocked_request.destination),
        };
        self.append_event(&entry).await?;

        self.client
            .del(&blocked_key(request_id))
            .await
            .map_err(|error| {
                Self::dependency_error("failed to remove blocked request after denial", &error)
            })?;

        Ok(ActionResponse {
            message: format!("denied {request_id}"),
        })
    }

    async fn list_events(&self, limit: usize) -> AppResult<EventsResponse> {
        let safe_limit = Self::sorted_set_index(
            limit.clamp(1, 200),
            "event query limit exceeded supported range",
        )?;
        let values = self
            .client
            .zrevrange(keys::EVENT_LOG, 0, safe_limit - 1)
            .await
            .map_err(|error| Self::dependency_error("failed to list security events", &error))?;

        let events = values
            .into_iter()
            .filter_map(|raw| {
                if let Some(event) = Self::parse_event_item(&raw) {
                    Some(event)
                } else {
                    tracing::warn!("skipping malformed event log entry");
                    None
                }
            })
            .collect();

        Ok(EventsResponse { events })
    }

    async fn get_security_level(&self) -> AppResult<LevelResponse> {
        let raw = self
            .client
            .get_string(keys::SECURITY_LEVEL)
            .await
            .map_err(|error| Self::dependency_error("failed to read security level", &error))?;

        let level = raw
            .map(|value| polis_common::migrate_security_level(&value).0)
            .unwrap_or_default();

        Ok(LevelResponse {
            level: Self::level_to_string(level),
        })
    }

    async fn set_security_level(&self, level: &str) -> AppResult<LevelResponse> {
        let parsed = Self::parse_security_level(level)?;
        let normalized = Self::level_to_string(parsed);

        self.client
            .set_string(keys::SECURITY_LEVEL, &normalized)
            .await
            .map_err(|error| Self::dependency_error("failed to update security level", &error))?;

        let entry = SecurityLogEntry {
            timestamp: Utc::now(),
            event_type: "level_changed".to_string(),
            request_id: None,
            details: format!("Security level changed to {normalized}"),
        };
        self.append_event(&entry).await?;

        Ok(LevelResponse { level: normalized })
    }

    async fn list_rules(&self) -> AppResult<RulesResponse> {
        let rule_keys = self
            .client
            .scan_keys(&format!("{}:*", keys::AUTO_APPROVE))
            .await
            .map_err(|error| Self::dependency_error("failed to scan auto-approve rules", &error))?;
        if rule_keys.is_empty() {
            return Ok(RulesResponse { rules: Vec::new() });
        }

        let values = self
            .client
            .mget_strings(rule_keys.clone())
            .await
            .map_err(|error| Self::dependency_error("failed to read auto-approve rules", &error))?;

        let mut rules = rule_keys
            .into_iter()
            .zip(values)
            .filter_map(|(key, action)| {
                let pattern = key.strip_prefix(&format!("{}:", keys::AUTO_APPROVE))?;
                let action = action?;
                let parsed = Self::parse_action(&action).ok()?;
                Some(RuleItem {
                    pattern: pattern.to_string(),
                    action: Self::action_to_string(&parsed),
                })
            })
            .collect::<Vec<_>>();

        rules.sort_by(|left, right| left.pattern.cmp(&right.pattern));

        Ok(RulesResponse { rules })
    }

    async fn add_rule(&self, pattern: &str, action: &str) -> AppResult<ActionResponse> {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return Err(AppError::Validation(
                "rule pattern must not be empty".to_string(),
            ));
        }

        let parsed = Self::parse_action(action)?;
        let normalized = Self::action_to_string(&parsed);

        self.client
            .set_string(&auto_approve_key(pattern), &normalized)
            .await
            .map_err(|error| {
                Self::dependency_error("failed to create auto-approve rule", &error)
            })?;

        let entry = SecurityLogEntry {
            timestamp: Utc::now(),
            event_type: "rule_added".to_string(),
            request_id: None,
            details: format!("Added auto-approve rule {pattern} -> {normalized}"),
        };
        self.append_event(&entry).await?;

        Ok(ActionResponse {
            message: format!("auto-approve rule set: {pattern} -> {normalized}"),
        })
    }

    async fn delete_rule(&self, pattern: &str) -> AppResult<ActionResponse> {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return Err(AppError::Validation(
                "rule pattern must not be empty".to_string(),
            ));
        }

        self.client
            .del(&auto_approve_key(pattern))
            .await
            .map_err(|error| {
                Self::dependency_error("failed to delete auto-approve rule", &error)
            })?;

        let entry = SecurityLogEntry {
            timestamp: Utc::now(),
            event_type: "rule_deleted".to_string(),
            request_id: None,
            details: format!("Deleted auto-approve rule {pattern}"),
        };
        self.append_event(&entry).await?;

        Ok(ActionResponse {
            message: format!("deleted auto-approve rule {pattern}"),
        })
    }
}

fn parse_event_timestamp(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    if let Some(timestamp) = value.as_i64() {
        return DateTime::<Utc>::from_timestamp(timestamp, 0);
    }
    if let Some(timestamp) = value.as_str()
        && let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp)
    {
        return Some(parsed.with_timezone(&Utc));
    }
    None
}

fn legacy_details(value: &serde_json::Value) -> Option<String> {
    let raw = value.get("blocked_request")?.as_str()?;
    if let Ok(request) = serde_json::from_str::<BlockedRequest>(raw) {
        return Some(format!("{} ({})", request.destination, request.request_id));
    }
    Some(raw.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        sync::{Arc, Mutex},
    };

    use chrono::{TimeZone, Utc};

    use super::*;

    type StringMap = HashMap<String, String>;
    type SortedSetEntries = Vec<(f64, String)>;
    type SortedSetMap = HashMap<String, SortedSetEntries>;

    #[derive(Clone, Default)]
    struct FakeValkeyClient {
        strings: Arc<Mutex<StringMap>>,
        zsets: Arc<Mutex<SortedSetMap>>,
        fail_ops: Arc<Mutex<HashSet<String>>>,
    }

    impl FakeValkeyClient {
        fn seed_string(&self, key: impl Into<String>, value: impl Into<String>) {
            self.strings
                .lock()
                .expect("strings lock")
                .insert(key.into(), value.into());
        }

        fn seed_event(&self, score: f64, value: impl Into<String>) {
            self.zsets
                .lock()
                .expect("zsets lock")
                .entry(keys::EVENT_LOG.to_string())
                .or_default()
                .push((score, value.into()));
        }

        fn fail(&self, op: &str) {
            self.fail_ops
                .lock()
                .expect("fail_ops lock")
                .insert(op.to_string());
        }

        fn should_fail(&self, op: &str) -> Result<()> {
            if self.fail_ops.lock().expect("fail_ops lock").contains(op) {
                anyhow::bail!("{op} failed");
            }
            Ok(())
        }

        fn all_keys(&self) -> Vec<String> {
            let mut keys = self
                .strings
                .lock()
                .expect("strings lock")
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            keys.extend(
                self.zsets
                    .lock()
                    .expect("zsets lock")
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            keys
        }

        fn index_from_i64(value: i64) -> usize {
            if value <= 0 {
                0
            } else {
                match usize::try_from(value) {
                    Ok(value) => value,
                    Err(_) => usize::MAX,
                }
            }
        }

        fn i64_from_usize(value: usize) -> i64 {
            i64::try_from(value).unwrap_or(i64::MAX)
        }
    }

    #[allow(async_fn_in_trait)]
    #[async_trait]
    impl ValkeyClient for FakeValkeyClient {
        async fn get_string(&self, key: &str) -> Result<Option<String>> {
            self.should_fail("get_string")?;
            Ok(self.strings.lock().expect("strings lock").get(key).cloned())
        }

        async fn set_string(&self, key: &str, value: &str) -> Result<()> {
            self.should_fail("set_string")?;
            self.strings
                .lock()
                .expect("strings lock")
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        async fn set_string_ex(&self, key: &str, value: &str, _ttl_secs: u64) -> Result<()> {
            self.should_fail("set_string_ex")?;
            self.strings
                .lock()
                .expect("strings lock")
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        async fn del(&self, key: &str) -> Result<()> {
            self.should_fail("del")?;
            self.strings.lock().expect("strings lock").remove(key);
            Ok(())
        }

        async fn exists(&self, key: &str) -> Result<bool> {
            self.should_fail("exists")?;
            Ok(self.strings.lock().expect("strings lock").contains_key(key))
        }

        async fn mget_strings(&self, keys: Vec<String>) -> Result<Vec<Option<String>>> {
            self.should_fail("mget_strings")?;
            let map = self.strings.lock().expect("strings lock");
            Ok(keys
                .into_iter()
                .map(|key| map.get(&key).cloned())
                .collect::<Vec<_>>())
        }

        async fn scan_keys(&self, pattern: &str) -> Result<Vec<String>> {
            self.should_fail("scan_keys")?;
            let keys = self.all_keys();
            let matched = if let Some(prefix) = pattern.strip_suffix('*') {
                keys.into_iter()
                    .filter(|key| key.starts_with(prefix))
                    .collect::<Vec<_>>()
            } else {
                keys.into_iter()
                    .filter(|key| key == pattern)
                    .collect::<Vec<_>>()
            };
            Ok(matched)
        }

        async fn zadd(&self, key: &str, score: f64, value: &str) -> Result<()> {
            self.should_fail("zadd")?;
            let mut sets = self.zsets.lock().expect("zsets lock");
            let entry = sets.entry(key.to_string()).or_default();
            entry.push((score, value.to_string()));
            entry.sort_by(|left, right| left.0.total_cmp(&right.0));
            Ok(())
        }

        async fn zrevrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<String>> {
            self.should_fail("zrevrange")?;
            let entry = self
                .zsets
                .lock()
                .expect("zsets lock")
                .get(key)
                .cloned()
                .unwrap_or_default();
            let reversed = entry
                .into_iter()
                .rev()
                .map(|(_, value)| value)
                .collect::<Vec<_>>();
            if reversed.is_empty() {
                return Ok(Vec::new());
            }

            let start = Self::index_from_i64(start);
            let stop = Self::index_from_i64(stop);
            Ok(reversed
                .into_iter()
                .skip(start)
                .take(stop.saturating_sub(start) + 1)
                .collect())
        }

        async fn zcard(&self, key: &str) -> Result<i64> {
            self.should_fail("zcard")?;
            Ok(self
                .zsets
                .lock()
                .expect("zsets lock")
                .get(key)
                .map_or(0, |items| Self::i64_from_usize(items.len())))
        }

        async fn zremrangebyrank(&self, key: &str, start: i64, stop: i64) -> Result<()> {
            self.should_fail("zremrangebyrank")?;
            let mut sets = self.zsets.lock().expect("zsets lock");
            let Some(items) = sets.get_mut(key) else {
                return Ok(());
            };
            let start = Self::index_from_i64(start);
            let stop = Self::index_from_i64(stop);
            if start >= items.len() {
                return Ok(());
            }
            let end = stop.min(items.len().saturating_sub(1));
            items.drain(start..=end);
            Ok(())
        }

        async fn ping(&self) -> Result<()> {
            self.should_fail("ping")
        }
    }

    fn blocked_request(id: &str, destination: &str, minute: u32) -> BlockedRequest {
        BlockedRequest {
            request_id: id.to_string(),
            reason: BlockReason::CredentialDetected,
            destination: destination.to_string(),
            pattern: Some("*.example.com".to_string()),
            blocked_at: Utc
                .with_ymd_and_hms(2026, 3, 5, 19, minute, 0)
                .single()
                .expect("valid timestamp"),
            status: RequestStatus::Pending,
        }
    }

    fn store_with_blocked(client: &FakeValkeyClient, request: &BlockedRequest) {
        client.seed_string(
            blocked_key(&request.request_id),
            serde_json::to_string(request).expect("serialize request"),
        );
    }

    #[tokio::test]
    async fn list_blocked_redacts_pattern_and_sorts_newest_first() {
        let client = FakeValkeyClient::default();
        store_with_blocked(
            &client,
            &blocked_request("req-abc12345", "https://a.example", 1),
        );
        store_with_blocked(
            &client,
            &blocked_request("req-def67890", "https://b.example", 2),
        );
        let store = GovernanceState::new_with_client(client);

        let response = store.list_blocked().await.expect("list blocked");

        assert_eq!(response.items.len(), 2);
        assert_eq!(response.items[0].request_id, "req-def67890");
        assert_eq!(response.items[0].reason, "credential_detected");
        assert_eq!(response.items[0].status, "pending");
    }

    #[tokio::test]
    async fn approve_moves_request_and_logs_event() {
        let client = FakeValkeyClient::default();
        store_with_blocked(
            &client,
            &blocked_request("req-abc12345", "https://a.example", 1),
        );
        let store = GovernanceState::new_with_client(client.clone());

        let response = store.approve("req-abc12345").await.expect("approve");

        assert_eq!(response.message, "approved req-abc12345");
        assert!(
            !client
                .strings
                .lock()
                .expect("strings lock")
                .contains_key(&blocked_key("req-abc12345"))
        );
        assert_eq!(
            client
                .strings
                .lock()
                .expect("strings lock")
                .get(&approved_key("req-abc12345"))
                .cloned(),
            Some("approved".to_string())
        );
        assert_eq!(
            client
                .zcard(keys::EVENT_LOG)
                .await
                .expect("zcard event log"),
            1
        );
    }

    #[tokio::test]
    async fn approve_missing_request_returns_not_found() {
        let store = GovernanceState::new_with_client(FakeValkeyClient::default());

        let error = store
            .approve("req-abc12345")
            .await
            .expect_err("missing request");
        assert!(matches!(error, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn deny_removes_request_and_logs_event() {
        let client = FakeValkeyClient::default();
        store_with_blocked(
            &client,
            &blocked_request("req-abc12345", "https://a.example", 1),
        );
        let store = GovernanceState::new_with_client(client.clone());

        let response = store.deny("req-abc12345").await.expect("deny");

        assert_eq!(response.message, "denied req-abc12345");
        assert!(
            !client
                .strings
                .lock()
                .expect("strings lock")
                .contains_key(&blocked_key("req-abc12345"))
        );
        assert_eq!(
            client
                .zcard(keys::EVENT_LOG)
                .await
                .expect("zcard event log"),
            1
        );
    }

    #[tokio::test]
    async fn get_and_set_security_level_roundtrip() {
        let client = FakeValkeyClient::default();
        let store = GovernanceState::new_with_client(client);

        assert_eq!(
            store
                .get_security_level()
                .await
                .expect("default level")
                .level,
            "balanced"
        );
        assert_eq!(
            store
                .set_security_level("strict")
                .await
                .expect("set level")
                .level,
            "strict"
        );
        assert_eq!(
            store
                .get_security_level()
                .await
                .expect("updated level")
                .level,
            "strict"
        );
    }

    #[tokio::test]
    async fn invalid_security_level_is_validation_error() {
        let store = GovernanceState::new_with_client(FakeValkeyClient::default());

        let error = store
            .set_security_level("permissive")
            .await
            .expect_err("invalid level");
        assert!(matches!(error, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn rules_can_be_added_listed_and_deleted() {
        let client = FakeValkeyClient::default();
        let store = GovernanceState::new_with_client(client);

        store
            .add_rule("*.example.com", "allow")
            .await
            .expect("add rule");

        let rules = store.list_rules().await.expect("list rules");
        assert_eq!(rules.rules.len(), 1);
        assert_eq!(rules.rules[0].pattern, "*.example.com");
        assert_eq!(rules.rules[0].action, "allow");

        store
            .delete_rule("*.example.com")
            .await
            .expect("delete rule");
        assert!(
            store
                .list_rules()
                .await
                .expect("list rules")
                .rules
                .is_empty()
        );
    }

    #[tokio::test]
    async fn invalid_rule_action_is_validation_error() {
        let store = GovernanceState::new_with_client(FakeValkeyClient::default());

        let error = store
            .add_rule("*.example.com", "permit")
            .await
            .expect_err("invalid rule action");
        assert!(matches!(error, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn list_events_parses_structured_and_legacy_entries() {
        let client = FakeValkeyClient::default();
        client.seed_event(
            2.0,
            serde_json::to_string(&SecurityLogEntry {
                timestamp: Utc
                    .with_ymd_and_hms(2026, 3, 5, 19, 0, 0)
                    .single()
                    .expect("valid timestamp"),
                event_type: "block_reported".to_string(),
                request_id: Some("req-abc12345".to_string()),
                details: "Blocked request to example.com".to_string(),
            })
            .expect("serialize event"),
        );
        client.seed_event(
            3.0,
            serde_json::json!({
                "timestamp": 1_772_707_200_i64,
                "event_type": "approved_via_cli",
                "request_id": "req-def67890",
                "blocked_request": serde_json::to_string(&blocked_request("req-def67890", "https://legacy.example", 1)).expect("serialize blocked request")
            })
            .to_string(),
        );
        let store = GovernanceState::new_with_client(client);

        let events = store.list_events(10).await.expect("list events");

        assert_eq!(events.events.len(), 2);
        assert_eq!(events.events[0].event_type, "approved_via_cli");
        assert!(events.events[0].details.contains("legacy.example"));
        assert_eq!(events.events[1].event_type, "block_reported");
    }

    #[tokio::test]
    async fn get_status_aggregates_counts() {
        let client = FakeValkeyClient::default();
        store_with_blocked(
            &client,
            &blocked_request("req-abc12345", "https://a.example", 1),
        );
        client.seed_string(approved_key("req-def67890"), "approved");
        client.seed_event(
            1.0,
            serde_json::to_string(&SecurityLogEntry {
                timestamp: Utc::now(),
                event_type: "block_reported".to_string(),
                request_id: Some("req-abc12345".to_string()),
                details: "Blocked".to_string(),
            })
            .expect("serialize event"),
        );
        let store = GovernanceState::new_with_client(client);

        let status = store.get_status().await.expect("get status");

        assert_eq!(status.security_level, "balanced");
        assert_eq!(status.pending_count, 1);
        assert_eq!(status.recent_approvals, 1);
        assert_eq!(status.events_count, 1);
    }

    #[tokio::test]
    async fn dependency_errors_map_to_service_unavailable() {
        let client = FakeValkeyClient::default();
        client.fail("scan_keys");
        let store = GovernanceState::new_with_client(client);

        let error = store.list_blocked().await.expect_err("dependency failure");
        assert!(matches!(error, AppError::DependencyUnavailable(_)));
    }
}
