//! Governance state access for the control-plane server.
//!
//! Valkey key patterns (from `polis_common::redis_keys`):
//! - `polis:blocked:{request_id}` — JSON `BlockedRequest` values waiting for approval.
//! - `polis:approved:{request_id}` — temporary approved markers with a short TTL.
//! - `polis:config:security_level` — global security level string.
//! - `polis:config:auto_approve:{pattern}` — rule action string for a destination pattern.
//! - `polis:log:events` — sorted set of JSON security log entries.

#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cp_api_types::{
    ActionResponse, AgentResponse, BlockedItem, BlockedListResponse, BypassListResponse,
    ConfigAgentResponse, ConfigResponse, ContainersResponse, EventItem, EventsResponse,
    LevelResponse, LogsResponse, MetricsHistoryResponse, MetricsResponse, RuleCreateRequest,
    RuleItem, RulesResponse, SecurityConfigResponse, SecurityOverview, StatusResponse,
    WorkspaceResponse,
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
use sha2::{Digest, Sha256};

use crate::{
    auth::Role,
    config::Config,
    docker::{DockerClient, MetricsCollector},
    error::{AppError, AppResult},
};

const EVENT_LOG_MAX_ENTRIES: usize = 1_000;
const AUTH_FAILURE_LIMIT: usize = 10;
const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(60);
const AUTH_TOKEN_PREFIX: &str = "polis:auth:tokens:";
const RUNTIME_BYPASS_PREFIX: &str = "polis:config:bypass:";
const DEFAULT_AGENT_NAME: &str = "openclaw";
const DEFAULT_AGENT_VERSION: &str = "1.0.0";
const PROTECTED_PATHS: &[&str] = &[
    "~/.ssh",
    "~/.aws",
    "~/.gnupg",
    "~/.config/gcloud",
    "~/.kube",
    "~/.docker",
];
const COMPILED_BYPASS_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../sentinel/modules/dlp/srv_polis_dlp.c"
));

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

#[async_trait]
pub trait WorkspaceStore: Clone + Send + Sync + 'static {
    async fn get_workspace(&self) -> AppResult<WorkspaceResponse>;
    async fn get_agent(&self) -> AppResult<AgentResponse>;
    async fn list_containers(&self) -> AppResult<ContainersResponse>;
}

#[async_trait]
pub trait MetricsStore: Clone + Send + Sync + 'static {
    async fn get_metrics(&self) -> AppResult<MetricsResponse>;
    async fn get_metrics_history(&self, minutes: u32) -> AppResult<MetricsHistoryResponse>;
}

#[async_trait]
pub trait LogsStore: Clone + Send + Sync + 'static {
    async fn get_logs(
        &self,
        lines: usize,
        since_seconds: Option<i64>,
        level: Option<String>,
    ) -> AppResult<LogsResponse>;
    async fn get_logs_for_service(
        &self,
        service: &str,
        lines: usize,
        since_seconds: Option<i64>,
        level: Option<String>,
    ) -> AppResult<LogsResponse>;
}

#[async_trait]
pub trait RuntimeConfigStore: Clone + Send + Sync + 'static {
    /// Validate and normalize a bypass domain or suffix entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is empty, malformed, or violates the
    /// runtime bypass domain constraints.
    fn normalize_bypass_domain(&self, domain: &str) -> AppResult<String>;
    fn display_bypass_domain(&self, domain: &str) -> String;
    async fn get_config(&self) -> AppResult<ConfigResponse>;
    async fn get_security_config(&self) -> AppResult<SecurityConfigResponse>;
    async fn set_security_level_via_config(&self, level: &str) -> AppResult<ActionResponse>;
    async fn list_bypass_domains(&self) -> AppResult<BypassListResponse>;
    async fn add_bypass_domain(&self, domain: &str) -> AppResult<ActionResponse>;
    async fn delete_bypass_domain(&self, domain: &str) -> AppResult<ActionResponse>;
}

#[async_trait]
pub trait AuthStore: Clone + Send + Sync + 'static {
    fn auth_enabled(&self) -> bool;
    async fn validate_token(&self, token: &str) -> AppResult<Role>;
    async fn register_auth_failure(&self, client_id: &str, reason: &str) -> AppResult<bool>;
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

#[derive(Clone)]
pub struct AppState<C = FredValkeyClient> {
    governance: GovernanceState<C>,
    docker: Option<DockerClient>,
    metrics: MetricsCollector,
    auth: AuthState,
}

#[derive(Clone, Default)]
pub struct AuthState {
    enabled: bool,
    failures: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl AuthState {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            failures: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    /// Record a failed authentication attempt and report whether the caller is
    /// now rate-limited.
    pub fn record_failure(&self, client_id: &str) -> bool {
        let now = Instant::now();
        let mut failures = match self.failures.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let window = failures.entry(client_id.to_string()).or_default();
        window.retain(|timestamp| now.duration_since(*timestamp) <= AUTH_FAILURE_WINDOW);
        window.push(now);
        window.len() > AUTH_FAILURE_LIMIT
    }
}

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

impl AppState<FredValkeyClient> {
    /// Build the production app state from the process configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the Valkey client cannot be initialized.
    pub async fn new(config: &Config) -> Result<Self> {
        let governance = GovernanceState::new(config).await?;
        let docker = if config.docker_enabled {
            match DockerClient::new().await {
                Ok(client) => Some(client),
                Err(error) => {
                    tracing::warn!(%error, "docker integration unavailable");
                    None
                }
            }
        } else {
            None
        };

        let state = Self::new_with_auth(
            governance,
            docker,
            MetricsCollector::new(),
            AuthState::new(config.auth_enabled),
        );
        if config.auth_enabled {
            state.seed_auth_tokens(config).await?;
        }
        Ok(state)
    }

    async fn seed_auth_tokens(&self, config: &Config) -> Result<()> {
        let tokens = vec![
            (Role::Admin, config.read_secret(&config.admin_token_file)?),
            (
                Role::Operator,
                config.read_secret(&config.operator_token_file)?,
            ),
            (Role::Viewer, config.read_secret(&config.viewer_token_file)?),
            (Role::Agent, config.read_secret(&config.agent_token_file)?),
        ];
        self.governance
            .register_auth_tokens(&tokens)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
}

impl<C> AppState<C> {
    #[must_use]
    fn new_with_auth(
        governance: GovernanceState<C>,
        docker: Option<DockerClient>,
        metrics: MetricsCollector,
        auth: AuthState,
    ) -> Self {
        Self {
            governance,
            docker,
            metrics,
            auth,
        }
    }

    #[must_use]
    pub fn new_with_parts(
        governance: GovernanceState<C>,
        docker: Option<DockerClient>,
        metrics: MetricsCollector,
    ) -> Self {
        Self::new_with_auth(governance, docker, metrics, AuthState::default())
    }

    #[must_use]
    pub fn governance(&self) -> &GovernanceState<C> {
        &self.governance
    }

    #[must_use]
    pub fn docker(&self) -> Option<&DockerClient> {
        self.docker.as_ref()
    }

    #[must_use]
    pub fn metrics(&self) -> &MetricsCollector {
        &self.metrics
    }

    #[must_use]
    pub fn auth(&self) -> &AuthState {
        &self.auth
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

    async fn log_security_event(&self, event_type: &str, details: String) -> AppResult<()> {
        self.append_event(&SecurityLogEntry {
            timestamp: Utc::now(),
            event_type: event_type.to_string(),
            request_id: None,
            details,
        })
        .await
    }

    fn auth_token_key(hash: &str) -> String {
        format!("{AUTH_TOKEN_PREFIX}{hash}")
    }

    fn bypass_key(domain: &str) -> String {
        format!("{RUNTIME_BYPASS_PREFIX}{domain}")
    }

    fn normalize_bypass_domain(domain: &str) -> AppResult<String> {
        let trimmed = domain.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            return Err(AppError::Validation(
                "bypass domain must not be empty".to_string(),
            ));
        }
        if trimmed.len() > 253 {
            return Err(AppError::Validation(
                "bypass domain must be 253 characters or fewer".to_string(),
            ));
        }
        if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains(':') {
            return Err(AppError::Validation(
                "bypass domain must not contain path separators or ports".to_string(),
            ));
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err(AppError::Validation(
                "bypass domain must not contain whitespace".to_string(),
            ));
        }

        let normalized = if let Some(suffix) = trimmed.strip_prefix("*.") {
            format!(".{suffix}")
        } else {
            trimmed
        };

        let bare = normalized.strip_prefix('.').unwrap_or(&normalized);
        if bare.is_empty() || bare.starts_with('.') || bare.ends_with('.') || bare.contains("..") {
            return Err(AppError::Validation(
                "bypass domain must be a valid hostname or wildcard hostname".to_string(),
            ));
        }

        let valid = bare.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        });
        if !valid {
            return Err(AppError::Validation(
                "bypass domain must contain only letters, digits, hyphens, and dots".to_string(),
            ));
        }

        Ok(normalized)
    }

    fn display_bypass_domain(domain: &str) -> String {
        domain
            .strip_prefix('.')
            .map_or_else(|| domain.to_string(), |suffix| format!("*.{suffix}"))
    }

    fn compiled_bypass_domains() -> Vec<String> {
        let mut in_array = false;
        let mut domains = COMPILED_BYPASS_SOURCE
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("static const char *known_domains[]") {
                    in_array = true;
                    return None;
                }
                if !in_array {
                    return None;
                }
                if trimmed.starts_with("NULL") {
                    in_array = false;
                    return None;
                }

                let start = trimmed.find('"')?;
                let tail = &trimmed[start + 1..];
                let end = tail.find('"')?;
                let raw = &tail[..end];
                Some(Self::display_bypass_domain(raw))
            })
            .collect::<Vec<_>>();
        domains.sort();
        domains.dedup();
        domains
    }

    async fn list_runtime_bypass_domains(&self) -> AppResult<Vec<String>> {
        let mut domains = self
            .client
            .scan_keys(&format!("{RUNTIME_BYPASS_PREFIX}*"))
            .await
            .map_err(|error| Self::dependency_error("failed to scan bypass domains", &error))?
            .into_iter()
            .filter_map(|key| {
                key.strip_prefix(RUNTIME_BYPASS_PREFIX)
                    .map(ToString::to_string)
            })
            .map(|domain| Self::display_bypass_domain(&domain))
            .collect::<Vec<_>>();
        domains.sort();
        domains.dedup();
        Ok(domains)
    }

    async fn register_auth_tokens(&self, tokens: &[(Role, String)]) -> AppResult<()> {
        for (role, token) in tokens {
            let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
            self.client
                .set_string(&Self::auth_token_key(&hash), role.as_str())
                .await
                .map_err(|error| {
                    Self::dependency_error("failed to register control-plane auth token", &error)
                })?;
        }
        Ok(())
    }

    async fn validate_token(&self, token: &str) -> AppResult<Role> {
        let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let value = self
            .client
            .get_string(&Self::auth_token_key(&hash))
            .await
            .map_err(|error| Self::dependency_error("failed to validate auth token", &error))?
            .ok_or_else(|| AppError::Validation("invalid authentication token".to_string()))?;

        Role::parse(&value)
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("stored auth role is invalid")))
    }

    async fn get_security_config(&self) -> AppResult<SecurityConfigResponse> {
        Ok(SecurityConfigResponse {
            level: self.get_security_level().await?.level,
            auto_approve_rules: self.list_rules().await?.rules,
        })
    }

    async fn get_config(&self, agent: ConfigAgentResponse) -> AppResult<ConfigResponse> {
        let bypass_domains = self.list_bypass_domains().await?;
        Ok(ConfigResponse {
            security: SecurityOverview {
                level: self.get_security_level().await?.level,
                protected_paths: PROTECTED_PATHS.iter().map(ToString::to_string).collect(),
            },
            auto_approve_rules: self.list_rules().await?.rules,
            bypass_domains_count: bypass_domains.total,
            agent,
        })
    }

    async fn set_security_level_via_config(&self, level: &str) -> AppResult<ActionResponse> {
        let response = self.set_security_level(level).await?;
        self.log_security_event(
            "config_changed",
            format!("security level changed to {}", response.level),
        )
        .await?;
        Ok(ActionResponse {
            message: format!("security level set to {}", response.level),
        })
    }

    async fn list_bypass_domains(&self) -> AppResult<BypassListResponse> {
        let mut domains = Self::compiled_bypass_domains();
        let runtime_domains = self.list_runtime_bypass_domains().await?;
        domains.extend(runtime_domains.iter().cloned());
        domains.sort();
        domains.dedup();

        Ok(BypassListResponse {
            total: domains.len(),
            source: if runtime_domains.is_empty() {
                "compiled".to_string()
            } else {
                "combined".to_string()
            },
            domains,
        })
    }

    async fn add_bypass_domain(&self, domain: &str) -> AppResult<ActionResponse> {
        let normalized = Self::normalize_bypass_domain(domain)?;
        self.client
            .set_string(&Self::bypass_key(&normalized), "bypass")
            .await
            .map_err(|error| Self::dependency_error("failed to add bypass domain", &error))?;
        let display = Self::display_bypass_domain(&normalized);
        self.log_security_event("config_changed", format!("bypass domain added: {display}"))
            .await?;
        Ok(ActionResponse {
            message: format!("added bypass domain {display}"),
        })
    }

    async fn delete_bypass_domain(&self, domain: &str) -> AppResult<ActionResponse> {
        let normalized = Self::normalize_bypass_domain(domain)?;
        self.client
            .del(&Self::bypass_key(&normalized))
            .await
            .map_err(|error| Self::dependency_error("failed to remove bypass domain", &error))?;
        let display = Self::display_bypass_domain(&normalized);
        self.log_security_event(
            "config_changed",
            format!("bypass domain removed: {display}"),
        )
        .await?;
        Ok(ActionResponse {
            message: format!("removed bypass domain {display}"),
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

#[async_trait]
impl<C> GovernanceStore for AppState<C>
where
    C: ValkeyClient,
{
    async fn get_status(&self) -> AppResult<StatusResponse> {
        self.governance.get_status().await
    }

    async fn list_blocked(&self) -> AppResult<BlockedListResponse> {
        self.governance.list_blocked().await
    }

    async fn approve(&self, request_id: &str) -> AppResult<ActionResponse> {
        self.governance.approve(request_id).await
    }

    async fn deny(&self, request_id: &str) -> AppResult<ActionResponse> {
        self.governance.deny(request_id).await
    }

    async fn list_events(&self, limit: usize) -> AppResult<EventsResponse> {
        self.governance.list_events(limit).await
    }

    async fn get_security_level(&self) -> AppResult<LevelResponse> {
        self.governance.get_security_level().await
    }

    async fn set_security_level(&self, level: &str) -> AppResult<LevelResponse> {
        self.governance.set_security_level(level).await
    }

    async fn list_rules(&self) -> AppResult<RulesResponse> {
        self.governance.list_rules().await
    }

    async fn add_rule(&self, pattern: &str, action: &str) -> AppResult<ActionResponse> {
        self.governance.add_rule(pattern, action).await
    }

    async fn add_rule_from_request(
        &self,
        request: &RuleCreateRequest,
    ) -> AppResult<ActionResponse> {
        self.governance.add_rule_from_request(request).await
    }

    async fn delete_rule(&self, pattern: &str) -> AppResult<ActionResponse> {
        self.governance.delete_rule(pattern).await
    }
}

#[async_trait]
impl<C> WorkspaceStore for AppState<C>
where
    C: ValkeyClient,
{
    async fn get_workspace(&self) -> AppResult<WorkspaceResponse> {
        let docker = self.docker.as_ref().ok_or_else(|| {
            AppError::DependencyUnavailable("docker socket not accessible".to_string())
        })?;
        docker.workspace_status().await
    }

    async fn get_agent(&self) -> AppResult<AgentResponse> {
        let docker = self.docker.as_ref().ok_or_else(|| {
            AppError::DependencyUnavailable("docker socket not accessible".to_string())
        })?;
        docker.agent_info().await
    }

    async fn list_containers(&self) -> AppResult<ContainersResponse> {
        let docker = self.docker.as_ref().ok_or_else(|| {
            AppError::DependencyUnavailable("docker socket not accessible".to_string())
        })?;
        Ok(ContainersResponse {
            containers: docker.list_polis_containers().await?,
        })
    }
}

#[async_trait]
impl<C> MetricsStore for AppState<C>
where
    C: ValkeyClient,
{
    async fn get_metrics(&self) -> AppResult<MetricsResponse> {
        if let Some(docker) = self.docker.as_ref() {
            let snapshot = docker.metrics_snapshot().await?;
            self.metrics.update_snapshot(snapshot.clone()).await;
            return Ok(snapshot);
        }

        self.metrics.current_snapshot().await.ok_or_else(|| {
            AppError::DependencyUnavailable("docker socket not accessible".to_string())
        })
    }

    async fn get_metrics_history(&self, minutes: u32) -> AppResult<MetricsHistoryResponse> {
        Ok(self.metrics.history(Some(minutes)).await)
    }
}

#[async_trait]
impl<C> LogsStore for AppState<C>
where
    C: ValkeyClient,
{
    async fn get_logs(
        &self,
        lines: usize,
        since_seconds: Option<i64>,
        level: Option<String>,
    ) -> AppResult<LogsResponse> {
        let docker = self.docker.as_ref().ok_or_else(|| {
            AppError::DependencyUnavailable("docker socket not accessible".to_string())
        })?;
        docker
            .logs_snapshot(None, lines, since_seconds, level.as_deref())
            .await
    }

    async fn get_logs_for_service(
        &self,
        service: &str,
        lines: usize,
        since_seconds: Option<i64>,
        level: Option<String>,
    ) -> AppResult<LogsResponse> {
        let docker = self.docker.as_ref().ok_or_else(|| {
            AppError::DependencyUnavailable("docker socket not accessible".to_string())
        })?;
        docker
            .logs_snapshot(Some(service), lines, since_seconds, level.as_deref())
            .await
    }
}

#[async_trait]
impl<C> RuntimeConfigStore for AppState<C>
where
    C: ValkeyClient,
{
    fn normalize_bypass_domain(&self, domain: &str) -> AppResult<String> {
        GovernanceState::<C>::normalize_bypass_domain(domain)
    }

    fn display_bypass_domain(&self, domain: &str) -> String {
        GovernanceState::<C>::display_bypass_domain(domain)
    }

    async fn get_config(&self) -> AppResult<ConfigResponse> {
        let agent = if let Some(docker) = self.docker.as_ref() {
            match docker.agent_info().await {
                Ok(agent) => ConfigAgentResponse {
                    name: agent.name,
                    version: agent.version,
                },
                Err(_) => default_config_agent(),
            }
        } else {
            default_config_agent()
        };

        self.governance.get_config(agent).await
    }

    async fn get_security_config(&self) -> AppResult<SecurityConfigResponse> {
        self.governance.get_security_config().await
    }

    async fn set_security_level_via_config(&self, level: &str) -> AppResult<ActionResponse> {
        self.governance.set_security_level_via_config(level).await
    }

    async fn list_bypass_domains(&self) -> AppResult<BypassListResponse> {
        self.governance.list_bypass_domains().await
    }

    async fn add_bypass_domain(&self, domain: &str) -> AppResult<ActionResponse> {
        self.governance.add_bypass_domain(domain).await
    }

    async fn delete_bypass_domain(&self, domain: &str) -> AppResult<ActionResponse> {
        self.governance.delete_bypass_domain(domain).await
    }
}

#[async_trait]
impl<C> AuthStore for AppState<C>
where
    C: ValkeyClient,
{
    fn auth_enabled(&self) -> bool {
        self.auth.enabled()
    }

    async fn validate_token(&self, token: &str) -> AppResult<Role> {
        if !self.auth.enabled() {
            return Ok(Role::Admin);
        }
        self.governance.validate_token(token).await
    }

    async fn register_auth_failure(&self, client_id: &str, reason: &str) -> AppResult<bool> {
        let rate_limited = self.auth.record_failure(client_id);
        let details = if rate_limited {
            format!("rate-limited failed auth attempt from {client_id}: {reason}")
        } else {
            format!("failed auth attempt from {client_id}: {reason}")
        };
        self.governance
            .log_security_event("auth_failed", details)
            .await?;
        Ok(rate_limited)
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

fn default_config_agent() -> ConfigAgentResponse {
    ConfigAgentResponse {
        name: DEFAULT_AGENT_NAME.to_string(),
        version: DEFAULT_AGENT_VERSION.to_string(),
    }
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
    async fn validate_token_roundtrips_registered_roles() {
        let client = FakeValkeyClient::default();
        let store = GovernanceState::new_with_client(client);

        store
            .register_auth_tokens(&[(Role::Viewer, "polis_viewer_deadbeef".to_string())])
            .await
            .expect("register token");

        let role = store
            .validate_token("polis_viewer_deadbeef")
            .await
            .expect("validate token");
        assert_eq!(role, Role::Viewer);
    }

    #[tokio::test]
    async fn register_auth_failure_logs_and_rate_limits_after_threshold() {
        let client = FakeValkeyClient::default();
        let governance = GovernanceState::new_with_client(client.clone());
        let app_state = AppState::new_with_auth(
            governance,
            None,
            MetricsCollector::new(),
            AuthState::new(true),
        );

        for attempt in 1..=AUTH_FAILURE_LIMIT {
            let rate_limited = app_state
                .register_auth_failure("127.0.0.1", "invalid authentication token")
                .await
                .expect("register auth failure");
            assert!(!rate_limited, "unexpected rate limit on attempt {attempt}");
        }

        let rate_limited = app_state
            .register_auth_failure("127.0.0.1", "invalid authentication token")
            .await
            .expect("register auth failure");
        assert!(rate_limited);
        assert_eq!(
            client.zcard(keys::EVENT_LOG).await.expect("event count"),
            11
        );
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

    #[tokio::test]
    async fn app_state_delegates_phase1_reads_to_governance_state() {
        let client = FakeValkeyClient::default();
        store_with_blocked(
            &client,
            &blocked_request("req-abc12345", "https://a.example", 1),
        );
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

        let governance = GovernanceState::new_with_client(client);
        let app_state = AppState::new_with_parts(governance.clone(), None, MetricsCollector::new());

        assert_eq!(
            GovernanceStore::get_status(&app_state)
                .await
                .expect("status"),
            GovernanceStore::get_status(&governance)
                .await
                .expect("status"),
        );
        assert_eq!(
            GovernanceStore::list_blocked(&app_state)
                .await
                .expect("blocked list"),
            GovernanceStore::list_blocked(&governance)
                .await
                .expect("blocked list"),
        );
        assert_eq!(
            GovernanceStore::list_events(&app_state, 50)
                .await
                .expect("events"),
            GovernanceStore::list_events(&governance, 50)
                .await
                .expect("events"),
        );
    }

    #[tokio::test]
    async fn app_state_delegates_phase1_mutations_to_governance_state() {
        let client = FakeValkeyClient::default();
        let governance = GovernanceState::new_with_client(client);
        let app_state = AppState::new_with_parts(governance.clone(), None, MetricsCollector::new());

        let level = GovernanceStore::set_security_level(&app_state, "strict")
            .await
            .expect("level update");
        assert_eq!(level.level, "strict");

        let rule_response = GovernanceStore::add_rule(&app_state, "*.example.com", "allow")
            .await
            .expect("rule add");
        assert!(rule_response.message.contains("*.example.com"));

        assert_eq!(
            GovernanceStore::get_security_level(&governance)
                .await
                .expect("governance level")
                .level,
            "strict",
        );
        assert_eq!(
            GovernanceStore::list_rules(&governance)
                .await
                .expect("governance rules")
                .rules,
            vec![RuleItem {
                pattern: "*.example.com".to_string(),
                action: "allow".to_string(),
            }],
        );
        assert!(app_state.docker().is_none());
        assert!(app_state.metrics().current_snapshot().await.is_none());
    }
}
