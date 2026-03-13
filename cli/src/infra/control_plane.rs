//! Control-plane HTTP client used by CLI commands.

use anyhow::{Context, Result, anyhow};
use cp_api_types::{
    ActionResponse, AgentResponse, BlockedListResponse, ContainersResponse, ErrorResponse,
    EventsResponse, LevelResponse, LogsResponse, MetricsResponse, RulesResponse, StatusResponse,
    WorkspaceResponse,
};
use reqwest::{Client, Method, Url, header};
use serde::de::DeserializeOwned;
use std::time::Duration;

use crate::application::ports::ControlPlanePort;
use crate::domain::config::PolisConfig;

/// Typed client for the control-plane HTTP API.
#[derive(Debug, Clone)]
pub struct ControlPlaneClient {
    client: Client,
    base_url: Url,
    token: Option<String>,
}

const CONTROL_PLANE_TIMEOUT: Duration = Duration::from_secs(5);

impl ControlPlaneClient {
    /// Build a client from a base URL and optional bearer token.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is invalid or the HTTP client cannot be
    /// constructed.
    pub fn new(base_url: &str, token: Option<String>) -> Result<Self> {
        let mut base_url = Url::parse(base_url)
            .with_context(|| format!("invalid control plane URL: {base_url}"))?;
        if !base_url.path().ends_with('/') {
            let mut path = base_url.path().to_string();
            path.push('/');
            base_url.set_path(&path);
        }

        let client = Client::builder()
            .timeout(CONTROL_PLANE_TIMEOUT)
            .build()
            .context("failed to build control-plane HTTP client")?;
        let token = token
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());

        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    /// Build a client from the current CLI configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured URL is invalid or the HTTP client
    /// cannot be constructed.
    pub fn from_config(config: &PolisConfig) -> Result<Self> {
        Self::new(
            &config.control_plane.url,
            config.control_plane.token.clone(),
        )
    }

    #[cfg(test)]
    pub(crate) fn base_url(&self) -> &Url {
        &self.base_url
    }

    #[cfg(test)]
    pub(crate) fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path.trim_start_matches('/'))
            .with_context(|| format!("failed to build control-plane endpoint for {path}"))
    }

    fn request(&self, method: Method, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.endpoint(path)?;
        let mut request = self.client.request(method, url);
        if let Some(token) = &self.token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        Ok(request)
    }

    async fn get_json<T>(&self, path: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .request(Method::GET, path)?
            .send()
            .await
            .with_context(|| format!("failed to call control-plane endpoint {path}"))?;
        decode_json(response).await
    }

    async fn get_json_at<T>(&self, url: Url) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let mut request = self.client.get(url);
        if let Some(token) = &self.token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = request
            .send()
            .await
            .context("failed to call control-plane endpoint")?;
        decode_json(response).await
    }

    /// Probe the control-plane health endpoint.
    #[must_use]
    pub async fn is_available(&self) -> bool {
        let Ok(request) = self.request(Method::GET, "/health") else {
            return false;
        };
        let Ok(response) = request.send().await else {
            return false;
        };
        response.status().is_success()
    }

    /// Fetch the current security level.
    ///
    /// # Errors
    ///
    /// Returns an error if the control-plane request fails.
    pub async fn get_level(&self) -> Result<LevelResponse> {
        self.get_json("/api/v1/config/level").await
    }

    /// Delete a rule by pattern.
    ///
    /// # Errors
    ///
    /// Returns an error if the control-plane request fails.
    pub async fn delete_rule(&self, pattern: &str) -> Result<ActionResponse> {
        let mut url = self.endpoint("/api/v1/config/rules")?;
        url.query_pairs_mut().append_pair("pattern", pattern);
        let mut request = self.client.request(Method::DELETE, url);
        if let Some(token) = &self.token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = request
            .send()
            .await
            .context("failed to delete control-plane rule")?;
        decode_action(response).await
    }

    /// Fetch logs for all services or a single service.
    ///
    /// # Errors
    ///
    /// Returns an error if the control-plane request fails.
    pub async fn get_logs(&self, service: Option<&str>, lines: usize) -> Result<LogsResponse> {
        let endpoint = if let Some(service) = service {
            format!("/api/v1/logs/{service}")
        } else {
            "/api/v1/logs".to_string()
        };
        let mut url = self.endpoint(&endpoint)?;
        url.query_pairs_mut()
            .append_pair("lines", &lines.to_string());
        self.get_json_at(url).await
    }

    /// Fetch the latest metrics snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the control-plane request fails.
    pub async fn get_metrics(&self) -> Result<MetricsResponse> {
        self.get_json("/api/v1/metrics").await
    }
}

#[allow(clippy::unused_async)]
async fn decode_json<T>(response: reqwest::Response) -> Result<T>
where
    T: DeserializeOwned,
{
    let status = response.status();
    if status.is_success() {
        return response
            .json::<T>()
            .await
            .with_context(|| format!("failed to decode control-plane response (status {status})"));
    }

    Err(anyhow!(parse_error_response(response).await?))
}

async fn parse_error_response(response: reqwest::Response) -> Result<String> {
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("failed to read control-plane error response ({status})"))?;

    if let Ok(error) = serde_json::from_str::<ErrorResponse>(&body) {
        return Ok(error.error);
    }

    let message = if body.trim().is_empty() {
        format!("control-plane request failed with status {status}")
    } else {
        body
    };
    Ok(message)
}

async fn decode_action(response: reqwest::Response) -> Result<ActionResponse> {
    decode_json(response).await
}

impl ControlPlanePort for ControlPlaneClient {
    async fn status(&self) -> Result<StatusResponse> {
        self.get_json("/api/v1/status").await
    }

    async fn blocked_requests(&self) -> Result<BlockedListResponse> {
        self.get_json("/api/v1/blocked").await
    }

    async fn approve_request(&self, request_id: &str) -> Result<ActionResponse> {
        let response = self
            .request(
                Method::POST,
                &format!("/api/v1/blocked/{request_id}/approve"),
            )?
            .header(header::CONTENT_TYPE, "application/json")
            .send()
            .await
            .context("failed to approve blocked request via control-plane")?;
        decode_action(response).await
    }

    async fn deny_request(&self, request_id: &str) -> Result<ActionResponse> {
        let response = self
            .request(Method::POST, &format!("/api/v1/blocked/{request_id}/deny"))?
            .header(header::CONTENT_TYPE, "application/json")
            .send()
            .await
            .context("failed to deny blocked request via control-plane")?;
        decode_action(response).await
    }

    async fn security_events(&self, limit: usize) -> Result<EventsResponse> {
        let mut url = self.endpoint("/api/v1/events")?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        let mut request = self.client.get(url);
        if let Some(token) = &self.token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = request
            .send()
            .await
            .context("failed to fetch security events from control-plane")?;
        decode_json(response).await
    }

    async fn rules(&self) -> Result<RulesResponse> {
        self.get_json("/api/v1/config/rules").await
    }

    async fn add_rule(&self, pattern: &str, action: &str) -> Result<ActionResponse> {
        let response = self
            .request(Method::POST, "/api/v1/config/rules")?
            .json(&serde_json::json!({
                "pattern": pattern,
                "action": action,
            }))
            .send()
            .await
            .context("failed to create control-plane rule")?;
        decode_action(response).await
    }

    async fn set_security_level(&self, level: &str) -> Result<LevelResponse> {
        let response = self
            .request(Method::PUT, "/api/v1/config/level")?
            .json(&serde_json::json!({ "level": level }))
            .send()
            .await
            .context("failed to update control-plane security level")?;
        decode_json(response).await
    }

    async fn workspace(&self) -> Result<WorkspaceResponse> {
        self.get_json("/api/v1/workspace").await
    }

    async fn agent(&self) -> Result<AgentResponse> {
        self.get_json("/api/v1/agent").await
    }

    async fn containers(&self) -> Result<ContainersResponse> {
        self.get_json("/api/v1/containers").await
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        future::Future,
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc,
        thread,
    };

    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};

    #[derive(Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: String,
    }

    struct MockResponse {
        status: u16,
        content_type: &'static str,
        body: String,
    }

    impl MockResponse {
        fn json(body: &Value) -> Self {
            Self {
                status: 200,
                content_type: "application/json",
                body: body.to_string(),
            }
        }

        fn error(status: u16, body: &Value) -> Self {
            Self {
                status,
                content_type: "application/json",
                body: body.to_string(),
            }
        }

        fn empty(status: u16) -> Self {
            Self {
                status,
                content_type: "text/plain",
                body: String::new(),
            }
        }
    }

    fn spawn_mock_server(
        response: MockResponse,
    ) -> (
        String,
        mpsc::Receiver<CapturedRequest>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept connection");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let request = read_request(&stream).expect("read request");
            tx.send(request).expect("send request");
            write_response(&mut stream, &response).expect("write response");
        });

        (format!("http://{address}"), rx, handle)
    }

    fn read_request(stream: &TcpStream) -> std::io::Result<CapturedRequest> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;

        let mut headers = HashMap::new();
        let mut content_length = 0usize;

        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line == "\r\n" || line.is_empty() {
                break;
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some((name, value)) = trimmed.split_once(':') {
                let value = value.trim().to_string();
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.insert(name.to_ascii_lowercase(), value);
            }
        }

        let mut body = vec![0_u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body)?;
        }

        let request_line = request_line.trim_end_matches(['\r', '\n']);
        let mut parts = request_line.split_whitespace();

        Ok(CapturedRequest {
            method: parts.next().unwrap_or_default().to_string(),
            path: parts.next().unwrap_or_default().to_string(),
            headers,
            body: String::from_utf8_lossy(&body).to_string(),
        })
    }

    fn write_response(stream: &mut TcpStream, response: &MockResponse) -> std::io::Result<()> {
        let reason = match response.status {
            201 => "Created",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "OK",
        };
        let header = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.status,
            reason,
            response.content_type,
            response.body.len()
        );
        stream.write_all(header.as_bytes())?;
        stream.write_all(response.body.as_bytes())?;
        stream.flush()
    }

    async fn call_client<T, F, Fut>(
        token: Option<String>,
        response: MockResponse,
        call: F,
    ) -> (T, CapturedRequest)
    where
        F: FnOnce(ControlPlaneClient) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (base_url, rx, handle) = spawn_mock_server(response);
        let client = ControlPlaneClient::new(&base_url, token).expect("client");
        let result = call(client).await.expect("call");
        let request = rx.recv_timeout(Duration::from_secs(2)).expect("request");
        handle.join().expect("server thread");
        (result, request)
    }

    async fn call_client_error<F, Fut>(
        token: Option<String>,
        response: MockResponse,
        call: F,
    ) -> (String, CapturedRequest)
    where
        F: FnOnce(ControlPlaneClient) -> Fut,
        Fut: Future<Output = Result<StatusResponse>>,
    {
        let (base_url, rx, handle) = spawn_mock_server(response);
        let client = ControlPlaneClient::new(&base_url, token).expect("client");
        let error = call(client).await.expect_err("expected error").to_string();
        let request = rx.recv_timeout(Duration::from_secs(2)).expect("request");
        handle.join().expect("server thread");
        (error, request)
    }

    fn auth_header(request: &CapturedRequest) -> Option<&str> {
        request.headers.get("authorization").map(String::as_str)
    }

    fn sample_timestamp() -> String {
        Utc.with_ymd_and_hms(2026, 3, 8, 2, 0, 0)
            .single()
            .expect("timestamp")
            .to_rfc3339()
    }

    #[test]
    fn from_config_uses_default_url_and_empty_token() {
        let config = PolisConfig::default();
        let client = ControlPlaneClient::from_config(&config).expect("client");

        assert_eq!(client.base_url.as_str(), "http://127.0.0.1:8090/");
        assert_eq!(client.token, None);
    }

    #[test]
    fn from_config_normalizes_trailing_slash() {
        let mut config = PolisConfig::default();
        config.control_plane.url = "http://localhost:9080/control".to_string();

        let client = ControlPlaneClient::from_config(&config).expect("client");

        assert_eq!(client.base_url.as_str(), "http://localhost:9080/control/");
        assert_eq!(
            client
                .endpoint("/api/v1/status")
                .expect("endpoint")
                .as_str(),
            "http://localhost:9080/control/api/v1/status"
        );
    }

    #[test]
    fn new_trims_blank_tokens() {
        let client = ControlPlaneClient::new("http://localhost:9080", Some("  ".to_string()))
            .expect("client");

        assert_eq!(client.token, None);
    }

    #[tokio::test]
    async fn is_available_checks_health_endpoint() {
        let (base_url, rx, handle) = spawn_mock_server(MockResponse::empty(200));
        let client = ControlPlaneClient::new(&base_url, None).expect("client");

        assert!(client.is_available().await);

        let request = rx.recv_timeout(Duration::from_secs(2)).expect("request");
        handle.join().expect("server thread");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/health");
        assert_eq!(auth_header(&request), None);
    }

    #[tokio::test]
    async fn status_requests_expected_endpoint() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "security_level": "balanced",
                "pending_count": 2,
                "recent_approvals": 1,
                "events_count": 5
            })),
            |client| async move { client.status().await },
        )
        .await;

        assert_eq!(response.security_level, "balanced");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/status");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn blocked_requests_requests_expected_endpoint() {
        let timestamp = sample_timestamp();
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "items": [{
                    "request_id": "req-12345678",
                    "reason": "credential_detected",
                    "destination": "https://example.test",
                    "blocked_at": timestamp,
                    "status": "pending"
                }]
            })),
            |client| async move { client.blocked_requests().await },
        )
        .await;

        assert_eq!(response.items.len(), 1);
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/blocked");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn approve_request_posts_expected_endpoint() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({ "message": "approved req-12345678" })),
            |client| async move { client.approve_request("req-12345678").await },
        )
        .await;

        assert_eq!(response.message, "approved req-12345678");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/v1/blocked/req-12345678/approve");
        assert_eq!(request.body, "");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
        assert_eq!(
            request.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[tokio::test]
    async fn deny_request_posts_expected_endpoint() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({ "message": "denied req-12345678" })),
            |client| async move { client.deny_request("req-12345678").await },
        )
        .await;

        assert_eq!(response.message, "denied req-12345678");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/v1/blocked/req-12345678/deny");
        assert_eq!(request.body, "");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn security_events_uses_limit_query_parameter() {
        let timestamp = sample_timestamp();
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "events": [{
                    "timestamp": timestamp,
                    "event_type": "block_reported",
                    "request_id": "req-12345678",
                    "details": "blocked"
                }]
            })),
            |client| async move { client.security_events(25).await },
        )
        .await;

        assert_eq!(response.events.len(), 1);
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/events?limit=25");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn rules_uses_expected_endpoint() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "rules": [{
                    "pattern": "*.example.test",
                    "action": "allow"
                }]
            })),
            |client| async move { client.rules().await },
        )
        .await;

        assert_eq!(response.rules.len(), 1);
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/config/rules");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn add_rule_posts_expected_json_body() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({ "message": "rule added" })),
            |client| async move { client.add_rule("github.com", "allow").await },
        )
        .await;

        assert_eq!(response.message, "rule added");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/v1/config/rules");
        assert_eq!(
            serde_json::from_str::<Value>(&request.body).expect("json body"),
            json!({
                "pattern": "github.com",
                "action": "allow"
            })
        );
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn get_level_uses_expected_endpoint() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({ "level": "strict" })),
            |client| async move { client.get_level().await },
        )
        .await;

        assert_eq!(response.level, "strict");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/config/level");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn set_security_level_puts_expected_json_body() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({ "level": "strict" })),
            |client| async move { client.set_security_level("strict").await },
        )
        .await;

        assert_eq!(response.level, "strict");
        assert_eq!(request.method, "PUT");
        assert_eq!(request.path, "/api/v1/config/level");
        assert_eq!(
            serde_json::from_str::<Value>(&request.body).expect("json body"),
            json!({ "level": "strict" })
        );
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn delete_rule_uses_query_parameter() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({ "message": "rule deleted" })),
            |client| async move { client.delete_rule("github.com").await },
        )
        .await;

        assert_eq!(response.message, "rule deleted");
        assert_eq!(request.method, "DELETE");
        assert_eq!(request.path, "/api/v1/config/rules?pattern=github.com");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn workspace_uses_expected_endpoint() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "status": "running",
                "uptime_seconds": 90,
                "containers": {
                    "total": 4,
                    "healthy": 3,
                    "unhealthy": 1,
                    "starting": 0
                },
                "networks": {
                    "internal_bridge": "10.10.1.0/24"
                }
            })),
            |client| async move { client.workspace().await },
        )
        .await;

        assert_eq!(response.status, "running");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/workspace");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn agent_uses_expected_endpoint() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "name": "openclaw",
                "display_name": "OpenClaw",
                "version": "1.0.0",
                "status": "running",
                "health": "healthy",
                "uptime_seconds": 90,
                "ports": [{
                    "container": 18789,
                    "host": 18789,
                    "protocol": "tcp"
                }],
                "resources": {
                    "memory_usage_mb": 512,
                    "memory_limit_mb": 4096,
                    "cpu_percent": 12.5
                }
            })),
            |client| async move { client.agent().await },
        )
        .await;

        assert_eq!(response.name, "openclaw");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/agent");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn containers_uses_expected_endpoint() {
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "containers": [{
                    "name": "polis-gate",
                    "service": "gate",
                    "status": "running",
                    "health": "healthy",
                    "uptime_seconds": 90,
                    "memory_usage_mb": 64,
                    "memory_limit_mb": 256,
                    "cpu_percent": 1.2,
                    "network": "internal",
                    "ip": "10.10.1.5"
                }]
            })),
            |client| async move { client.containers().await },
        )
        .await;

        assert_eq!(response.containers.len(), 1);
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/containers");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn get_logs_without_service_uses_lines_query() {
        let timestamp = sample_timestamp();
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "lines": [{
                    "timestamp": timestamp,
                    "service": "control-plane",
                    "level": "info",
                    "message": "ready"
                }],
                "total": 1,
                "truncated": false
            })),
            |client| async move { client.get_logs(None, 20).await },
        )
        .await;

        assert_eq!(response.total, 1);
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/logs?lines=20");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn get_logs_with_service_uses_service_endpoint() {
        let timestamp = sample_timestamp();
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "lines": [{
                    "timestamp": timestamp,
                    "service": "gate",
                    "level": "warn",
                    "message": "blocked"
                }],
                "total": 1,
                "truncated": false
            })),
            |client| async move { client.get_logs(Some("gate"), 10).await },
        )
        .await;

        assert_eq!(response.total, 1);
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/logs/gate?lines=10");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn get_metrics_uses_expected_endpoint() {
        let timestamp = sample_timestamp();
        let (response, request) = call_client(
            Some("secret-token".to_string()),
            MockResponse::json(&json!({
                "timestamp": timestamp,
                "system": {
                    "total_memory_usage_mb": 1280,
                    "total_memory_limit_mb": 8192,
                    "total_cpu_percent": 15.2,
                    "container_count": 4
                },
                "containers": [{
                    "service": "workspace",
                    "status": "running",
                    "health": "healthy",
                    "memory_usage_mb": 512,
                    "memory_limit_mb": 4096,
                    "cpu_percent": 12.5,
                    "network_rx_bytes": 1000,
                    "network_tx_bytes": 2000,
                    "pids": 42
                }]
            })),
            |client| async move { client.get_metrics().await },
        )
        .await;

        assert_eq!(response.system.container_count, 4);
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/metrics");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }

    #[tokio::test]
    async fn decode_json_surfaces_error_response_message() {
        let (error, request) = call_client_error(
            Some("secret-token".to_string()),
            MockResponse::error(403, &json!({ "error": "permission denied" })),
            |client| async move { client.status().await },
        )
        .await;

        assert_eq!(error, "permission denied");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/v1/status");
        assert_eq!(auth_header(&request), Some("Bearer secret-token"));
    }
}
