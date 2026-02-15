# Requirements Document

## Introduction

This feature creates the `molis-mcp-common` Rust crate — a shared library containing types, Valkey key schema, TTL constants, input validation helpers, and configuration structures used by MCP-Agent, the future MCP-Admin, ICAP approval modules, and the CLI approval tool. The crate has zero runtime dependencies (no tokio, no async) and serves as the single source of truth for the Molis security control plane data contract.

## Glossary

- **molis-mcp-common**: The shared Rust crate (`polis/crates/molis-mcp-common/`).
- **BlockReason**: Enum classifying why a request was blocked (credential, malware, URL, file).
- **SecurityLevel**: Enum for enforcement posture (Relaxed, Balanced, Strict). Defaults to Balanced.
- **BlockedRequest**: Struct representing a blocked request awaiting approval, stored in Valkey.
- **OttMapping**: Struct mapping a One-Time Token to its original request_id with a time-gate.
- **request_id**: Format `req-[a-f0-9]{8}` (12 chars). Identifier for blocked requests.
- **OTT**: Format `ott-[a-zA-Z0-9]{8}` (12 chars). One-Time Token visible to the user, replacing request_id in outbound messages.
- **Valkey**: Redis-compatible data store used for state management.

## Requirements

### Requirement 1: Core Types

**User Story:** As a platform developer, I want shared enums and structs for the security control plane, so that all consumers use consistent, type-safe data structures.

#### Acceptance Criteria

1. THE crate SHALL export `BlockReason` enum with variants: `CredentialDetected`, `MalwareDomain`, `UrlBlocked`, `FileInfected`, serialized as snake_case
2. THE crate SHALL export `SecurityLevel` enum with variants: `Relaxed`, `Balanced`, `Strict`, serialized as lowercase, with `Balanced` as the `Default`
3. THE crate SHALL export `RequestStatus` enum with variants: `Pending`, `Approved`, `Denied`
4. THE crate SHALL export `BlockedRequest` struct with fields: `request_id`, `reason`, `destination`, `pattern` (Option), `blocked_at` (DateTime<Utc>), `status`
5. THE crate SHALL export `SecurityLogEntry` struct with fields: `timestamp`, `event_type`, `request_id` (Option), `details`
6. THE crate SHALL export `OttMapping` struct with fields: `ott_code`, `request_id`, `armed_after`, `created_at`
7. THE crate SHALL export `ApprovalSource` enum with variants: `ProxyInterception`, `Cli`, `McpAdmin`, serialized as snake_case
8. THE crate SHALL export `AutoApproveAction` enum, `AutoApproveRule` struct, and `UserConfirmation` enum
9. ALL types SHALL derive `Debug`, `Clone`, `Serialize`, `Deserialize`

### Requirement 2: Valkey Key Schema & TTL Constants

**User Story:** As a platform developer, I want a single source of truth for Valkey key formats and TTL values, so that all consumers construct keys consistently and apply correct expiration.

#### Acceptance Criteria

1. THE crate SHALL export a `keys` module with constants: `BLOCKED`, `APPROVED`, `AUTO_APPROVE`, `SECURITY_LEVEL`, `EVENT_LOG`, `OTT_MAPPING`
2. THE crate SHALL export a `ttl` module with constants: `APPROVED_REQUEST_SECS` (300), `BLOCKED_REQUEST_SECS` (3600), `OTT_MAPPING_SECS` (600), `EVENT_LOG_SECS` (86400)
3. THE crate SHALL export helper functions: `blocked_key()`, `approved_key()`, `auto_approve_key()`, `ott_key()` producing keys in format `molis:{namespace}:{id}`
4. WHEN `blocked_key("req-abc12345")` is called, THEN it SHALL return `"molis:blocked:req-abc12345"`
5. WHEN `ott_key("ott-x7k9m2p4")` is called, THEN it SHALL return `"molis:ott:ott-x7k9m2p4"`

### Requirement 3: Input Validation

**User Story:** As a security engineer, I want strict input validation on request_id and OTT codes, so that malformed or injected values cannot corrupt Valkey key namespaces (CWE-20).

#### Acceptance Criteria

1. THE crate SHALL export `validate_request_id()` that enforces format `^req-[a-f0-9]{8}$` (exactly 12 chars) and returns `Result<(), &'static str>`
2. THE crate SHALL export `validate_ott_code()` that enforces format `^ott-[a-zA-Z0-9]{8}$` (exactly 12 chars) and returns `Result<(), &'static str>`
3. WHEN `validate_request_id("req-abc12345")` is called, THEN it SHALL return `Ok(())`
4. WHEN `validate_request_id("evil:inject")` is called, THEN it SHALL return `Err(...)`
5. WHEN `validate_request_id("")` is called, THEN it SHALL return `Err(...)`
6. WHEN `validate_ott_code("ott-x7k9m2p4")` is called, THEN it SHALL return `Ok(())`
7. WHEN `validate_ott_code("bad-input!!!")` is called, THEN it SHALL return `Err(...)`

### Requirement 4: Approval Constants

**User Story:** As a platform developer, I want shared approval command prefixes, OTT configuration, and domain allowlists, so that proxy interception and approval flows use consistent values.

#### Acceptance Criteria

1. THE crate SHALL export an `approval` module with constants: `APPROVE_PREFIX` (`"/polis-approve"`), `DENY_PREFIX` (`"/polis-deny"`), `DEFAULT_TIME_GATE_SECS` (15), `OTT_PREFIX` (`"ott-"`), `OTT_RANDOM_LEN` (8)
2. THE crate SHALL export `DEFAULT_APPROVAL_DOMAINS` with dot-prefixed domains: `.api.telegram.org`, `.api.slack.com`, `.discord.com` to prevent domain suffix spoofing (CWE-346)
3. THE crate SHALL export `approval_command(request_id)` helper that returns `"/polis-approve {request_id}"`
4. WHEN `approval_command("req-abc12345")` is called, THEN it SHALL return `"/polis-approve req-abc12345"`

### Requirement 5: Configuration

**User Story:** As a platform developer, I want shared configuration structs with sensible defaults, so that MCP-Agent and MCP-Admin servers can deserialize config from environment variables.

#### Acceptance Criteria

1. THE crate SHALL export `AgentServerConfig` with defaults: `listen_addr` = `0.0.0.0:8080`, `redis_url` = `redis://valkey:6379`
2. THE crate SHALL export `AdminServerConfig` with defaults: `listen_addr` = `127.0.0.1:8765`, `redis_url` = `redis://valkey:6379`
3. `AdminServerConfig` SHALL include a doc comment noting the server MUST validate `is_loopback()` at startup (CWE-1327)
4. BOTH config structs SHALL implement `Default` and `serde::Deserialize`

### Requirement 6: Crate Structure

**User Story:** As a platform developer, I want a clean crate with re-exports at the root, so that consumers can import types with minimal path nesting.

#### Acceptance Criteria

1. THE crate SHALL be named `molis-mcp-common` at path `polis/crates/molis-mcp-common/`
2. THE crate SHALL have dependencies: `serde` 1.0 (derive), `chrono` 0.4 (serde), `thiserror` 1.0
3. THE crate SHALL NOT include runtime dependencies (tokio, async-trait, deadpool-redis)
4. `lib.rs` SHALL re-export all public types, key helpers, validation functions, and config structs at the crate root
5. WHEN `cargo build -p molis-mcp-common` is executed, THE crate SHALL compile with zero warnings

## Notes

- Source of truth for all code: `odralabs-docs/docs/linear-issues/molis-oss/01-foundation-types.md`
- This crate is a blocking dependency for `molis-mcp-agent` (spec 09) and future `molis-mcp-admin` (spec 10)
- `request_id` is an identifier (32-bit entropy), not a capability token. The OTT (47.6-bit) is the capability. No entropy increase needed.
- When editing files, split all edits into chunks no greater than 50 lines
