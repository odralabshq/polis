#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use std::{
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
    ActionResponse, BlockedItem, BlockedListResponse, EventItem, EventsResponse, LevelResponse,
    RuleCreateRequest, RuleItem, RulesResponse, StatusResponse,
};
use cp_server::{
    HttpState, build_router,
    error::{AppError, AppResult},
    state::GovernanceStore,
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
    events: Vec<EventItem>,
    fail_status: bool,
    approvals: usize,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            level: "balanced".to_string(),
            blocked: vec![BlockedItem {
                request_id: "req-abc12345".to_string(),
                reason: "credential_detected".to_string(),
                destination: "https://example.com/api".to_string(),
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
            events: vec![EventItem {
                timestamp: Utc
                    .with_ymd_and_hms(2026, 3, 5, 19, 0, 0)
                    .single()
                    .expect("valid timestamp"),
                event_type: "block_reported".to_string(),
                request_id: Some("req-abc12345".to_string()),
                details: "Blocked request to example.com".to_string(),
            }],
            fail_status: false,
            approvals: 0,
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
        let before = fixture.blocked.len();
        fixture.blocked.retain(|item| item.request_id != request_id);
        if fixture.blocked.len() == before {
            return Err(AppError::NotFound(format!(
                "no blocked request found for {request_id}"
            )));
        }
        fixture.approvals += 1;
        fixture.events.insert(
            0,
            EventItem {
                timestamp: Utc::now(),
                event_type: "approved_via_control_plane".to_string(),
                request_id: Some(request_id.to_string()),
                details: "Approved request".to_string(),
            },
        );
        Ok(ActionResponse {
            message: format!("approved {request_id}"),
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
    })
    .await;

    assert!(text.contains("event: status"));
    assert!(text.contains("event: blocked"));
    assert!(text.contains("event: event_log"));
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
