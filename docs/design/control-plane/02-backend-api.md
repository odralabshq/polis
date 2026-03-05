# Control Plane — Backend API Specification

## REST API Endpoints

All API responses use `Content-Type: application/json`. Errors return `ErrorResponse`. All API paths are versioned under `/api/v1/`.

| Method | Path | Description | Request Body | Response |
|---|---|---|---|---|
| `GET` | `/health` | Health check | — | `200 OK` |
| `GET` | `/` | Web UI (HTML) | — | `200` `text/html` |
| `GET` | `/api/v1/status` | Dashboard summary | — | `StatusResponse` |
| `GET` | `/api/v1/blocked` | List pending blocked requests | — | `BlockedListResponse` |
| `POST` | `/api/v1/blocked/{id}/approve` | Approve a blocked request | — | `ActionResponse` |
| `POST` | `/api/v1/blocked/{id}/deny` | Deny a blocked request | — | `ActionResponse` |
| `GET` | `/api/v1/events` | Security event log | — | `EventsResponse` |
| `GET` | `/api/v1/config/level` | Get security level | — | `LevelResponse` |
| `PUT` | `/api/v1/config/level` | Set security level | `LevelRequest` | `LevelResponse` |
| `GET` | `/api/v1/config/rules` | List auto-approve rules | — | `RulesResponse` |
| `POST` | `/api/v1/config/rules` | Add auto-approve rule | `RuleCreateRequest` | `ActionResponse` |
| `DELETE` | `/api/v1/config/rules` | Remove auto-approve rule | — | `ActionResponse` |
| `GET` | `/api/v1/stream` | SSE event stream (real-time updates) | — | `text/event-stream` |

**Why versioned paths:** The `polis dashboard` subcommand ships inside the CLI binary which may not be upgraded in lockstep with the Docker image. Versioning allows backwards-incompatible API changes in `/api/v2/` without breaking older CLI versions.

### Query Parameters

- `GET /api/v1/events?limit=50` — max events to return (default 50, max 200)
- `GET /api/v1/blocked?status=pending` — filter by status (default: pending only)
- `DELETE /api/v1/config/rules?pattern=*.example.com` — pattern to delete (required, URL-encoded)

**Why query param for DELETE:** Patterns may contain characters like `*`, `.`, or `/` that conflict with URL path segments (CWE-116). Using a query parameter avoids axum routing ambiguity and simplifies URL encoding.

## JSON Schemas

### `StatusResponse`

```json
{
  "security_level": "balanced",
  "pending_count": 3,
  "recent_approvals": 1,
  "events_count": 42
}
```

### `BlockedListResponse`

```json
{
  "items": [
    {
      "request_id": "req-abc12345",
      "reason": "credential_detected",
      "destination": "https://example.com/api",
      "blocked_at": "2026-03-05T19:00:00Z",
      "status": "pending"
    }
  ]
}
```

Note: The `pattern` field from `BlockedRequest` is deliberately omitted to prevent DLP ruleset exfiltration (CWE-200). This matches the existing behavior in `toolbox-server/src/tools.rs`.

### `EventsResponse`

```json
{
  "events": [
    {
      "timestamp": "2026-03-05T19:00:00Z",
      "event_type": "block_reported",
      "request_id": "req-abc12345",
      "details": "Blocked request to example.com"
    }
  ]
}
```

### `LevelRequest` / `LevelResponse`

```json
{ "level": "strict" }
```

Valid values: `relaxed`, `balanced`, `strict`.

### `RulesResponse`

```json
{
  "rules": [
    { "pattern": "*.example.com", "action": "allow" },
    { "pattern": "evil.com", "action": "block" }
  ]
}
```

### `RuleCreateRequest`

```json
{ "pattern": "*.example.com", "action": "allow" }
```

Valid actions: `allow`, `prompt`, `block`.

### `ActionResponse`

```json
{ "message": "approved req-abc12345" }
```

### `ErrorResponse`

```json
{ "error": "no blocked request found for req-abc12345" }
```

## HTTP Status Codes

| Code | When |
|---|---|
| `200` | Success |
| `400` | Invalid input (bad request_id format, invalid level/action) |
| `404` | Blocked request not found |
| `500` | Valkey connection error or internal failure |

## `cp-api-types` Crate

Shared types used by both `cp-server` and the CLI `polis dashboard` command:

```rust
// All types derive: Debug, Clone, Serialize, Deserialize

pub struct StatusResponse {
    pub security_level: String,
    pub pending_count: usize,
    pub recent_approvals: usize,
    pub events_count: usize,
}

pub struct BlockedItem {
    pub request_id: String,
    pub reason: String,       // BlockReason serialized as snake_case
    pub destination: String,
    pub blocked_at: DateTime<Utc>,
    pub status: String,       // RequestStatus serialized as lowercase
}

pub struct BlockedListResponse {
    pub items: Vec<BlockedItem>,
}

pub struct EventItem {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub request_id: Option<String>,
    pub details: String,
}

pub struct EventsResponse {
    pub events: Vec<EventItem>,
}

pub struct LevelRequest {
    pub level: String,
}

pub struct LevelResponse {
    pub level: String,
}

pub struct RuleItem {
    pub pattern: String,
    pub action: String,
}

pub struct RulesResponse {
    pub rules: Vec<RuleItem>,
}

pub struct RuleCreateRequest {
    pub pattern: String,
    pub action: String,
}

pub struct ActionResponse {
    pub message: String,
}

pub struct ErrorResponse {
    pub error: String,
}
```

**Dependencies:** `serde`, `chrono` (with `serde` feature). References `polis-common` types for documentation but does not depend on it — the API types are a deliberate boundary between the Valkey-facing server and the HTTP-facing clients.

## State Layer (`cp-server/src/state.rs`)

Replicates the pattern from `toolbox-server/src/state.rs`:

```rust
pub struct AppState {
    client: fred::prelude::Client,  // fred with rustls mTLS
}

impl AppState {
    pub async fn new(url: &str, user: &str, password: &str) -> Result<Self>;

    // Blocked requests
    pub async fn list_blocked(&self) -> Result<Vec<BlockedRequest>>;
    pub async fn approve(&self, request_id: &str) -> Result<()>;
    pub async fn deny(&self, request_id: &str) -> Result<()>;

    // Security config
    pub async fn get_security_level(&self) -> Result<SecurityLevel>;
    pub async fn set_security_level(&self, level: SecurityLevel) -> Result<()>;

    // Auto-approve rules
    pub async fn list_rules(&self) -> Result<Vec<(String, String)>>;
    pub async fn add_rule(&self, pattern: &str, action: &str) -> Result<()>;
    pub async fn delete_rule(&self, pattern: &str) -> Result<()>;

    // Events
    pub async fn list_events(&self, limit: usize) -> Result<Vec<SecurityLogEntry>>;

    // Dashboard
    pub async fn get_status(&self) -> Result<StatusSummary>;
}
```

**mTLS setup:** Identical to `toolbox-server/src/state.rs` — load CA cert, client cert, client key from PEM files, build `rustls::ClientConfig`, configure `fred::types::config::TlsConfig` with `TlsConnector::Rustls`.

**Approve workflow** (matches `approve-cli`):
1. Validate request_id via `polis_common::validate_request_id()`
2. GET blocked key — fail with 404 if not found
3. ZADD audit log entry with `event_type: "approved_via_control_plane"`
4. Atomic pipeline: DEL blocked key + SETEX approved key (TTL 300s)

**Deny workflow:**
1. Validate request_id
2. GET blocked key — fail with 404 if not found
3. ZADD audit log entry with `event_type: "denied_via_control_plane"`
4. DEL blocked key

## Configuration

Loaded via `envy::prefixed("POLIS_CP_")`:

| Env Var | Default | Description |
|---|---|---|
| `POLIS_CP_LISTEN_ADDR` | `0.0.0.0:9080` | HTTP listen address |
| `POLIS_CP_VALKEY_URL` | `rediss://valkey:6379` | Valkey connection URL |
| `POLIS_CP_VALKEY_USER` | `cp-server` | Valkey ACL username |
| `POLIS_CP_VALKEY_PASS_FILE` | `/run/secrets/valkey_cp_server_password` | Path to password file |

TLS cert paths use the same env vars as toolbox or default to:
- CA: `/etc/valkey/tls/ca.crt`
- Client cert: `/etc/valkey/tls/client.crt`
- Client key: `/etc/valkey/tls/client.key`

## Error Handling

All handlers return `axum::Json<ErrorResponse>` with appropriate status codes on failure. The axum error extractor pattern:

```rust
async fn approve_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ActionResponse>, AppError> { ... }

// AppError implements IntoResponse, mapping:
//   anyhow errors → 500 + ErrorResponse
//   validation errors → 400 + ErrorResponse
//   not-found errors → 404 + ErrorResponse
```

## Input Validation

- **request_id**: `polis_common::validate_request_id()` — must be `req-[a-f0-9]{8}`, exactly 12 chars
- **security level**: must be one of `relaxed`, `balanced`, `strict`
- **auto-approve action**: must be one of `allow`, `prompt`, `block`
- **pattern** (query param for DELETE): non-empty string, URL-decoded

## Server-Sent Events (SSE)

`GET /api/v1/stream` returns a `text/event-stream` connection that pushes real-time updates to connected clients. This replaces polling for both the TUI and web UI.

### Event Types

```
event: status
data: {"security_level":"balanced","pending_count":3,"recent_approvals":1,"events_count":42}

event: blocked
data: {"items":[...]}

event: event_log
data: {"timestamp":"2026-03-05T19:05:32Z","event_type":"block_reported","request_id":"req-abc12345","details":"Blocked request to api.example.com"}
```

- `status` — full status snapshot, sent on connect and on any state change
- `blocked` — full blocked list, sent on connect and when requests are added/approved/denied
- `event_log` — individual new event entries as they occur

### Server Implementation

```rust
// cp-server/src/sse.rs
use axum::response::sse::{Event, Sse};
use tokio::sync::broadcast;

// Background task polls Valkey every 1s, compares with previous state,
// and broadcasts changes via tokio::sync::broadcast channel.
pub async fn stream_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> { ... }
```

The broadcast channel is created once at startup. A background task polls Valkey every 1 second and broadcasts on state change. On mutation (approve/deny/rule change), the API handler also triggers an immediate broadcast so SSE clients see changes within milliseconds.

### Client Usage

**TUI (reqwest):** Opens a streaming GET, reads SSE frames, updates app state on each event.

**Web UI (JavaScript):**
```javascript
const source = new EventSource('/api/v1/stream');
source.addEventListener('status', (e) => updateDashboard(JSON.parse(e.data)));
source.addEventListener('blocked', (e) => updateBlockedTable(JSON.parse(e.data)));
source.addEventListener('event_log', (e) => prependEvent(JSON.parse(e.data)));
```

`EventSource` handles reconnection automatically with exponential backoff.

## CORS

Strict origin allowlist to prevent CSRF from malicious websites:
```
Access-Control-Allow-Origin: http://localhost:9080, http://127.0.0.1:9080
Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS
Access-Control-Allow-Headers: Content-Type
```

**Rationale:** Since the API operates without authentication on localhost, a permissive `Access-Control-Allow-Origin: *` would allow any website open in the user's browser to make requests to the control plane. The strict allowlist ensures only the control plane's own web UI can make cross-origin requests.
