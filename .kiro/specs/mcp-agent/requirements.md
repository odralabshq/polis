# Requirements Document

## Introduction

This feature creates the MCP-Agent server — a Rust-based MCP (Model Context Protocol) server exposing read-only tools to the AI agent running inside the Workspace container. The agent uses these tools to report blocked requests, query approval status, and retrieve security logs. The MCP-Agent connects to Valkey for state persistence and authenticates using the `mcp-agent` ACL user. It sits on both `internal-bridge` (reachable by workspace) and `gateway-bridge` (reaches Valkey). The MCP-Agent cannot approve requests — approvals happen out-of-band via the approval system (spec 10).

This spec also includes the `molis-mcp-common` foundation types crate, which provides shared types, Valkey key schema, and configuration structures used by both MCP-Agent and the future MCP-Admin/approval system.

## Glossary

- **MCP**: Model Context Protocol. An open protocol for AI tool integration, implemented via the `rmcp` Rust crate.
- **MCP-Agent**: The Rust binary (`molis-mcp-agent`) exposing read-only tools to the Workspace agent via SSE transport.
- **molis-mcp-common**: Shared Rust crate containing types, Valkey key helpers, and configuration structures.
- **Valkey**: Redis-compatible in-memory data store (already deployed via the valkey-state-management spec).
- **ACL_User**: The `mcp-agent` Valkey ACL user with restricted permissions (read/write to `molis:blocked:*`, `molis:approved:*`, `molis:config:*`; no DEL/UNLINK).
- **Blocked_Request**: A JSON-serialized request stored at `molis:blocked:{request_id}` with a 1-hour TTL.
- **Approved_Request**: A key at `molis:approved:{request_id}` with a 5-minute TTL.
- **Event_Log**: A sorted set at `molis:log:events` scored by Unix timestamp.
- **SSE**: Server-Sent Events transport for MCP communication.
- **OTT**: One-Time Token. The proxy REQMOD rewriter replaces request_id with OTT before the message reaches the user (spec 10).
- **internal-bridge**: Docker network (`10.10.1.0/24`) connecting workspace and gateway.
- **gateway-bridge**: Docker network (`10.30.1.0/24`) connecting gateway-tier services and Valkey.

## Requirements

### Requirement 1: Foundation Types Crate (molis-mcp-common)

**User Story:** As a platform developer, I want shared types and Valkey key helpers in a common crate, so that MCP-Agent and future services use consistent data structures.

#### Acceptance Criteria

1. WHEN `cargo build -p molis-mcp-common` is executed, THE crate SHALL compile successfully with serde, chrono, and thiserror dependencies
2. THE crate SHALL export `BlockReason`, `SecurityLevel`, `RequestStatus`, `BlockedRequest`, `AutoApproveAction`, `SecurityLogEntry`, and `OttMapping` types with serde Serialize/Deserialize derives
3. THE crate SHALL export Valkey key helper functions: `blocked_key()`, `approved_key()`, `auto_approve_key()`, `ott_key()` that produce keys in the format `molis:{namespace}:{id}`
4. THE crate SHALL export TTL constants: `APPROVED_REQUEST_SECS` (300), `BLOCKED_REQUEST_SECS` (3600), `OTT_MAPPING_SECS` (600), `EVENT_LOG_SECS` (86400)
5. THE crate SHALL export `AgentServerConfig` with defaults: listen_addr `0.0.0.0:8080`, valkey_url `redis://valkey:6379`
6. THE crate SHALL default `SecurityLevel` to `Balanced`
7. THE crate SHALL NOT include any runtime dependencies (tokio, async-trait) — only serde, chrono, thiserror
8. THE crate SHALL export `validate_request_id()` that enforces the format `^req-[a-f0-9]{8}$` (exactly 12 chars) and returns `Result<(), &'static str>`
9. THE crate SHALL export `validate_ott_code()` that enforces the format `^ott-[a-zA-Z0-9]{8}$` (exactly 12 chars) and returns `Result<(), &'static str>`
10. THE crate SHALL define `DEFAULT_APPROVAL_DOMAINS` with dot-prefixed domains (`.api.telegram.org`, `.api.slack.com`, `.discord.com`) to prevent domain suffix spoofing (CWE-346)

### Requirement 2: MCP-Agent Server Binary

**User Story:** As an AI agent in the Workspace, I want an MCP server with read-only tools, so that I can report blocks, check approval status, and view security logs without being able to approve requests myself.

#### Acceptance Criteria

1. WHEN `cargo build -p molis-mcp-agent` is executed, THE binary SHALL compile successfully
2. THE server SHALL expose exactly 5 MCP tools: `report_block`, `get_security_status`, `list_pending_approvals`, `get_security_log`, `check_request_status`
3. THE server SHALL NOT expose any write/approve tools (`approve_request`, `deny_request`, `configure_auto_approve`, `set_security_level`)
4. WHEN `report_block` is called, THE server SHALL validate `request_id` format via `validate_request_id()` and reject invalid IDs with an error response before touching Valkey
5. WHEN `report_block` is called with a valid request_id, THE server SHALL store the blocked request in Valkey at `molis:blocked:{request_id}` with a 1-hour TTL using SETEX, log a security event, and return a human-readable message with an approval command containing the raw request_id
6. WHEN `report_block` returns a response, THE server SHALL NOT include the DLP pattern in the agent-facing message (pattern is stored in Valkey for admin/CLI use but redacted from agent output to prevent DLP ruleset exfiltration — CWE-200)
7. WHEN `get_security_status` is called, THE server SHALL return the count of pending approvals, recent approvals, and current security level from Valkey
8. WHEN `list_pending_approvals` is called, THE server SHALL return all blocked requests using SCAN (not KEYS) to iterate the `molis:blocked:*` namespace, with the `pattern` field set to `None` in each returned `BlockedRequest`
9. WHEN `get_security_log` is called, THE server SHALL return the most recent 50 events from the `molis:log:events` sorted set
10. WHEN `check_request_status` is called, THE server SHALL validate `request_id` format via `validate_request_id()`, then check `molis:approved:{id}` and `molis:blocked:{id}` keys and return "approved", "pending", or "not_found"
9. THE server SHALL use SSE (Server-Sent Events) transport via `rmcp::transport::sse_server`
10. THE server SHALL read configuration from environment variables prefixed with `MOLIS_AGENT_`

### Requirement 3: Valkey Authentication and Connection

**User Story:** As a security engineer, I want the MCP-Agent to authenticate to Valkey using the `mcp-agent` ACL user, so that it can only access the key namespaces it needs.

#### Acceptance Criteria

1. THE server SHALL connect to Valkey using the URL from `MOLIS_AGENT_VALKEY_URL` environment variable (default: `redis://valkey:6379`)
2. THE server SHALL authenticate using the `mcp-agent` ACL user with credentials from `MOLIS_AGENT_VALKEY_USER` and `MOLIS_AGENT_VALKEY_PASS` environment variables
3. THE server SHALL use connection pooling via `deadpool-redis` with a configurable pool size (default: 8)
4. THE server SHALL verify connectivity at startup by sending a PING command and exit with an error if Valkey is unreachable
5. THE server SHALL use SCAN instead of KEYS for iterating key namespaces (KEYS is disabled in Valkey ACL)
6. IF Valkey becomes unavailable during operation, THEN the server SHALL return error responses to tool calls and log warnings, but SHALL NOT crash

### Requirement 4: Docker Service Definition

**User Story:** As an infrastructure operator, I want the MCP-Agent running as a Docker container in the compose stack, so that it integrates with the existing Polis infrastructure.

#### Acceptance Criteria

1. THE MCP-Agent service SHALL be defined in `polis/deploy/docker-compose.yml` with container name `polis-mcp-agent`
2. THE service SHALL be built from `polis/build/mcp-server/Dockerfile.agent` using a multi-stage Rust build
3. THE service SHALL connect to both `internal-bridge` (workspace access) and `gateway-bridge` (Valkey access) networks
4. THE service SHALL NOT connect to `external-bridge` (no internet access)
5. THE service SHALL depend on the `valkey` service being healthy
6. THE service SHALL pass Valkey ACL credentials via environment variables (from Docker secrets or .env)
7. THE service SHALL include a health check that verifies the HTTP endpoint responds
8. THE service SHALL run with `no-new-privileges:true` security option and `cap_drop: ALL`
9. THE service SHALL use `json-file` logging with 50MB max size and 5 file rotation
10. THE service SHALL restart automatically using `unless-stopped` policy

### Requirement 5: Blocked Request TTL

**User Story:** As a platform developer, I want blocked requests to expire after 1 hour, so that stale requests don't accumulate indefinitely in Valkey.

#### Acceptance Criteria

1. WHEN `report_block` stores a blocked request, THE server SHALL use SETEX with a 3600-second (1-hour) TTL instead of plain SET
2. THE TTL constant SHALL be defined in `molis-mcp-common` as `BLOCKED_REQUEST_SECS = 3600`
3. IF a blocked request expires before being approved, THEN `check_request_status` SHALL return "not_found"

### Requirement 6: Testing

**User Story:** As a developer, I want BATS tests for the MCP-Agent container, so that I can verify it's correctly deployed and configured.

#### Acceptance Criteria

1. THE test suite SHALL include unit tests in `polis/tests/unit/mcp-agent.bats` verifying container state, network connectivity, and configuration
2. THE unit tests SHALL verify the container exists, is running, and is healthy
3. THE unit tests SHALL verify the container is on `internal-bridge` and `gateway-bridge` but NOT on `external-bridge`
4. THE unit tests SHALL verify the container has `no-new-privileges` and `cap_drop: ALL`
5. THE unit tests SHALL verify the MCP-Agent HTTP endpoint responds on port 8080
6. THE test suite SHALL include e2e tests in `polis/tests/e2e/mcp-agent.bats` verifying tool functionality through the MCP protocol
7. THE e2e tests SHALL verify `report_block` stores data in Valkey and returns an approval command
8. THE e2e tests SHALL verify `check_request_status` returns correct status for pending, approved, and unknown requests

## Notes

- Post-MVP: Block reporting should move to the proxy layer (DLP writes to Valkey directly via hiredis), eliminating the need for the agent to call `report_block`.
- The approval command returned by `report_block` contains the raw `request_id`. The proxy REQMOD rewriter (spec 10) replaces it with an OTT before the message reaches the user.
- When editing files, split all edits into chunks no greater than 50 lines.
