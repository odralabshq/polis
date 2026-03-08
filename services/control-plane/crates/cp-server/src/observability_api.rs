use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use cp_api_types::{LogsResponse, MetricsHistoryResponse, MetricsResponse};
use serde::Deserialize;

use crate::{
    HttpState,
    auth::{self, Permission},
    error::{AppError, AppResult},
    state::{LogsStore, MetricsStore},
};

const DEFAULT_LOG_LINES: usize = 100;
const MAX_LOG_LINES: usize = 1_000;
const DEFAULT_HISTORY_MINUTES: u32 = 30;
const MAX_HISTORY_MINUTES: u32 = 60;
const VALID_LOG_LEVELS: &[&str] = &["info", "warn", "error"];
const VALID_LOG_SERVICES: &[&str] = &[
    "gate",
    "sentinel",
    "scanner",
    "resolver",
    "state",
    "toolbox",
    "workspace",
    "control-plane",
];

#[derive(Debug, Clone, Deserialize)]
pub struct LogsQuery {
    #[serde(default = "default_log_lines")]
    pub lines: usize,
    pub since: Option<i64>,
    pub level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetricsHistoryQuery {
    #[serde(default = "default_history_minutes")]
    pub minutes: u32,
}

/// Return recent control-plane and workspace logs.
///
/// # Errors
///
/// Returns an error when query validation fails or log retrieval fails.
pub async fn logs<S>(
    State(state): State<HttpState<S>>,
    Query(query): Query<LogsQuery>,
) -> AppResult<Json<LogsResponse>>
where
    S: LogsStore,
{
    let params = validate_logs_query(query)?;
    Ok(Json(
        state
            .store
            .get_logs(params.lines, params.since, params.level)
            .await?,
    ))
}

/// Return recent logs for a single service.
///
/// # Errors
///
/// Returns an error when the service or query is invalid, or log retrieval fails.
pub async fn logs_by_service<S>(
    State(state): State<HttpState<S>>,
    Path(service): Path<String>,
    Query(query): Query<LogsQuery>,
) -> AppResult<Json<LogsResponse>>
where
    S: LogsStore,
{
    validate_service_name(&service)?;
    let params = validate_logs_query(query)?;
    Ok(Json(
        state
            .store
            .get_logs_for_service(&service, params.lines, params.since, params.level)
            .await?,
    ))
}

/// Return the latest metrics snapshot.
///
/// # Errors
///
/// Returns an error when metrics retrieval fails.
pub async fn metrics<S>(State(state): State<HttpState<S>>) -> AppResult<Json<MetricsResponse>>
where
    S: MetricsStore,
{
    Ok(Json(state.store.get_metrics().await?))
}

/// Return recent metrics history for the requested time window.
///
/// # Errors
///
/// Returns an error when query validation fails or metrics retrieval fails.
pub async fn metrics_history<S>(
    State(state): State<HttpState<S>>,
    Query(query): Query<MetricsHistoryQuery>,
) -> AppResult<Json<MetricsHistoryResponse>>
where
    S: MetricsStore,
{
    let minutes = validate_history_minutes(query.minutes)?;
    Ok(Json(state.store.get_metrics_history(minutes).await?))
}

pub fn routes<S>() -> Router<HttpState<S>>
where
    S: LogsStore + MetricsStore + Clone + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/logs",
            get(logs::<S>).route_layer(axum::middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/logs/{service}",
            get(logs_by_service::<S>).route_layer(axum::middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/metrics",
            get(metrics::<S>).route_layer(axum::middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/metrics/history",
            get(metrics_history::<S>).route_layer(axum::middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
}

fn default_log_lines() -> usize {
    DEFAULT_LOG_LINES
}

fn default_history_minutes() -> u32 {
    DEFAULT_HISTORY_MINUTES
}

#[derive(Debug, Clone)]
struct ValidatedLogsQuery {
    lines: usize,
    since: Option<i64>,
    level: Option<String>,
}

fn validate_logs_query(query: LogsQuery) -> AppResult<ValidatedLogsQuery> {
    if !(1..=MAX_LOG_LINES).contains(&query.lines) {
        return Err(AppError::Validation(format!(
            "lines must be between 1 and {MAX_LOG_LINES}"
        )));
    }

    if let Some(since) = query.since
        && since < 0
    {
        return Err(AppError::Validation(
            "since must be a non-negative number of seconds".to_string(),
        ));
    }

    let level = query.level.map(|level| level.to_ascii_lowercase());
    if let Some(level) = level.as_deref()
        && !VALID_LOG_LEVELS.contains(&level)
    {
        return Err(AppError::Validation(
            "level must be one of: info, warn, error".to_string(),
        ));
    }

    Ok(ValidatedLogsQuery {
        lines: query.lines,
        since: query.since,
        level,
    })
}

fn validate_history_minutes(minutes: u32) -> AppResult<u32> {
    if !(1..=MAX_HISTORY_MINUTES).contains(&minutes) {
        return Err(AppError::Validation(format!(
            "minutes must be between 1 and {MAX_HISTORY_MINUTES}"
        )));
    }

    Ok(minutes)
}

fn validate_service_name(service: &str) -> AppResult<()> {
    if VALID_LOG_SERVICES.contains(&service) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "service must be one of: {}",
            VALID_LOG_SERVICES.join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use chrono::Utc;
    use cp_api_types::{ContainerMetrics, LogLine, MetricsPoint, SystemMetrics};
    use tokio::sync::broadcast;
    use tower::ServiceExt;

    use super::*;

    #[derive(Clone)]
    struct TestStore;

    #[async_trait]
    impl LogsStore for TestStore {
        async fn get_logs(
            &self,
            lines: usize,
            _since_seconds: Option<i64>,
            level: Option<String>,
        ) -> AppResult<LogsResponse> {
            Ok(sample_logs_response(lines, level.as_deref()))
        }

        async fn get_logs_for_service(
            &self,
            service: &str,
            lines: usize,
            _since_seconds: Option<i64>,
            level: Option<String>,
        ) -> AppResult<LogsResponse> {
            let mut response = sample_logs_response(lines, level.as_deref());
            response.lines.retain(|line| line.service == service);
            response.total = response.lines.len();
            Ok(response)
        }
    }

    #[async_trait]
    impl MetricsStore for TestStore {
        async fn get_metrics(&self) -> AppResult<MetricsResponse> {
            Ok(sample_metrics_response())
        }

        async fn get_metrics_history(&self, minutes: u32) -> AppResult<MetricsHistoryResponse> {
            Ok(MetricsHistoryResponse {
                interval_seconds: 10,
                points: vec![MetricsPoint {
                    timestamp: Utc::now(),
                    total_memory_usage_mb: u64::from(minutes),
                    total_cpu_percent: 5.0,
                }],
            })
        }
    }

    #[tokio::test]
    async fn logs_handler_returns_snapshot() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .uri("/logs")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn logs_handler_rejects_invalid_level() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .uri("/logs?level=debug")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn logs_by_service_rejects_invalid_service() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .uri("/logs/unknown")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn metrics_handlers_return_snapshots() {
        let router = test_router();

        for uri in ["/metrics", "/metrics/history", "/metrics/history?minutes=5"] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");

            assert_eq!(response.status(), axum::http::StatusCode::OK, "{uri}");
        }
    }

    #[test]
    fn validates_history_minutes_bounds() {
        assert!(validate_history_minutes(0).is_err());
        assert!(validate_history_minutes(61).is_err());
        assert_eq!(validate_history_minutes(30).expect("valid"), 30);
    }

    fn test_router() -> Router {
        let (sender, _) = broadcast::channel(4);
        let state = HttpState::new(Arc::new(TestStore), sender);
        routes::<TestStore>().with_state(state)
    }

    fn sample_logs_response(limit: usize, level: Option<&str>) -> LogsResponse {
        let mut lines = vec![
            LogLine {
                timestamp: Utc::now(),
                service: "workspace".to_string(),
                level: "info".to_string(),
                message: "workspace booted".to_string(),
            },
            LogLine {
                timestamp: Utc::now(),
                service: "control-plane".to_string(),
                level: "warn".to_string(),
                message: "retrying docker socket".to_string(),
            },
        ];

        if let Some(level) = level {
            lines.retain(|line| line.level == level);
        }
        if lines.len() > limit {
            lines.truncate(limit);
        }

        LogsResponse {
            total: lines.len(),
            truncated: false,
            lines,
        }
    }

    fn sample_metrics_response() -> MetricsResponse {
        MetricsResponse {
            timestamp: Utc::now(),
            system: SystemMetrics {
                total_memory_usage_mb: 768,
                total_memory_limit_mb: 4_096,
                total_cpu_percent: 12.5,
                container_count: 3,
            },
            containers: vec![ContainerMetrics {
                service: "workspace".to_string(),
                status: "running".to_string(),
                health: "healthy".to_string(),
                memory_usage_mb: 512,
                memory_limit_mb: 4_096,
                cpu_percent: 8.1,
                network_rx_bytes: 1_024,
                network_tx_bytes: 2_048,
                pids: 42,
                stale: false,
            }],
        }
    }
}
