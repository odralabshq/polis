# Design Document: Foundation Types (molis-mcp-common)

## Overview

The `molis-mcp-common` crate is a pure-data Rust library providing shared types, Valkey key schema, TTL constants, input validation, and configuration structures for the Molis security control plane. It has zero runtime dependencies and serves as the single source of truth consumed by MCP-Agent, MCP-Admin, ICAP modules, and the CLI approval tool.

Source of truth: #[[file:odralabs-docs/docs/linear-issues/molis-oss/01-foundation-types.md]]

## Architecture

```
molis-mcp-common (this crate)
├── types.rs        → Enums + structs (BlockReason, SecurityLevel, BlockedRequest, etc.)
├── redis_keys.rs   → Key prefixes, TTLs, approval constants, validation, helpers
├── config.rs       → AgentServerConfig, AdminServerConfig with defaults
└── lib.rs          → Re-exports everything at crate root
```

Consumers:
```
molis-mcp-agent ──depends──► molis-mcp-common
molis-mcp-admin ──depends──► molis-mcp-common  (future, spec 10)
ICAP DLP module ──reads───► approval constants  (via config file, not Rust dep)
CLI tool ────────depends──► molis-mcp-common  (future, spec 10)
```

## Components and Interfaces

### Component 1: types.rs

Defines all shared enums and structs for the security control plane.

**Enums:**
| Type | Variants | Serde | Notes |
|---|---|---|---|
| `BlockReason` | `CredentialDetected`, `MalwareDomain`, `UrlBlocked`, `FileInfected` | snake_case | Classifies block trigger |
| `SecurityLevel` | `Relaxed`, `Balanced`, `Strict` | lowercase | Default = `Balanced` |
| `RequestStatus` | `Pending`, `Approved`, `Denied` | lowercase | State machine states |
| `UserConfirmation` | `Yes`, `No`, `Approve`, `Allow`, `Deny` | lowercase | User input variants |
| `AutoApproveAction` | `Allow`, `Prompt`, `Block` | lowercase | Rule action |
| `ApprovalSource` | `ProxyInterception`, `Cli`, `McpAdmin` | snake_case | Audit trail |

**Structs:**
| Type | Fields | Notes |
|---|---|---|
| `BlockedRequest` | `request_id`, `reason`, `destination`, `pattern` (Option), `blocked_at`, `status` | JSON-serialized into Valkey |
| `SecurityLogEntry` | `timestamp`, `event_type`, `request_id` (Option), `details` | Audit log entries |
| `OttMapping` | `ott_code`, `request_id`, `armed_after`, `created_at` | Time-gated OTT→request_id mapping |
| `AutoApproveRule` | `pattern`, `action` | Config rule |

All types derive `Debug`, `Clone`, `Serialize`, `Deserialize`. Timestamps use `chrono::DateTime<Utc>`.

### Component 2: redis_keys.rs

Contains three submodules and top-level helper functions.

**`keys` module — Key prefix constants:**
| Constant | Value | Format | TTL |
|---|---|---|---|
| `BLOCKED` | `"molis:blocked"` | `molis:blocked:{request_id}` | 3600s |
| `APPROVED` | `"molis:approved"` | `molis:approved:{request_id}` | 300s |
| `AUTO_APPROVE` | `"molis:config:auto_approve"` | `molis:config:auto_approve:{pattern}` | None |
| `SECURITY_LEVEL` | `"molis:config:security_level"` | Single key | None |
| `EVENT_LOG` | `"molis:log:events"` | Sorted set | App-level 24h |
| `OTT_MAPPING` | `"molis:ott"` | `molis:ott:{ott_code}` | 600s |

**`ttl` module — TTL constants (seconds):**
| Constant | Value | Purpose |
|---|---|---|
| `APPROVED_REQUEST_SECS` | 300 | Approved allowlist window (5 min) |
| `BLOCKED_REQUEST_SECS` | 3600 | Blocked request expiry (1 hour) |
| `OTT_MAPPING_SECS` | 600 | OTT validity window (10 min) |
| `EVENT_LOG_SECS` | 86400 | Log retention (24 hours) |

**`approval` module — Approval flow constants:**
| Constant | Value | Purpose |
|---|---|---|
| `APPROVE_PREFIX` | `"/polis-approve"` | Chat command prefix |
| `DENY_PREFIX` | `"/polis-deny"` | Chat deny command |
| `DEFAULT_TIME_GATE_SECS` | 15 | OTT armed-after delay |
| `OTT_PREFIX` | `"ott-"` | OTT code prefix |
| `OTT_RANDOM_LEN` | 8 | Random suffix length |
| `DEFAULT_APPROVAL_DOMAINS` | `.api.telegram.org`, `.api.slack.com`, `.discord.com` | Dot-prefixed for suffix-safe matching (CWE-346) |

Plus `approval_command(request_id) -> String` helper.

**Helper functions:**
- `blocked_key(request_id)` → `"molis:blocked:{request_id}"`
- `approved_key(request_id)` → `"molis:approved:{request_id}"`
- `auto_approve_key(pattern)` → `"molis:config:auto_approve:{pattern}"`
- `ott_key(ott_code)` → `"molis:ott:{ott_code}"`

**Validation functions:**
- `validate_request_id(id)` → enforces `^req-[a-f0-9]{8}$`, returns `Result<(), &'static str>`
- `validate_ott_code(code)` → enforces `^ott-[a-zA-Z0-9]{8}$`, returns `Result<(), &'static str>`

Security: consumers MUST call validation before constructing Valkey keys from untrusted input (CWE-20).

### Component 3: config.rs

Two configuration structs with serde `Deserialize` and `Default` impls.

| Struct | Field | Default | Notes |
|---|---|---|---|
| `AgentServerConfig` | `listen_addr` | `0.0.0.0:8080` | Workspace-facing |
| `AgentServerConfig` | `redis_url` | `redis://valkey:6379` | Valkey hostname |
| `AdminServerConfig` | `listen_addr` | `127.0.0.1:8765` | Localhost-only |
| `AdminServerConfig` | `redis_url` | `redis://valkey:6379` | Valkey hostname |

`AdminServerConfig` includes a doc comment: the server binary MUST validate `listen_addr.ip().is_loopback()` at startup and hard-fail on non-loopback (CWE-1327). This validation belongs in the server, not the types crate.

### Component 4: lib.rs

Re-exports all public items at crate root:
- `pub use types::*` — all enums and structs
- `pub use redis_keys::{keys, ttl, approval, blocked_key, approved_key, auto_approve_key, ott_key, validate_request_id, validate_ott_code}`
- `pub use config::{AgentServerConfig, AdminServerConfig}`

### Component 5: Cargo.toml

```toml
[package]
name = "molis-mcp-common"
version = "0.1.0"
edition = "2021"
description = "Shared types for Molis MCP servers"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1.0"
```

No runtime dependencies. No tokio, no async-trait, no deadpool-redis.

## Data Models

### Valkey Key Schema

```
molis:blocked:{request_id}     → JSON BlockedRequest    TTL: 3600s (1h)
molis:approved:{request_id}    → "approved"              TTL: 300s (5min)
molis:config:security_level    → SecurityLevel string    TTL: None
molis:config:auto_approve:{p}  → AutoApproveAction       TTL: None
molis:log:events               → Sorted Set (score=ts)   TTL: App-level 24h
molis:ott:{ott_code}           → JSON OttMapping         TTL: 600s (10min)
```

### State Transitions

```
[*] → Pending : Request blocked
Pending → Approved : User approves (via proxy/CLI/admin)
Pending → Denied : User denies
Approved → [*] : TTL expires (5min)
Denied → [*] : Removed from queue
```

## File Layout

```
polis/crates/molis-mcp-common/
├── Cargo.toml
└── src/
    ├── lib.rs          (re-exports)
    ├── types.rs        (enums, structs)
    ├── redis_keys.rs   (keys, ttl, approval, helpers, validation)
    └── config.rs       (AgentServerConfig, AdminServerConfig)
```

## Testing Strategy

Unit tests in `src/` modules (standard Rust `#[cfg(test)]`):

1. **types.rs tests**: Serialize/deserialize round-trips for all types, `SecurityLevel::default()` == `Balanced`
2. **redis_keys.rs tests**: Key helper output format, validation accept/reject cases
3. **config.rs tests**: Default values for both config structs

All tests run via `cargo test -p molis-mcp-common`. No external dependencies needed.

## Correctness Properties

1. All types round-trip through `serde_json` without data loss
2. `SecurityLevel::default()` always returns `Balanced`
3. `AdminServerConfig::default().listen_addr` is always loopback
4. `validate_request_id` rejects any string not matching `^req-[a-f0-9]{8}$`
5. `validate_ott_code` rejects any string not matching `^ott-[a-zA-Z0-9]{8}$`
6. Key helpers produce deterministic output for the same input
7. No runtime panics — `default_*_addr()` uses `.parse().unwrap()` on compile-time-known valid strings only
