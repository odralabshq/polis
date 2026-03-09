//! Polis control-plane server library.

#![cfg_attr(test, allow(clippy::expect_used))]

pub mod auth;
pub mod config;
pub mod config_api;
pub mod docker;
pub mod error;
pub mod observability_api;
pub mod state;
pub mod workspace_api;

use std::{convert::Infallible, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE},
    middleware,
    response::{
        Html,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use cp_api_types::{
    ActionResponse, BlockedListResponse, EventsResponse, LevelRequest, LevelResponse,
    RuleCreateRequest, RulesResponse, StatusResponse,
};
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio::{sync::broadcast, task::JoinHandle, time::MissedTickBehavior};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

use crate::{
    auth::Permission,
    config::Config,
    error::{AppError, AppResult},
    state::{
        AppState, AuthStore, GovernanceStore, LogsStore, MetricsStore, RuntimeConfigStore,
        WorkspaceStore,
    },
};

const INDEX_HTML: &str = include_str!("../web/index.html");
const DEFAULT_EVENT_LIMIT: usize = 50;
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(2);
const WORKSPACE_POLL_TICKS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BroadcastMessage {
    Status,
    Blocked,
    EventLog,
    Rules,
    Metrics,
    Workspace,
    Agent,
    Config(cp_api_types::ConfigEvent),
    Full,
}

#[derive(Clone)]
pub struct HttpState<S> {
    store: Arc<S>,
    broadcaster: broadcast::Sender<BroadcastMessage>,
}

impl<S> HttpState<S> {
    #[must_use]
    pub fn new(store: Arc<S>, broadcaster: broadcast::Sender<BroadcastMessage>) -> Self {
        Self { store, broadcaster }
    }

    fn notify(&self, message: BroadcastMessage) {
        let _ = self.broadcaster.send(message);
    }
}

/// Build the Phase 1 control-plane router.
pub fn build_router<S>(state: HttpState<S>) -> Router
where
    S: GovernanceStore + WorkspaceStore + MetricsStore + LogsStore + RuntimeConfigStore + AuthStore,
{
    let api: Router<HttpState<S>> = Router::new()
        .route(
            "/status",
            get(status::<S>).route_layer(middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/workspace",
            get(workspace_api::workspace::<S>).route_layer(middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/agent",
            get(workspace_api::agent::<S>).route_layer(middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/containers",
            get(workspace_api::containers::<S>).route_layer(middleware::from_fn(
                |request, next| auth::require_permission(request, next, Permission::ReadDashboard),
            )),
        )
        .route(
            "/blocked",
            get(blocked::<S>).route_layer(middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadBlocked)
            })),
        )
        .route(
            "/blocked/{id}/approve",
            post(approve::<S>).route_layer(middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::MutateGovernance)
            })),
        )
        .route(
            "/blocked/{id}/deny",
            post(deny::<S>).route_layer(middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::MutateGovernance)
            })),
        )
        .route(
            "/events",
            get(events::<S>).route_layer(middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/config/level",
            get(get_security_level::<S>)
                .route_layer(middleware::from_fn(|request, next| {
                    auth::require_permission(request, next, Permission::ReadLevel)
                }))
                .put(set_security_level::<S>)
                .route_layer(middleware::from_fn(|request, next| {
                    auth::require_permission(request, next, Permission::MutateConfig)
                })),
        )
        .route(
            "/config/rules",
            get(list_rules::<S>)
                .route_layer(middleware::from_fn(|request, next| {
                    auth::require_permission(request, next, Permission::ReadDashboard)
                }))
                .post(add_rule::<S>)
                .delete(delete_rule::<S>)
                .route_layer(middleware::from_fn(|request, next| {
                    auth::require_permission(request, next, Permission::MutateConfig)
                })),
        )
        .merge(config_api::routes::<S>())
        .merge(observability_api::routes::<S>())
        .route(
            "/stream",
            get(stream_events::<S>).route_layer(middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware::<S>,
        ))
        .with_state(state.clone());

    Router::<HttpState<S>>::new()
        .route("/", get(index))
        .route("/health", get(health))
        .nest("/api/v1", api)
        .layer(build_cors(state.store.auth_enabled()))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Run the production control-plane server.
///
/// # Errors
///
/// Returns an error if config loading, Valkey initialization, or serving fails.
pub async fn run() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init();

    let config = Config::from_env()?;
    let state = Arc::new(AppState::new(&config).await?);
    let (sender, _) = broadcast::channel(64);
    let http_state = HttpState::new(state, sender);
    let _poller = spawn_poller(http_state.clone());
    let router = build_router(http_state);

    tracing::info!(listen_addr = %config.listen_addr, "starting cp-server");
    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.listen_addr))?;

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("control-plane HTTP server failed")?;

    Ok(())
}

/// Probe the local HTTP health endpoint and exit successfully only on `200 OK`.
///
/// # Errors
///
/// Returns an error if the health endpoint cannot be reached or returns a non-200 response.
pub async fn run_healthcheck() -> Result<()> {
    let config = Config::from_env()?;
    let port = config
        .listen_addr
        .rsplit_once(':')
        .map(|(_, port)| port)
        .filter(|port| !port.is_empty())
        .context("control-plane listen address must include a port")?;
    let target = format!("127.0.0.1:{port}");

    let response = tokio::time::timeout(HEALTHCHECK_TIMEOUT, async {
        let mut stream = TcpStream::connect(&target)
            .await
            .with_context(|| format!("failed to connect to {target}"))?;
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .context("failed to send healthcheck request")?;

        let mut buffer = [0_u8; 128];
        let read = stream
            .read(&mut buffer)
            .await
            .context("failed to read healthcheck response")?;
        Ok::<String, anyhow::Error>(String::from_utf8_lossy(&buffer[..read]).into_owned())
    })
    .await
    .context("control-plane healthcheck timed out")??;

    let status_line = response.lines().next().unwrap_or_default();
    if status_line.starts_with("HTTP/1.1 200") || status_line.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        bail!("unexpected control-plane healthcheck response: {status_line}");
    }
}

#[must_use]
pub fn spawn_poller<S>(state: HttpState<S>) -> JoinHandle<()>
where
    S: GovernanceStore + WorkspaceStore + MetricsStore,
{
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(POLL_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut last_status: Option<StatusResponse> = None;
        let mut last_blocked: Option<BlockedListResponse> = None;
        let mut last_events: Option<EventsResponse> = None;
        let mut last_rules: Option<RulesResponse> = None;
        let mut last_workspace = None;
        let mut last_agent = None;
        let mut last_metrics = None;
        let mut ticks = 0_u64;

        loop {
            interval.tick().await;
            ticks = ticks.wrapping_add(1);

            match state.store.get_status().await {
                Ok(status) => {
                    if last_status.as_ref() != Some(&status) {
                        last_status = Some(status);
                        state.notify(BroadcastMessage::Status);
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to poll status snapshot"),
            }

            match state.store.list_blocked().await {
                Ok(blocked) => {
                    if last_blocked.as_ref() != Some(&blocked) {
                        last_blocked = Some(blocked);
                        state.notify(BroadcastMessage::Blocked);
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to poll blocked snapshot"),
            }

            match state.store.list_events(DEFAULT_EVENT_LIMIT).await {
                Ok(events) => {
                    if last_events.as_ref() != Some(&events) {
                        last_events = Some(events);
                        state.notify(BroadcastMessage::EventLog);
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to poll event snapshot"),
            }

            match state.store.list_rules().await {
                Ok(rules) => {
                    if last_rules.as_ref() != Some(&rules) {
                        last_rules = Some(rules);
                        state.notify(BroadcastMessage::Rules);
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to poll rules snapshot"),
            }

            if !ticks.is_multiple_of(WORKSPACE_POLL_TICKS) {
                continue;
            }

            match state.store.get_workspace().await {
                Ok(workspace) => {
                    if last_workspace.as_ref() != Some(&workspace) {
                        last_workspace = Some(workspace);
                        state.notify(BroadcastMessage::Workspace);
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to poll workspace snapshot"),
            }

            match state.store.get_agent().await {
                Ok(agent) => {
                    if last_agent.as_ref() != Some(&agent) {
                        last_agent = Some(agent);
                        state.notify(BroadcastMessage::Agent);
                    }
                }
                Err(AppError::NotFound(error)) => {
                    tracing::debug!(%error, "no active agent detected during polling");
                }
                Err(error) => tracing::warn!(%error, "failed to poll agent snapshot"),
            }

            if !ticks.is_multiple_of(docker::METRICS_INTERVAL_SECONDS) {
                continue;
            }

            match state.store.get_metrics().await {
                Ok(metrics) => {
                    if last_metrics.as_ref() != Some(&metrics) {
                        last_metrics = Some(metrics);
                        state.notify(BroadcastMessage::Metrics);
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to poll metrics snapshot"),
            }
        }
    })
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn status<S>(State(state): State<HttpState<S>>) -> AppResult<Json<StatusResponse>>
where
    S: GovernanceStore,
{
    Ok(Json(state.store.get_status().await?))
}

async fn blocked<S>(State(state): State<HttpState<S>>) -> AppResult<Json<BlockedListResponse>>
where
    S: GovernanceStore,
{
    Ok(Json(state.store.list_blocked().await?))
}

async fn approve<S>(
    State(state): State<HttpState<S>>,
    Path(id): Path<String>,
) -> AppResult<Json<ActionResponse>>
where
    S: GovernanceStore,
{
    let response = state.store.approve(&id).await?;
    state.notify(BroadcastMessage::Full);
    Ok(Json(response))
}

async fn deny<S>(
    State(state): State<HttpState<S>>,
    Path(id): Path<String>,
) -> AppResult<Json<ActionResponse>>
where
    S: GovernanceStore,
{
    let response = state.store.deny(&id).await?;
    state.notify(BroadcastMessage::Full);
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    #[serde(default = "default_event_limit")]
    limit: usize,
}

fn default_event_limit() -> usize {
    DEFAULT_EVENT_LIMIT
}

async fn events<S>(
    State(state): State<HttpState<S>>,
    Query(query): Query<EventsQuery>,
) -> AppResult<Json<EventsResponse>>
where
    S: GovernanceStore,
{
    Ok(Json(state.store.list_events(query.limit).await?))
}

async fn get_security_level<S>(State(state): State<HttpState<S>>) -> AppResult<Json<LevelResponse>>
where
    S: GovernanceStore,
{
    Ok(Json(state.store.get_security_level().await?))
}

async fn set_security_level<S>(
    State(state): State<HttpState<S>>,
    Json(request): Json<LevelRequest>,
) -> AppResult<Json<LevelResponse>>
where
    S: GovernanceStore,
{
    let response = state.store.set_security_level(&request.level).await?;
    state.notify(BroadcastMessage::Full);
    state.notify(BroadcastMessage::Config(cp_api_types::ConfigEvent {
        event_type: "level_changed".to_string(),
        level: Some(request.level.to_ascii_lowercase()),
        domain: None,
    }));
    Ok(Json(response))
}

async fn list_rules<S>(State(state): State<HttpState<S>>) -> AppResult<Json<RulesResponse>>
where
    S: GovernanceStore,
{
    Ok(Json(state.store.list_rules().await?))
}

async fn add_rule<S>(
    State(state): State<HttpState<S>>,
    Json(request): Json<RuleCreateRequest>,
) -> AppResult<Json<ActionResponse>>
where
    S: GovernanceStore,
{
    let response = state.store.add_rule_from_request(&request).await?;
    state.notify(BroadcastMessage::Full);
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct DeleteRuleQuery {
    pattern: String,
}

async fn delete_rule<S>(
    State(state): State<HttpState<S>>,
    Query(query): Query<DeleteRuleQuery>,
) -> AppResult<Json<ActionResponse>>
where
    S: GovernanceStore,
{
    let response = state.store.delete_rule(&query.pattern).await?;
    state.notify(BroadcastMessage::Full);
    Ok(Json(response))
}

async fn stream_events<S>(
    State(state): State<HttpState<S>>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>>
where
    S: GovernanceStore + WorkspaceStore + MetricsStore,
{
    let mut receiver = state.broadcaster.subscribe();
    let stream = stream! {
        for event in snapshot_events(state.store.as_ref(), BroadcastMessage::Full).await {
            yield Ok(event);
        }

        loop {
            let message = match receiver.recv().await {
                Ok(message) => message,
                Err(broadcast::error::RecvError::Lagged(_)) => BroadcastMessage::Full,
                Err(broadcast::error::RecvError::Closed) => break,
            };

            for event in snapshot_events(state.store.as_ref(), message).await {
                yield Ok(event);
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn snapshot_events<S>(store: &S, message: BroadcastMessage) -> Vec<Event>
where
    S: GovernanceStore + WorkspaceStore + MetricsStore,
{
    let mut events = Vec::new();

    if matches!(&message, BroadcastMessage::Status | BroadcastMessage::Full)
        && let Ok(status) = store.get_status().await
        && let Some(event) = serialize_sse_event("status", &status)
    {
        events.push(event);
    }

    if matches!(&message, BroadcastMessage::Blocked | BroadcastMessage::Full)
        && let Ok(blocked) = store.list_blocked().await
        && let Some(event) = serialize_sse_event("blocked", &blocked)
    {
        events.push(event);
    }

    if matches!(
        &message,
        BroadcastMessage::EventLog | BroadcastMessage::Full
    ) && let Ok(events_response) = store.list_events(DEFAULT_EVENT_LIMIT).await
        && let Some(event) = serialize_sse_event("event_log", &events_response)
    {
        events.push(event);
    }

    if matches!(&message, BroadcastMessage::Rules | BroadcastMessage::Full)
        && let Ok(rules) = store.list_rules().await
        && let Some(event) = serialize_sse_event("rules", &rules)
    {
        events.push(event);
    }

    if matches!(
        &message,
        BroadcastMessage::Workspace | BroadcastMessage::Full
    ) && let Ok(workspace) = store.get_workspace().await
        && let Some(event) = serialize_sse_event("workspace", &workspace)
    {
        events.push(event);
    }

    if matches!(&message, BroadcastMessage::Agent | BroadcastMessage::Full)
        && let Ok(agent) = store.get_agent().await
        && let Some(event) = serialize_sse_event("agent", &agent)
    {
        events.push(event);
    }

    if matches!(&message, BroadcastMessage::Metrics | BroadcastMessage::Full)
        && let Ok(metrics) = store.get_metrics().await
        && let Some(event) = serialize_sse_event("metrics", &metrics)
    {
        events.push(event);
    }

    if let BroadcastMessage::Config(config_event) = message
        && let Some(event) = serialize_sse_event("config", &config_event)
    {
        events.push(event);
    }

    events
}

fn serialize_sse_event<T>(name: &str, payload: &T) -> Option<Event>
where
    T: serde::Serialize,
{
    serialize_sse_message(name, payload)
        .map(|(event, json)| Event::default().event(event).data(json))
}

fn serialize_sse_message<T>(name: &str, payload: &T) -> Option<(String, String)>
where
    T: serde::Serialize,
{
    match serde_json::to_string(payload) {
        Ok(json) => Some((name.to_string(), json)),
        Err(error) => {
            tracing::warn!(%error, event = name, "failed to serialize SSE payload");
            None
        }
    }
}

fn build_cors(auth_enabled: bool) -> CorsLayer {
    let allow_headers = if auth_enabled {
        vec![CONTENT_TYPE, axum::http::header::AUTHORIZATION]
    } else {
        vec![CONTENT_TYPE]
    };

    CorsLayer::new()
        .allow_origin([
            HeaderValue::from_static("http://localhost:9080"),
            HeaderValue::from_static("http://127.0.0.1:9080"),
        ])
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(allow_headers)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};
    use cp_api_types::{
        AgentResponse, ConfigEvent, ContainerMetrics, MetricsResponse, PortMapping, ResourceUsage,
        SystemMetrics, WorkspaceResponse,
    };

    use super::serialize_sse_message;

    #[test]
    fn serializes_metrics_sse_payload() {
        let payload = MetricsResponse {
            timestamp: Utc
                .with_ymd_and_hms(2026, 3, 6, 8, 0, 0)
                .single()
                .expect("valid timestamp"),
            system: SystemMetrics {
                total_memory_usage_mb: 1_024,
                total_memory_limit_mb: 8_192,
                total_cpu_percent: 12.5,
                container_count: 8,
            },
            containers: vec![ContainerMetrics {
                service: "workspace".to_string(),
                status: "running".to_string(),
                health: "healthy".to_string(),
                memory_usage_mb: 512,
                memory_limit_mb: 4_096,
                cpu_percent: 8.1,
                network_rx_bytes: 1,
                network_tx_bytes: 2,
                pids: 3,
                stale: false,
            }],
        };

        let (event, json) = serialize_sse_message("metrics", &payload).expect("serialize");
        assert_eq!(event, "metrics");
        assert!(json.contains("\"total_memory_usage_mb\":1024"));
    }

    #[test]
    fn serializes_workspace_and_agent_sse_payloads() {
        let workspace = WorkspaceResponse {
            status: "running".to_string(),
            uptime_seconds: Some(3_600),
            containers: cp_api_types::ContainerSummary {
                total: 11,
                healthy: 10,
                unhealthy: 1,
                starting: 0,
            },
            networks: HashMap::from([("gateway_bridge".to_string(), "10.30.1.0/24".to_string())]),
        };
        let agent = AgentResponse {
            name: "openclaw".to_string(),
            display_name: "OpenClaw".to_string(),
            version: "1.0.0".to_string(),
            status: "running".to_string(),
            health: "healthy".to_string(),
            uptime_seconds: Some(3_500),
            ports: vec![PortMapping {
                container: 18_789,
                host: 18_789,
                protocol: "tcp".to_string(),
            }],
            resources: ResourceUsage {
                memory_usage_mb: 512,
                memory_limit_mb: 4_096,
                cpu_percent: 8.1,
            },
            stale: false,
        };

        let (workspace_event, workspace_json) =
            serialize_sse_message("workspace", &workspace).expect("workspace serialize");
        let (agent_event, agent_json) =
            serialize_sse_message("agent", &agent).expect("agent serialize");

        assert_eq!(workspace_event, "workspace");
        assert!(workspace_json.contains("\"status\":\"running\""));
        assert_eq!(agent_event, "agent");
        assert!(agent_json.contains("\"display_name\":\"OpenClaw\""));
    }

    #[test]
    fn serializes_config_sse_payload() {
        let payload = ConfigEvent {
            event_type: "level_changed".to_string(),
            level: Some("strict".to_string()),
            domain: None,
        };

        let (event, json) = serialize_sse_message("config", &payload).expect("serialize");
        assert_eq!(event, "config");
        assert!(json.contains("\"type\":\"level_changed\""));
        assert!(json.contains("\"level\":\"strict\""));
    }
}
