#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use chrono::{TimeZone, Utc};
use cp_api_types::{
    ActionResponse, AgentResponse, BlockedItem, BlockedListResponse, BypassListResponse,
    ConfigAgentResponse, ConfigResponse, ContainerInfo, ContainerSummary, ContainersResponse,
    CredentialAllowItem, CredentialAllowsResponse, EventItem, EventsResponse, LevelResponse,
    LogLine, LogsResponse, MetricsHistoryResponse, MetricsPoint, MetricsResponse, ResourceUsage,
    RuleCreateRequest, RuleItem, RulesResponse, SecurityConfigResponse, StatusResponse,
    SystemMetrics, WorkspaceResponse,
};
use cp_server::{
    HttpState,
    auth::Role,
    build_router,
    error::{AppError, AppResult},
    state::{
        AuthStore, GovernanceStore, LogsStore, MetricsStore, RuntimeConfigStore, WorkspaceStore,
    },
};
use futures::StreamExt;
use tower::ServiceExt;

#[derive(Clone)]
struct TestStore {
    inner: Arc<Mutex<Fixture>>,
}

#[derive(Clone)]
struct Fixture {
    level: String,
    blocked: Vec<BlockedItem>,
    rules: Vec<RuleItem>,
    credential_allows: Vec<CredentialAllowItem>,
    events: Vec<EventItem>,
    bypass_domains: Vec<String>,
    fail_status: bool,
    approvals: usize,
    auth_enabled: bool,
    auth_failures: usize,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            level: "balanced".to_string(),
            blocked: vec![BlockedItem {
                request_id: "req-abc12345".to_string(),
                reason: "credential_detected".to_string(),
                destination: "example.com".to_string(),
                pattern: Some("aws_access".to_string()),
                fingerprint: Some("0123456789abcdef".to_string()),
                blocked_at: Utc
                    .with_ymd_and_hms(2026, 3, 5, 19, 0, 0)
                    .single()
                    .expect("valid timestamp"),
                status: "pending".to_string(),
            }],
            rules: vec![RuleItem {
                pattern: "*.example.com/path".to_string(),
                action: "allow".to_string(),
            }],
            credential_allows: vec![CredentialAllowItem {
                pattern: "aws_access".to_string(),
                host: "example.com".to_string(),
                fingerprint: "0123456789abcdef".to_string(),
            }],
            events: vec![EventItem {
                timestamp: Utc
                    .with_ymd_and_hms(2026, 3, 5, 19, 0, 0)
                    .single()
                    .expect("valid timestamp"),
                event_type: "block_reported".to_string(),
                request_id: Some("req-abc12345".to_string()),
                details: "Blocked request to example.com".to_string(),
            }],
            bypass_domains: vec!["internal.corp.com".to_string()],
            fail_status: false,
            approvals: 0,
            auth_enabled: false,
            auth_failures: 0,
        }
    }
}

impl TestStore {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Fixture::default())),
        }
    }

    fn set_fail_status(&self, fail: bool) {
        self.inner.lock().expect("fixture lock").fail_status = fail;
    }

    fn set_auth_enabled(&self, enabled: bool) {
        self.inner.lock().expect("fixture lock").auth_enabled = enabled;
    }

    fn router(&self) -> axum::Router {
        let (sender, _) = tokio::sync::broadcast::channel(32);
        build_router(HttpState::new(Arc::new(self.clone()), sender))
    }
}

#[async_trait]
impl GovernanceStore for TestStore {
    async fn get_status(&self) -> AppResult<StatusResponse> {
        let fixture = self.inner.lock().expect("fixture lock").clone();
        if fixture.fail_status {
            return Err(AppError::DependencyUnavailable(
                "valkey temporarily unreachable".to_string(),
            ));
        }

        Ok(StatusResponse {
            security_level: fixture.level,
            pending_count: fixture.blocked.len(),
            recent_approvals: fixture.approvals,
            events_count: fixture.events.len(),
        })
    }

    async fn list_blocked(&self) -> AppResult<BlockedListResponse> {
        Ok(BlockedListResponse {
            items: self.inner.lock().expect("fixture lock").blocked.clone(),
        })
    }

    async fn approve(&self, request_id: &str) -> AppResult<ActionResponse> {
        let mut fixture = self.inner.lock().expect("fixture lock");
        let item = fixture
            .blocked
            .iter()
            .find(|item| item.request_id == request_id)
            .cloned()
            .ok_or_else(|| {
                AppError::NotFound(format!("no blocked request found for {request_id}"))
            })?;
        fixture
            .blocked
            .retain(|blocked| blocked.request_id != request_id);
        fixture.approvals += 1;
        fixture.events.insert(
            0,
            EventItem {
                timestamp: Utc::now(),
                event_type: if item.reason == "credential_detected" {
                    "credential_approved_temp".to_string()
                } else {
                    "approved_via_control_plane".to_string()
                },
                request_id: Some(request_id.to_string()),
                details: "Approved request".to_string(),
            },
        );
        Ok(ActionResponse {
            message: format!("approved {request_id}"),
        })
    }

    async fn allow_credential(&self, request_id: &str) -> AppResult<ActionResponse> {
        let mut fixture = self.inner.lock().expect("fixture lock");
        let item = fixture
            .blocked
            .iter()
            .find(|item| item.request_id == request_id)
            .cloned()
            .ok_or_else(|| {
                AppError::NotFound(format!("no blocked request found for {request_id}"))
            })?;
        fixture.credential_allows.push(CredentialAllowItem {
            pattern: item.pattern.unwrap_or_default(),
            host: item.destination.clone(),
            fingerprint: item.fingerprint.unwrap_or_default(),
        });
        fixture.events.insert(
            0,
            EventItem {
                timestamp: Utc::now(),
                event_type: "credential_allowed_permanent".to_string(),
                request_id: Some(request_id.to_string()),
                details: "Remembered credential allow".to_string(),
            },
        );
        fixture
            .blocked
            .retain(|blocked| blocked.request_id != request_id);
        Ok(ActionResponse {
            message: format!("remembered credential allow for request {request_id}"),
        })
    }

    async fn bypass_blocked_domain(&self, request_id: &str) -> AppResult<ActionResponse> {
        let mut fixture = self.inner.lock().expect("fixture lock");
        let item = fixture
            .blocked
            .iter()
            .find(|item| item.request_id == request_id)
            .cloned()
            .ok_or_else(|| {
                AppError::NotFound(format!("no blocked request found for {request_id}"))
            })?;
        fixture.bypass_domains.push(item.destination.clone());
        fixture.events.insert(
            0,
            EventItem {
                timestamp: Utc::now(),
                event_type: "bypass_domain_added".to_string(),
                request_id: Some(request_id.to_string()),
                details: format!("Bypassed domain {}", item.destination),
            },
        );
        fixture
            .blocked
            .retain(|blocked| blocked.request_id != request_id);
        Ok(ActionResponse {
            message: format!("added bypass domain {}", item.destination),
        })
    }

    async fn deny(&self, request_id: &str) -> AppResult<ActionResponse> {
        let mut fixture = self.inner.lock().expect("fixture lock");
        let before = fixture.blocked.len();
        fixture.blocked.retain(|item| item.request_id != request_id);
        if fixture.blocked.len() == before {
            return Err(AppError::NotFound(format!(
                "no blocked request found for {request_id}"
            )));
        }
        fixture.events.insert(
            0,
            EventItem {
                timestamp: Utc::now(),
                event_type: "denied_via_control_plane".to_string(),
                request_id: Some(request_id.to_string()),
                details: "Denied request".to_string(),
            },
        );
        Ok(ActionResponse {
            message: format!("denied {request_id}"),
        })
    }

    async fn list_events(&self, limit: usize) -> AppResult<EventsResponse> {
        let fixture = self.inner.lock().expect("fixture lock");
        Ok(EventsResponse {
            events: fixture.events.iter().take(limit).cloned().collect(),
        })
    }

    async fn get_security_level(&self) -> AppResult<LevelResponse> {
        Ok(LevelResponse {
            level: self.inner.lock().expect("fixture lock").level.clone(),
        })
    }

    async fn set_security_level(&self, level: &str) -> AppResult<LevelResponse> {
        if !matches!(level, "relaxed" | "balanced" | "strict") {
            return Err(AppError::Validation(
                "invalid security level: expected relaxed, balanced, or strict".to_string(),
            ));
        }
        let mut fixture = self.inner.lock().expect("fixture lock");
        fixture.level = level.to_string();
        fixture.events.insert(
            0,
            EventItem {
                timestamp: Utc::now(),
                event_type: "level_changed".to_string(),
                request_id: None,
                details: format!("Security level changed to {level}"),
            },
        );
        Ok(LevelResponse {
            level: level.to_string(),
        })
    }

    async fn list_rules(&self) -> AppResult<RulesResponse> {
        Ok(RulesResponse {
            rules: self.inner.lock().expect("fixture lock").rules.clone(),
        })
    }

    async fn add_rule(&self, pattern: &str, action: &str) -> AppResult<ActionResponse> {
        let mut fixture = self.inner.lock().expect("fixture lock");
        fixture.rules.push(RuleItem {
            pattern: pattern.to_string(),
            action: action.to_string(),
        });
        Ok(ActionResponse {
            message: format!("auto-approve rule set: {pattern} -> {action}"),
        })
    }

    async fn add_rule_from_request(
        &self,
        request: &RuleCreateRequest,
    ) -> AppResult<ActionResponse> {
        self.add_rule(&request.pattern, &request.action).await
    }

    async fn delete_rule(&self, pattern: &str) -> AppResult<ActionResponse> {
        self.inner
            .lock()
            .expect("fixture lock")
            .rules
            .retain(|rule| rule.pattern != pattern);
        Ok(ActionResponse {
            message: format!("deleted auto-approve rule {pattern}"),
        })
    }

    async fn list_credential_allows(&self) -> AppResult<CredentialAllowsResponse> {
        Ok(CredentialAllowsResponse {
            items: self
                .inner
                .lock()
                .expect("fixture lock")
                .credential_allows
                .clone(),
        })
    }

    async fn delete_credential_allow(
        &self,
        pattern: &str,
        host: &str,
        fingerprint: &str,
    ) -> AppResult<ActionResponse> {
        self.inner
            .lock()
            .expect("fixture lock")
            .credential_allows
            .retain(|item| {
                item.pattern != pattern || item.host != host || item.fingerprint != fingerprint
            });
        Ok(ActionResponse {
            message: format!("deleted credential allow for {pattern} on {host}"),
        })
    }
}

#[async_trait]
impl WorkspaceStore for TestStore {
    async fn get_workspace(&self) -> AppResult<WorkspaceResponse> {
        Ok(WorkspaceResponse {
            status: "running".to_string(),
            uptime_seconds: Some(3_600),
            containers: ContainerSummary {
                total: 2,
                healthy: 2,
                unhealthy: 0,
                starting: 0,
            },
            networks: HashMap::from([("gateway-bridge".to_string(), "10.20.0.0/24".to_string())]),
        })
    }

    async fn get_agent(&self) -> AppResult<AgentResponse> {
        Ok(AgentResponse {
            name: "openclaw".to_string(),
            display_name: "OpenClaw".to_string(),
            version: "1.0.0".to_string(),
            status: "running".to_string(),
            health: "healthy".to_string(),
            uptime_seconds: Some(3_540),
            ports: Vec::new(),
            resources: ResourceUsage {
                memory_usage_mb: 512,
                memory_limit_mb: 4_096,
                cpu_percent: 8.1,
            },
            stale: false,
        })
    }

    async fn list_containers(&self) -> AppResult<ContainersResponse> {
        Ok(ContainersResponse {
            containers: vec![ContainerInfo {
                name: "polis-workspace".to_string(),
                service: "workspace".to_string(),
                status: "running".to_string(),
                health: "healthy".to_string(),
                uptime_seconds: Some(3_600),
                memory_usage_mb: 512,
                memory_limit_mb: 4_096,
                cpu_percent: 8.1,
                network: "gateway-bridge".to_string(),
                ip: "10.20.0.10".to_string(),
                stale: false,
            }],
        })
    }
}

#[async_trait]
impl MetricsStore for TestStore {
    async fn get_metrics(&self) -> AppResult<MetricsResponse> {
        Ok(MetricsResponse {
            timestamp: Utc::now(),
            system: SystemMetrics {
                total_memory_usage_mb: 768,
                total_memory_limit_mb: 4_096,
                total_cpu_percent: 12.5,
                container_count: 2,
            },
            containers: Vec::new(),
        })
    }

    async fn get_metrics_history(&self, minutes: u32) -> AppResult<MetricsHistoryResponse> {
        Ok(MetricsHistoryResponse {
            interval_seconds: 10,
            points: vec![MetricsPoint {
                timestamp: Utc::now(),
                total_memory_usage_mb: u64::from(minutes),
                total_cpu_percent: 12.5,
            }],
        })
    }
}

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
impl RuntimeConfigStore for TestStore {
    fn normalize_bypass_domain(&self, domain: &str) -> AppResult<String> {
        Ok(domain.trim().to_ascii_lowercase())
    }

    fn display_bypass_domain(&self, domain: &str) -> String {
        domain.to_string()
    }

    async fn get_config(&self) -> AppResult<ConfigResponse> {
        let fixture = self.inner.lock().expect("fixture lock").clone();
        Ok(ConfigResponse {
            security: cp_api_types::SecurityOverview {
                level: fixture.level,
                protected_paths: vec!["~/.ssh".to_string()],
            },
            auto_approve_rules: fixture.rules,
            bypass_domains_count: fixture.bypass_domains.len(),
            agent: ConfigAgentResponse {
                name: "openclaw".to_string(),
                version: "1.0.0".to_string(),
            },
        })
    }

    async fn get_security_config(&self) -> AppResult<SecurityConfigResponse> {
        let fixture = self.inner.lock().expect("fixture lock").clone();
        Ok(SecurityConfigResponse {
            level: fixture.level,
            auto_approve_rules: fixture.rules,
        })
    }

    async fn set_security_level_via_config(&self, level: &str) -> AppResult<ActionResponse> {
        self.inner.lock().expect("fixture lock").level = level.to_string();
        Ok(ActionResponse {
            message: format!("security level set to {level}"),
        })
    }

    async fn list_bypass_domains(&self) -> AppResult<BypassListResponse> {
        let fixture = self.inner.lock().expect("fixture lock").clone();
        Ok(BypassListResponse {
            domains: fixture.bypass_domains.clone(),
            total: fixture.bypass_domains.len(),
            source: "runtime".to_string(),
        })
    }

    async fn add_bypass_domain(&self, domain: &str) -> AppResult<ActionResponse> {
        self.inner
            .lock()
            .expect("fixture lock")
            .bypass_domains
            .push(domain.to_string());
        Ok(ActionResponse {
            message: format!("added bypass domain {domain}"),
        })
    }

    async fn delete_bypass_domain(&self, domain: &str) -> AppResult<ActionResponse> {
        self.inner
            .lock()
            .expect("fixture lock")
            .bypass_domains
            .retain(|entry| entry != domain);
        Ok(ActionResponse {
            message: format!("removed bypass domain {domain}"),
        })
    }
}

#[async_trait]
impl AuthStore for TestStore {
    fn auth_enabled(&self) -> bool {
        self.inner.lock().expect("fixture lock").auth_enabled
    }

    async fn validate_token(&self, token: &str) -> AppResult<Role> {
        match token {
            "polis_admin_token" => Ok(Role::Admin),
            "polis_operator_token" => Ok(Role::Operator),
            "polis_viewer_token" => Ok(Role::Viewer),
            "polis_agent_token" => Ok(Role::Agent),
            _ => Err(AppError::Validation(
                "invalid authentication token".to_string(),
            )),
        }
    }

    async fn register_auth_failure(&self, _client_id: &str, _reason: &str) -> AppResult<bool> {
        let mut fixture = self.inner.lock().expect("fixture lock");
        fixture.auth_failures += 1;
        Ok(false)
    }
}

async fn request_body_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf8 body")
}

async fn read_sse_until(
    stream: &mut (impl futures::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin),
    timeout: Duration,
    predicate: impl Fn(&str) -> bool,
) -> String {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut text = String::new();

    loop {
        if predicate(&text) {
            return text;
        }

        let chunk = tokio::time::timeout_at(deadline, stream.next())
            .await
            .expect("chunk timeout")
            .expect("chunk present")
            .expect("chunk bytes");
        text.push_str(std::str::from_utf8(&chunk).expect("utf8 chunk"));
    }
}

#[tokio::test]
async fn health_route_returns_ok() {
    let router = TestStore::new().router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn root_route_serves_embedded_html() {
    let router = TestStore::new().router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = request_body_text(response).await;
    assert!(body.contains("Polis Control Plane"));
    assert!(body.contains("dashboard-section"));
    assert!(body.contains("blocked-section"));
    assert!(body.contains("events-section"));
    assert!(body.contains("rules-section"));
}

#[tokio::test]
async fn strict_cors_preflight_is_present() {
    let router = TestStore::new().router();

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/v1/status")
                .header(header::ORIGIN, "http://localhost:9080")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "Content-Type")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&header::HeaderValue::from_static("http://localhost:9080"))
    );
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_HEADERS),
        Some(&header::HeaderValue::from_static("content-type"))
    );
}

#[tokio::test]
async fn auth_enabled_cors_allows_authorization_header() {
    let store = TestStore::new();
    store.set_auth_enabled(true);
    let router = store.router();

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/v1/status")
                .header(header::ORIGIN, "http://localhost:9080")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(
                    header::ACCESS_CONTROL_REQUEST_HEADERS,
                    "Content-Type, Authorization",
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_HEADERS),
        Some(&header::HeaderValue::from_static(
            "content-type,authorization"
        ))
    );
}

#[tokio::test]
async fn dependency_unavailable_maps_to_503() {
    let store = TestStore::new();
    store.set_fail_status(true);
    let router = store.router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn workspace_endpoints_return_snapshots() {
    let router = TestStore::new().router();

    for uri in [
        "/api/v1/workspace",
        "/api/v1/agent",
        "/api/v1/containers",
        "/api/v1/config",
        "/api/v1/config/security",
        "/api/v1/config/bypass",
        "/api/v1/logs",
        "/api/v1/logs/workspace",
        "/api/v1/metrics",
        "/api/v1/metrics/history",
    ] {
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

        assert_eq!(response.status(), StatusCode::OK, "{uri}");
    }
}

#[tokio::test]
async fn delete_rule_accepts_url_encoded_pattern() {
    let store = TestStore::new();
    let router = store.router();

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/v1/config/rules?pattern=*%2Eexample%2Ecom%2Fpath")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(store.inner.lock().expect("fixture lock").rules.is_empty());
}

#[tokio::test]
async fn auth_enabled_routes_require_a_valid_token() {
    let store = TestStore::new();
    store.set_auth_enabled(true);
    let router = store.router();

    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .header(header::AUTHORIZATION, "Bearer polis_viewer_token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(authorized.status(), StatusCode::OK);
}

#[tokio::test]
async fn viewer_cannot_mutate_config_when_auth_is_enabled() {
    let store = TestStore::new();
    store.set_auth_enabled(true);
    let router = store.router();

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/v1/config/security")
                .header(header::AUTHORIZATION, "Bearer polis_viewer_token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"level":"strict"}"#))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn operator_can_bypass_blocked_domain_when_auth_is_enabled() {
    let store = TestStore::new();
    store.set_auth_enabled(true);
    let router = store.router();

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/blocked/req-abc12345/bypass-domain")
                .header(header::AUTHORIZATION, "Bearer polis_operator_token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        store
            .inner
            .lock()
            .expect("fixture lock")
            .bypass_domains
            .contains(&"example.com".to_string())
    );
}

#[tokio::test]
async fn sse_stream_sends_initial_snapshot() {
    let router = TestStore::new().router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/stream")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    let mut stream = response.into_body().into_data_stream();
    let text = read_sse_until(&mut stream, Duration::from_secs(1), |text| {
        text.contains("event: status")
            && text.contains("event: blocked")
            && text.contains("event: event_log")
            && text.contains("event: workspace")
            && text.contains("event: agent")
            && text.contains("event: metrics")
    })
    .await;

    assert!(text.contains("event: status"));
    assert!(text.contains("event: blocked"));
    assert!(text.contains("event: event_log"));
    assert!(text.contains("event: workspace"));
    assert!(text.contains("event: agent"));
    assert!(text.contains("event: metrics"));
}

#[tokio::test]
async fn sse_stream_broadcasts_after_mutation() {
    let store = TestStore::new();
    let router = store.router();

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/stream")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    let mut stream = response.into_body().into_data_stream();
    let _initial = read_sse_until(&mut stream, Duration::from_secs(1), |text| {
        text.contains("event: status")
            && text.contains("event: blocked")
            && text.contains("event: event_log")
            && text.contains("event: workspace")
            && text.contains("event: agent")
            && text.contains("event: metrics")
    })
    .await;

    let mutation_response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/blocked/req-abc12345/approve")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("mutation response");
    assert_eq!(mutation_response.status(), StatusCode::OK);

    let text = read_sse_until(&mut stream, Duration::from_secs(1), |text| {
        text.contains("approved_via_control_plane")
            || text.contains("\"pending_count\":0")
            || text.contains("event: blocked")
    })
    .await;

    assert!(
        text.contains("approved_via_control_plane")
            || text.contains("\"pending_count\":0")
            || text.contains("event: blocked")
    );
}

#[tokio::test]
async fn credential_allow_routes_work() {
    let store = TestStore::new();
    store
        .inner
        .lock()
        .expect("fixture lock")
        .credential_allows
        .clear();
    let router = store.router();

    let allow_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/blocked/req-abc12345/allow-credential")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("allow response");
    assert_eq!(allow_response.status(), StatusCode::OK);

    let list_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/config/credential-allows")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("list response");
    assert_eq!(list_response.status(), StatusCode::OK);

    let body = to_bytes(list_response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let response: CredentialAllowsResponse =
        serde_json::from_slice(&body).expect("credential allows response");
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].pattern, "aws_access");

    let delete_response = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(
                    "/api/v1/config/credential-allows?pattern=aws_access&host=example.com&fingerprint=0123456789abcdef",
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("delete response");
    assert_eq!(delete_response.status(), StatusCode::OK);
    assert!(
        store
            .inner
            .lock()
            .expect("fixture lock")
            .credential_allows
            .is_empty()
    );
}

#[tokio::test]
async fn blocked_bypass_domain_route_works() {
    let store = TestStore::new();
    store.inner.lock().expect("fixture lock").blocked = vec![BlockedItem {
        request_id: "req-bypass01".to_string(),
        reason: "new_domain_prompt".to_string(),
        destination: "unknown.example".to_string(),
        pattern: Some("new_domain_prompt".to_string()),
        fingerprint: None,
        blocked_at: Utc::now(),
        status: "pending".to_string(),
    }];
    let router = store.router();

    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/blocked/req-bypass01/bypass-domain")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        store
            .inner
            .lock()
            .expect("fixture lock")
            .bypass_domains
            .contains(&"unknown.example".to_string())
    );
}

fn sample_logs_response(lines: usize, level: Option<&str>) -> LogsResponse {
    let mut response_lines = vec![
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
        response_lines.retain(|line| line.level == level);
    }
    if response_lines.len() > lines {
        response_lines.truncate(lines);
    }

    LogsResponse {
        total: response_lines.len(),
        truncated: false,
        lines: response_lines,
    }
}
