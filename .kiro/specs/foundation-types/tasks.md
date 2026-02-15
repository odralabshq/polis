# Implementation Plan: Foundation Types (molis-mcp-common)

## Overview

Create the `molis-mcp-common` Rust crate with shared types, Valkey key schema, input validation, and configuration. All code is defined in the architectural spec (`01-foundation-types.md`). Tasks create each source file, wire up the crate, and verify compilation.

## Tasks

- [x] 1. Create Cargo workspace and crate scaffold
  - [x] 1.1 Create `polis/Cargo.toml` workspace manifest
    - Define `[workspace]` with `members = ["crates/molis-mcp-common"]`
    - Set `resolver = "2"`
    - Note: `molis-mcp-agent` will be added to members when its spec is implemented
    - _Requirements: 6.1_

  - [x] 1.2 Create `polis/crates/molis-mcp-common/Cargo.toml`
    - Package: name `molis-mcp-common`, version `0.1.0`, edition `2021`
    - Dependencies: serde 1.0 (derive), chrono 0.4 (serde), thiserror 1.0
    - No runtime dependencies
    - _Requirements: 6.2, 6.3_

- [x] 2. Create types.rs
  - [x] 2.1 Create `polis/crates/molis-mcp-common/src/types.rs`
    - Define `BlockReason` enum (CredentialDetected, MalwareDomain, UrlBlocked, FileInfected) with `#[serde(rename_all = "snake_case")]`
    - Define `SecurityLevel` enum (Relaxed, Balanced, Strict) with `#[serde(rename_all = "lowercase")]` and `#[default] Balanced`
    - Define `RequestStatus` enum (Pending, Approved, Denied) with lowercase serde
    - Define `UserConfirmation` enum (Yes, No, Approve, Allow, Deny) with lowercase serde
    - Define `AutoApproveAction` enum (Allow, Prompt, Block) with lowercase serde
    - Define `ApprovalSource` enum (ProxyInterception, Cli, McpAdmin) with snake_case serde
    - Define `BlockedRequest` struct with fields: request_id, reason, destination, pattern (Option), blocked_at (DateTime<Utc>), status
    - Define `SecurityLogEntry` struct with fields: timestamp, event_type, request_id (Option), details
    - Define `OttMapping` struct with fields: ott_code, request_id, armed_after, created_at
    - Define `AutoApproveRule` struct with fields: pattern, action
    - All types derive Debug, Clone, Serialize, Deserialize
    - Copy code from `01-foundation-types.md` Section 2.1
    - _Requirements: 1.1–1.9_

- [x] 3. Create redis_keys.rs
  - [x] 3.1 Create `polis/crates/molis-mcp-common/src/redis_keys.rs`
    - Define `keys` module with constants: BLOCKED, APPROVED, AUTO_APPROVE, SECURITY_LEVEL, EVENT_LOG, OTT_MAPPING
    - Define `ttl` module with constants: APPROVED_REQUEST_SECS (300), BLOCKED_REQUEST_SECS (3600), OTT_MAPPING_SECS (600), EVENT_LOG_SECS (86400)
    - Define `approval` module with: APPROVE_PREFIX, DENY_PREFIX, DEFAULT_TIME_GATE_SECS (15), OTT_PREFIX, OTT_RANDOM_LEN (8), DEFAULT_APPROVAL_DOMAINS (dot-prefixed), approval_command() helper
    - Define helper functions: blocked_key(), approved_key(), auto_approve_key(), ott_key()
    - Define validate_request_id() — enforce ^req-[a-f0-9]{8}$ format
    - Define validate_ott_code() — enforce ^ott-[a-zA-Z0-9]{8}$ format
    - Copy code from `01-foundation-types.md` Section 2.2
    - _Requirements: 2.1–2.5, 3.1–3.7, 4.1–4.4_

- [x] 4. Create config.rs
  - [x] 4.1 Create `polis/crates/molis-mcp-common/src/config.rs`
    - Define `AgentServerConfig` with listen_addr (default 0.0.0.0:8080) and redis_url (default redis://valkey:6379)
    - Define `AdminServerConfig` with listen_addr (default 127.0.0.1:8765) and redis_url (default redis://valkey:6379)
    - Add CWE-1327 doc comment on AdminServerConfig.listen_addr
    - Implement Default for both structs
    - Copy code from `01-foundation-types.md` Section 2.3
    - _Requirements: 5.1–5.4_

- [x] 5. Create lib.rs and verify compilation
  - [x] 5.1 Create `polis/crates/molis-mcp-common/src/lib.rs`
    - Declare modules: types, redis_keys, config
    - Re-export all public types via `pub use types::*`
    - Re-export key helpers, validation functions, modules from redis_keys
    - Re-export config structs
    - Copy code from `01-foundation-types.md` Section 2.5
    - _Requirements: 6.4_

  - [x] 5.2 Verify `cargo build -p molis-mcp-common` compiles with zero warnings
    - Run from `polis/` directory
    - Fix any compilation errors
    - _Requirements: 6.5_

- [x] 6. Add unit tests
  - [x] 6.1 Add tests to `types.rs`
    - Test serde round-trip for BlockReason, SecurityLevel, RequestStatus, BlockedRequest, OttMapping
    - Test SecurityLevel::default() == Balanced
    - Test ApprovalSource serializes to snake_case (proxy_interception, cli, mcp_admin)
    - _Requirements: 1.1–1.9_

  - [x] 6.2 Add tests to `redis_keys.rs`
    - Test blocked_key(), approved_key(), auto_approve_key(), ott_key() output format
    - Test approval_command() output
    - Test validate_request_id() accepts valid, rejects invalid (empty, wrong prefix, uppercase hex, wrong length, injection chars)
    - Test validate_ott_code() accepts valid, rejects invalid
    - Test DEFAULT_APPROVAL_DOMAINS are all dot-prefixed
    - _Requirements: 2.3–2.5, 3.1–3.7, 4.2–4.4_

  - [x] 6.3 Add tests to `config.rs`
    - Test AgentServerConfig::default() values
    - Test AdminServerConfig::default().listen_addr is loopback
    - _Requirements: 5.1–5.4_

  - [x] 6.4 Run `cargo test -p molis-mcp-common` and verify all tests pass
    - _Requirements: 6.5_

## Notes

- All code is already defined in `01-foundation-types.md` — tasks are copy + verify
- Split file edits into chunks no greater than 50 lines
- The workspace Cargo.toml initially only includes molis-mcp-common; molis-mcp-agent is added when its spec is implemented
- Strip BOM from any files created on Windows
