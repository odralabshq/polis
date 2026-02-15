# Design Document: MCP-Agent Server

## Overview

This design implements two Rust crates and a Docker service:

1. **molis-mcp-common** (`polis/crates/molis-mcp-common/`) — Shared types, Valkey key schema, TTL constants, and configuration structs. Zero runtime dependencies.
2. **molis-mcp-agent** (`polis/crates/molis-mcp-agent/`) — MCP server binary exposing 5 read-only tools via SSE transport. Connects to Valkey as the `mcp-agent` ACL user.
3. **Docker service** in `polis/deploy/docker-compose.yml` — Multi-stage Rust build, dual-network (internal-bridge + gateway-bridge), security-hardened container.

The MCP-Agent acts as a controlled gateway between the Workspace agent and Valkey state. The agent can report blocks and query status but cannot approve requests. Blocked requests use a 1-hour TTL (SETEX) to prevent unbounded accumulation.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Docker Compose Stack                   │
│                                                          │
│  ┌──────────────┐    internal-bridge    ┌─────────────┐ │
│  │  workspace    │◄────────────────────►│  mcp-agent   │ │
│  │  (agent)      │    10.10.1.0/24      │  :8080 SSE   │ │
│  └──────────────┘                       └──────┬──────┘ │
│                                                │         │
│                                         gateway-bridge   │
│                                          10.30.1.0/24    │
│                                                │         │
│  ┌──────────────┐                       ┌──────┴──────┐ │
│  │  gateway      │◄───────────────────►│   valkey     │ │
│  │  (g3proxy)    │                      │   :6379 TLS  │ │
│  └──────────────┘                       └─────────────┘ │
└─────────────────────────────────────────────────────────┘

Network isolation:
  - workspace: internal-bridge only (cannot reach Valkey)
  - mcp-agent: internal-bridge + gateway-bridge (bridges the gap)
  - valkey: gateway-bridge only (not reachable from workspace)
```

## Components and Interfaces

### Component 1: molis-mcp-common Crate

**Directory**: `polis/crates/molis-mcp-common/`

**Files**:
| File | Purpose |
|---|---|
| `Cargo.toml` | Dependencies: serde 1.0 (derive), chrono 0.4 (serde), thiserror 1.0 |
| `src/lib.rs` | Re-exports from submodules |
| `src/types.rs` | `BlockReason`, `SecurityLevel`, `RequestStatus`, `BlockedRequest`, `AutoApproveAction`, `SecurityLogEntry`, `OttMapping`, `ApprovalSource`, `UserConfirmation` |
| `src/redis_keys.rs` | Key prefixes (`keys` module), TTL constants (`ttl` module), approval constants (`approval` module), helper functions (`blocked_key`, `approved_key`, `ott_key`, etc.), input validation (`validate_request_id`, `validate_ott_code`) |
| `src/config.rs` | `AgentServerConfig`, `AdminServerConfig` with env-based defaults |

**Key design decisions**:
- `BLOCKED_REQUEST_SECS = 3600` (1-hour TTL on blocked keys, per security review)
- All types derive `Serialize`/`Deserialize` with `snake_case` renaming
- `SecurityLevel` defaults to `Balanced`
- `AdminServerConfig` defaults to `127.0.0.1:8765` (localhost-only, post-MVP)
- `DEFAULT_APPROVAL_DOMAINS` uses dot-prefixed domains (`.api.telegram.org`, `.api.slack.com`, `.discord.com`) to prevent suffix spoofing (CWE-346)
- `validate_request_id()` enforces `^req-[a-f0-9]{8}$` format — consumers must call before constructing Redis keys (CWE-20)
- `validate_ott_code()` enforces `^ott-[a-zA-Z0-9]{8}$` format
- `default_redis_url()` returns `redis://valkey:6379` (correct hostname for the Valkey service)
- No runtime deps — this crate is pure data types

### Component 2: molis-mcp-agent Crate

**Directory**: `polis/crates/molis-mcp-agent/`

**Files**:
| File | Purpose |
|---|---|
| `Cargo.toml` | Dependencies: molis-mcp-common, rmcp (server + sse), tokio, deadpool-redis, redis, serde, serde_json, tracing, anyhow, envy |
| `src/main.rs` | Entry point: load config, init state, create tools, start SSE server |
| `src/state.rs` | `AppState` struct wrapping Valkey connection pool with all data operations |
| `src/tools.rs` | `MolisAgentTools` struct with 5 MCP tool implementations |

**Cargo.toml dependencies**:
```toml
[dependencies]
molis-mcp-common = { path = "../molis-mcp-common" }
rmcp = { version = "0.1", features = ["server", "transport-sse-server"] }
tokio = { version = "1.0", features = ["full"] }
deadpool-redis = "0.18"
redis = { version = "0.27", features = ["tokio-comp"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1.0"
envy = "0.4"
chrono = { version = "0.4", features = ["serde"] }
```

#### AppState (`src/state.rs`)

The `AppState` struct wraps a `deadpool-redis` connection pool and provides all Valkey operations.

**Connection setup**:
- Pool created from `MOLIS_AGENT_VALKEY_URL` (default: `redis://valkey:6379`)
- ACL authentication via `AUTH mcp-agent <password>` on each connection
- Pool size: 8 (configurable)
- Startup PING to verify connectivity

**Operations**:
| Method | Valkey Command | Key Pattern |
|---|---|---|
| `store_blocked_request(req)` | `SETEX molis:blocked:{id} 3600 {json}` | `molis:blocked:*` |
| `count_pending_approvals()` | `SCAN ... MATCH molis:blocked:*` + count | `molis:blocked:*` |
| `count_recent_approvals()` | `SCAN ... MATCH molis:approved:*` + count | `molis:approved:*` |
| `get_security_level()` | `GET molis:config:security_level` | `molis:config:*` |
| `get_pending_approvals()` | `SCAN ... MATCH molis:blocked:*` + `GET` each | `molis:blocked:*` |
| `get_security_log(limit)` | `ZREVRANGE molis:log:events 0 {limit-1}` | `molis:log:events` |
| `get_request_status(id)` | `EXISTS molis:approved:{id}`, `EXISTS molis:blocked:{id}` | Both namespaces |
| `log_event(type, id, details)` | `ZADD molis:log:events {timestamp} {json}` | `molis:log:events` |

**SCAN vs KEYS**: The `mcp-agent` ACL user has KEYS disabled (dangerous command). All namespace iteration uses `SCAN` with `MATCH` pattern and `COUNT 100`.

**TTL on blocked keys**: `store_blocked_request` uses `SETEX` with `ttl::BLOCKED_REQUEST_SECS` (3600s = 1 hour). This prevents unbounded accumulation of stale blocked requests. The `volatile-lru` eviction policy can also evict these keys under memory pressure since they now have a TTL.

#### MCP Tools (`src/tools.rs`)

Five read-only tools exposed via the `#[tool]` macro from `rmcp`:

| Tool | Input | Output | Side Effects |
|---|---|---|---|
| `report_block` | `request_id`, `reason`, `destination`, `pattern?` | message, request_id, requires_approval, approval_command | Validates request_id format, stores blocked request (SETEX 1h), logs event. Pattern redacted from agent-facing output. |
| `get_security_status` | (none) | status, pending_approvals, recent_approvals, security_level | None |
| `list_pending_approvals` | (none) | pending: Vec<BlockedRequest> (pattern=None) | None. Pattern field redacted before returning to agent. |
| `get_security_log` | (none) | entries: Vec<SecurityLogEntry>, total_count | None |
| `check_request_status` | `request_id` | request_id, status, message | Validates request_id format before Valkey lookup. |

**Security constraint**: No `approve_request`, `deny_request`, `configure_auto_approve`, or `set_security_level` tools are exposed. These operations are reserved for the CLI tool (spec 10) and future MCP-Admin.

### Component 3: Workspace Cargo.toml

**File**: `polis/Cargo.toml` (workspace root)

A Cargo workspace manifest that includes both crates:

```toml
[workspace]
members = [
    "crates/molis-mcp-common",
    "crates/molis-mcp-agent",
]
resolver = "2"
```

### Component 4: Dockerfile

**File**: `polis/build/mcp-server/Dockerfile.agent`

Multi-stage build:
1. **Builder stage** (`rust:1-bookworm`): Copy crates, build release binary
2. **Runtime stage** (`debian:bookworm-slim`): Copy binary, install ca-certificates, set env vars

```dockerfile
FROM rust:1-bookworm AS builder
WORKDIR /build
COPY crates/ crates/
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release -p molis-mcp-agent

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/molis-mcp-agent /usr/bin/
ENV RUST_LOG=info
ENV MOLIS_AGENT_LISTEN_ADDR=0.0.0.0:8080
ENV MOLIS_AGENT_VALKEY_URL=redis://valkey:6379
EXPOSE 8080
ENTRYPOINT ["/usr/bin/molis-mcp-agent"]
```

`curl` is included for the Docker health check.

### Component 5: Docker Compose Service

**File**: `polis/deploy/docker-compose.yml` (append to existing services)

```yaml
mcp-agent:
  build:
    context: ..
    dockerfile: ./build/mcp-server/Dockerfile.agent
  image: polis-mcp-agent-oss:latest
  container_name: polis-mcp-agent
  networks:
    internal-bridge: {}
    gateway-bridge: {}
  environment:
    - RUST_LOG=info
    - MOLIS_AGENT_LISTEN_ADDR=0.0.0.0:8080
    - MOLIS_AGENT_VALKEY_URL=redis://valkey:6379
    - MOLIS_AGENT_VALKEY_USER=mcp-agent
    - MOLIS_AGENT_VALKEY_PASS=${VALKEY_MCP_AGENT_PASS}
  depends_on:
    valkey:
      condition: service_healthy
  healthcheck:
    test: ["CMD", "curl", "-sf", "http://localhost:8080/health"]
    interval: 10s
    timeout: 5s
    retries: 3
    start_period: 15s
  security_opt:
    - no-new-privileges:true
  cap_drop:
    - ALL
  restart: unless-stopped
  logging:
    driver: json-file
    options:
      max-size: "50m"
      max-file: "5"
      labels: "service"
  labels:
    service: "polis-mcp-agent"
```

**Network justification**:
- `internal-bridge`: Workspace agent connects to MCP-Agent on port 8080
- `gateway-bridge`: MCP-Agent connects to Valkey on port 6379
- NOT on `external-bridge`: MCP-Agent has no internet access

## Data Models

### Valkey Key Schema (from molis-mcp-common)

```
molis:blocked:{request_id}     → String (JSON BlockedRequest)  TTL: 3600s (1h)
molis:approved:{request_id}    → String ("approved")           TTL: 300s (5min)
molis:config:security_level    → String (SecurityLevel)        TTL: None
molis:config:auto_approve:{p}  → String (AutoApproveAction)    TTL: None
molis:log:events               → Sorted Set (score=timestamp)  TTL: App-level 24h
```

### ACL Permissions (mcp-agent user)

The `mcp-agent` Valkey ACL user (defined in valkey-state-management spec) has:
- **Allowed keys**: `molis:blocked:*`, `molis:approved:*`, `molis:config:*`
- **Allowed commands**: `@read`, `@write`, `@connection` (excluding `DEL`, `UNLINK`, `@admin`, `@dangerous`)
- **Denied**: `DEL`, `UNLINK`, `KEYS`, `FLUSHALL`, `FLUSHDB`, etc.

Note: The `mcp-agent` user cannot write to `molis:log:events` (that's the `log-writer` user's namespace). Event logging from MCP-Agent will need to either:
- Use a separate connection authenticated as `log-writer`, OR
- Be deferred to post-MVP (log events via stdout/tracing only)

For MVP, we'll log events via tracing (stdout) only. The `log_event` method will be a no-op that logs to tracing. Post-MVP, a dedicated log-writer connection can be added.

### File Layout

```
polis/
├── Cargo.toml                              # NEW: Workspace manifest
├── Cargo.lock                              # NEW: Generated
├── crates/
│   ├── molis-mcp-common/                   # NEW: Shared types
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs
│   │       ├── redis_keys.rs
│   │       └── config.rs
│   └── molis-mcp-agent/                    # NEW: MCP server
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── state.rs
│           └── tools.rs
├── build/
│   └── mcp-server/
│       └── Dockerfile.agent                # NEW: Multi-stage Rust build
├── deploy/
│   └── docker-compose.yml                  # MODIFIED: Add mcp-agent service
└── tests/
    ├── unit/
    │   └── mcp-agent.bats                  # NEW: Container/config tests
    └── e2e/
        └── mcp-agent.bats                  # NEW: Tool functionality tests
```

## Error Handling

### Startup Errors

| Condition | Behavior |
|---|---|
| Valkey unreachable | Log error, exit with non-zero code. Docker restarts. |
| Invalid config env vars | Log error, exit. `envy` returns parse error. |
| Port already in use | Log error, exit. |

### Runtime Errors

| Condition | Behavior |
|---|---|
| Valkey connection lost | Tool calls return error message. Server stays up. |
| Valkey ACL denied | Tool calls return error. Logged as warning. |
| Invalid tool input | MCP protocol returns validation error. |
| Pool exhausted | Tool calls block until connection available (pool timeout). |

## Testing Strategy

### Unit Tests (`polis/tests/unit/mcp-agent.bats`)

Container-level verification (no MCP protocol interaction):
- Container exists, running, healthy
- On correct networks (internal-bridge, gateway-bridge, NOT external-bridge)
- Security hardening (no-new-privileges, cap_drop ALL)
- Health check endpoint responds
- Environment variables set correctly
- Depends on Valkey (service dependency)

### E2E Tests (`polis/tests/e2e/mcp-agent.bats`)

Tool-level verification via HTTP/SSE:
- `report_block` stores data in Valkey, returns approval command
- `check_request_status` returns "pending" for stored request
- `check_request_status` returns "not_found" for unknown request
- `get_security_status` returns valid JSON with counts
- `list_pending_approvals` returns stored requests

E2E tests interact with the MCP-Agent via its HTTP endpoint and verify Valkey state using `valkey-cli` from the Valkey container.

## Correctness Properties

### Property 1: No write tools exposed

For any MCP tool discovery request, the response SHALL NOT contain tools named `approve_request`, `deny_request`, `configure_auto_approve`, or `set_security_level`.

### Property 2: Blocked request TTL

For any call to `report_block`, the stored key `molis:blocked:{request_id}` SHALL have a TTL between 1 and 3600 seconds (never -1/no-expiry).

### Property 3: Network isolation

The MCP-Agent container SHALL be reachable from the workspace container on port 8080, and SHALL be able to reach the Valkey container on port 6379, but SHALL NOT be reachable from the external-bridge network.

### Property 4: Input validation on request_id

For any call to `report_block` or `check_request_status` with a `request_id` that does not match `^req-[a-f0-9]{8}$`, the tool SHALL return an error/not_found response without constructing a Redis key or querying Valkey.

### Property 5: Pattern redaction in agent responses

For any `BlockedRequest` returned by `list_pending_approvals` or any message returned by `report_block`, the `pattern` field SHALL be `None`/absent. The pattern is stored in Valkey (for admin/CLI use) but never exposed to the agent.

## PS
- While editing files, you must split all edints into chunks of max 50 lines.