# Implementation Plan: MCP-Agent Server

## Overview

Incremental implementation of the MCP-Agent server and its foundation types dependency. Tasks build sequentially: shared types first, then the server crate, Docker integration, and finally tests. The Rust workspace is created under `polis/` alongside the existing shell/C infrastructure.

## Tasks

- [x] 1. Create Cargo workspace and molis-mcp-common crate
  - [x] 1.1 Create `polis/Cargo.toml` workspace manifest
    - Define `[workspace]` with `members = ["crates/molis-mcp-common", "crates/molis-mcp-agent"]`
    - Set `resolver = "2"`
    - _Requirements: 1.1_

  - [x] 1.2 Create `polis/crates/molis-mcp-common/Cargo.toml`
    - Package: name `molis-mcp-common`, version `0.1.0`, edition `2021`
    - Dependencies: serde 1.0 (derive), chrono 0.4 (serde), thiserror 1.0
    - _Requirements: 1.1, 1.7_

  - [x] 1.3 Create `polis/crates/molis-mcp-common/src/types.rs`
    - Define `BlockReason` enum (CredentialDetected, MalwareDomain, UrlBlocked, FileInfected) with serde snake_case
    - Define `SecurityLevel` enum (Relaxed, Balanced, Strict) with Default = Balanced
    - Define `RequestStatus` enum (Pending, Approved, Denied)
    - Define `BlockedRequest` struct (request_id, reason, destination, pattern, blocked_at, status)
    - Define `AutoApproveAction` enum (Allow, Prompt, Block)
    - Define `SecurityLogEntry` struct (timestamp, event_type, request_id, details)
    - Define `OttMapping` struct (ott_code, request_id, armed_after, created_at)
    - Define `ApprovalSource` enum (ProxyInterception, Cli, McpAdmin)
    - Define `UserConfirmation` enum (Yes, No, Approve, Allow, Deny)
    - Define `AutoApproveRule` struct (pattern, action)
    - _Requirements: 1.2_

  - [x] 1.4 Create `polis/crates/molis-mcp-common/src/redis_keys.rs`
    - Define `keys` module with constants: BLOCKED, APPROVED, AUTO_APPROVE, SECURITY_LEVEL, EVENT_LOG, OTT_MAPPING
    - Define `ttl` module with constants: APPROVED_REQUEST_SECS (300), BLOCKED_REQUEST_SECS (3600), OTT_MAPPING_SECS (600), EVENT_LOG_SECS (86400)
    - Define `approval` module with constants: APPROVE_PREFIX, DENY_PREFIX, DEFAULT_TIME_GATE_SECS (15), OTT_PREFIX, OTT_RANDOM_LEN (8), DEFAULT_APPROVAL_DOMAINS (dot-prefixed: `.api.telegram.org`, `.api.slack.com`, `.discord.com`), and `approval_command()` helper
    - Define helper functions: `blocked_key()`, `approved_key()`, `auto_approve_key()`, `ott_key()`
    - Define `validate_request_id()` — enforce `^req-[a-f0-9]{8}$` format, return `Result<(), &'static str>`
    - Define `validate_ott_code()` — enforce `^ott-[a-zA-Z0-9]{8}$` format, return `Result<(), &'static str>`
    - _Requirements: 1.3, 1.4, 1.8, 1.9, 1.10_

  - [x] 1.5 Create `polis/crates/molis-mcp-common/src/config.rs`
    - Define `AgentServerConfig` with listen_addr (default 0.0.0.0:8080) and valkey_url (default redis://valkey:6379)
    - Define `AdminServerConfig` with listen_addr (default 127.0.0.1:8765) and valkey_url
    - Implement `Default` for both
    - _Requirements: 1.5_

  - [x] 1.6 Create `polis/crates/molis-mcp-common/src/lib.rs`
    - Declare and re-export all submodules: types, redis_keys, config
    - Re-export key types, functions, and validation helpers at crate root (including `validate_request_id`, `validate_ott_code`)
    - _Requirements: 1.1_

  - [x] 1.7 Verify `cargo build -p molis-mcp-common` compiles
    - Run `cargo build -p molis-mcp-common` from `polis/` directory
    - Fix any compilation errors
    - _Requirements: 1.1, 1.6_

- [x] 2. Create molis-mcp-agent crate
  - [x] 2.1 Create `polis/crates/molis-mcp-agent/Cargo.toml`
    - Package: name `molis-mcp-agent`, version `0.1.0`, edition `2021`
    - Dependencies: molis-mcp-common (path), rmcp (server + sse), tokio (full), deadpool-redis, redis (tokio-comp), serde (derive), serde_json, tracing, tracing-subscriber (env-filter), anyhow, envy, chrono (serde)
    - _Requirements: 2.1_

  - [x] 2.2 Create `polis/crates/molis-mcp-agent/src/state.rs`
    - Define `AppState` struct with `deadpool-redis::Pool`
    - Implement `AppState::new(valkey_url, user, password)` — create pool, test PING
    - Implement `store_blocked_request(req)` — SETEX with 3600s TTL
    - Implement `count_pending_approvals()` — SCAN (not KEYS) molis:blocked:* + count
    - Implement `count_recent_approvals()` — SCAN (not KEYS) molis:approved:* + count
    - Implement `get_security_level()` — GET molis:config:security_level
    - Implement `get_pending_approvals()` — SCAN + GET each, redact `pattern` field to `None` before returning
    - Implement `get_security_log(limit)` — ZREVRANGE molis:log:events
    - Implement `get_request_status(id)` — EXISTS approved, EXISTS blocked
    - Implement `log_event(type, id, details)` — tracing::info only (MVP)
    - ⚠️ All namespace iteration MUST use SCAN with MATCH/COUNT, never KEYS (disabled in mcp-agent ACL)
    - _Requirements: 2.4-2.10, 3.1-3.5, 5.1_

  - [x] 2.3 Create `polis/crates/molis-mcp-agent/src/tools.rs`
    - Define `MolisAgentTools` struct holding `Arc<AppState>`
    - Implement `report_block` tool — validate request_id via `validate_request_id()`, store request, log event, return approval command. Redact pattern from agent-facing message (CWE-200).
    - Implement `get_security_status` tool — query counts and security level
    - Implement `list_pending_approvals` tool — return all pending requests with `pattern` field set to `None`
    - Implement `get_security_log` tool — return recent 50 events
    - Implement `check_request_status` tool — validate request_id, then check approved/blocked/not_found
    - Define input/output structs: ReportBlockInput, ReportBlockOutput, CheckRequestStatusInput, CheckRequestStatusOutput, SecurityStatusOutput, PendingApprovalsOutput, SecurityLogOutput
    - _Requirements: 2.2, 2.3, 2.4-2.10_

  - [x] 2.4 Create `polis/crates/molis-mcp-agent/src/main.rs`
    - Initialize tracing with RUST_LOG env filter
    - Load config via `envy::prefixed("MOLIS_AGENT_")`
    - Create AppState with Valkey connection (ACL auth)
    - Create MolisAgentTools with Arc<AppState>
    - Start SSE server via `rmcp::transport::sse_server::SseServer::serve`
    - _Requirements: 2.1, 2.9, 2.10, 3.1-3.4_

  - [x] 2.5 Verify `cargo build -p molis-mcp-agent` compiles
    - Run `cargo build -p molis-mcp-agent` from `polis/` directory
    - Fix any compilation errors
    - _Requirements: 2.1_

- [x] 3. Create Docker integration
  - [x] 3.1 Create `polis/build/mcp-server/Dockerfile.agent`
    - Builder stage: rust:1-bookworm, copy crates + workspace Cargo.toml, cargo build --release
    - Runtime stage: debian:bookworm-slim, install ca-certificates + curl, copy binary
    - Set env vars: RUST_LOG, MOLIS_AGENT_LISTEN_ADDR, MOLIS_AGENT_VALKEY_URL
    - Expose port 8080, entrypoint molis-mcp-agent
    - _Requirements: 4.2_

  - [x] 3.2 Add mcp-agent service to `polis/deploy/docker-compose.yml`
    - Build context, image name, container name polis-mcp-agent
    - Networks: internal-bridge, gateway-bridge (NOT external-bridge)
    - Environment: RUST_LOG, MOLIS_AGENT_* vars, Valkey ACL credentials
    - depends_on valkey (service_healthy)
    - Health check: curl http://localhost:8080/health
    - Security: no-new-privileges, cap_drop ALL
    - Logging: json-file, 50m, 5 files
    - Restart: unless-stopped
    - _Requirements: 4.1-4.10_

- [x] 4. Create test suites
  - [x] 4.1 Create `polis/tests/unit/mcp-agent.bats`
    - Container state: exists, running, healthy
    - Network: on internal-bridge, on gateway-bridge, NOT on external-bridge
    - Security: no-new-privileges, cap_drop ALL
    - Health endpoint: curl responds on port 8080
    - Environment: MOLIS_AGENT_* vars set
    - _Requirements: 6.1-6.5_

  - [x] 4.2 Create `polis/tests/e2e/mcp-agent.bats`
    - report_block: POST to MCP endpoint, verify Valkey key created with TTL
    - check_request_status: verify pending/not_found responses
    - get_security_status: verify valid JSON response
    - list_pending_approvals: verify stored requests returned
    - _Requirements: 6.6-6.8_

- [x] 5. Final checkpoint
  - Verify `cargo build --release -p molis-mcp-agent` succeeds
  - Verify Docker image builds successfully
  - Run unit and e2e tests, fix any failures
  - Ask user to run `polis.sh up --local` and execute tests

## Notes

- All file edits must be split into chunks no greater than 50 lines
- The `mcp-agent` ACL user is already defined in the valkey-state-management spec's secrets generator
- Event logging to Valkey (`molis:log:events`) requires the `log-writer` ACL user, not `mcp-agent`. For MVP, events are logged via tracing (stdout) only.
- The `rmcp` crate version and API may need adjustment based on the actual published version — verify with `cargo search rmcp` before implementation
- Post-MVP: Block reporting should move to the proxy layer (DLP writes to Valkey directly via hiredis)
