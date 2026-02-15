# Design Document: Approval System

## Overview

This design implements a multi-channel HITL approval system with two channels (MVP):

1. **Channel A: Proxy Interception** — Two c-ICAP modules (REQMOD rewriter + RESPMOD scanner) that intercept approval codes in chat traffic, rewrite them with OTT codes, and detect user approvals from messaging platforms.
2. **Channel B: CLI Tool** — Standalone Rust binary for host-based approval/denial.

The system prevents agent self-approval through four mitigations:

| Mitigation | Attack Vector Blocked | Layer |
|:-----------|:---------------------|:------|
| REQMOD Code Rewriting | Agent cannot guess OTT code | Knowledge |
| Time-Gated Arming (15s) | Prevents self-approval via API echo | Timing |
| Domain Scoping (dot-prefixed) | Prevents external echo/reflection servers | Channel |
| Context Binding (origin_host) | Prevents cross-channel OTT replay | Channel |

## Architecture

```
Agent sends: POST https://api.telegram.org/.../sendMessage
  body: "Type: /polis-approve req-abc12345"
    │
    ▼ REQMOD (chains after DLP credcheck)
srv_molis_approval_rewrite:
    ├─ Generate OTT "ott-x7k9m2p4" via /dev/urandom
    ├─ Store in Valkey: SET molis:ott:ott-x7k9m2p4 {...} NX EX 600
    │   (includes origin_host: "api.telegram.org", armed_after: now+15s)
    ├─ Rewrite body: "req-abc12345" → "ott-x7k9m2p4"
    ▼
User sees: "Type: /polis-approve ott-x7k9m2p4"
User types: ott-x7k9m2p4
    │
    ▼ RESPMOD
srv_molis_approval:
    ├─ Domain check: api.telegram.org matches .api.telegram.org? YES
    ├─ OTT found: ott-x7k9m2p4
    ├─ Time-gate: now > armed_after? YES
    ├─ Context binding: response host == origin_host? YES
    ├─ Preserve blocked data → audit log
    ├─ Approve: SETEX molis:approved:req-abc12345 300 "approved"
    ├─ Strip OTT from response
    ▼
Agent polls check_request_status → "approved"
```

## Components and Interfaces

### Component 1: REQMOD Rewriter (`srv_molis_approval_rewrite.c`)

**File**: `polis/build/icap/srv_molis_approval_rewrite.c`

A c-ICAP REQMOD service that:
- Scans outbound bodies for `/polis-approve req-*` pattern
- Validates request_id format (CWE-116 prevention)
- Generates OTT via `/dev/urandom` with fail-closed error handling
- Stores OTT mapping with `SET ... NX EX` (collision-safe) including `origin_host`
- Performs length-preserving body substitution
- Enforces `MAX_BODY_SCAN` (2MB) before regex scanning

**Key data structures** (from design doc):
```c
#define MAX_BODY_SCAN 2097152
#define OTT_LEN 12

static int time_gate_secs = 15;
static int ott_ttl_secs = 600;
static regex_t approve_pattern;
static redisContext *valkey_ctx = NULL;
```

**OTT generation** — fail-closed, no PRNG fallback:
```c
static int generate_ott(char *buf, size_t buf_len);
// Returns 0 on success, -1 on failure (caller aborts rewrite)
// Uses /dev/urandom only. Checks fread() return value.
```

**OTT storage** — includes context binding:
```
SET molis:ott:{ott} {"ott_code":"{ott}","request_id":"{req_id}",
  "armed_after":{timestamp},"origin_host":"{host}"} NX EX {ttl}
```

### Component 2: RESPMOD Scanner (`srv_molis_approval.c`)

**File**: `polis/build/icap/srv_molis_approval.c`

A c-ICAP RESPMOD service that:
- Checks response Host against dot-prefixed domain allowlist
- Scans response body for OTT pattern `ott-[a-zA-Z0-9]{8}`
- Validates time-gate, context binding (origin_host match), and blocked request existence
- Preserves blocked request data in audit log before deletion
- Writes approval to `molis:approved:{request_id}` with 5-min TTL
- Strips OTT from response body
- Enforces `MAX_BODY_SCAN` (2MB) before regex scanning

**Domain matching** — dot-boundary enforcement:
```c
static int is_allowed_domain(const char *host);
// ".slack.com" matches "api.slack.com" (suffix with dot boundary)
// ".slack.com" does NOT match "evil-slack.com" (no dot before suffix)
// "slack.com" matches ".slack.com" (exact match without leading dot)
```

**Approval flow** — context-bound with audit preservation:
```c
static int process_ott_approval(const char *ott_code, const char *resp_host);
// 1. GET molis:ott:{ott} → parse JSON
// 2. Check time-gate: now >= armed_after
// 3. Check context binding: resp_host == origin_host
// 4. Check blocked request exists
// 5. GET blocked data for audit preservation
// 6. DEL blocked key, SETEX approved key
// 7. ZADD audit log with blocked_request data
// 8. DEL OTT key
```

### Component 3: Approval Configuration

**File**: `polis/config/molis_approval.conf`

```ini
time_gate_secs = 15
approval_domain.0 = .api.telegram.org
approval_domain.1 = .api.slack.com
approval_domain.2 = .discord.com
ott_ttl_secs = 600
approval_ttl_secs = 300
```

All domains use dot-prefixed format per `01-foundation-types.md`.

### Component 4: Valkey ACL Rules

**Location**: Valkey configuration / design documentation

Per-component least-privilege ACL rules:
```
user governance-reqmod  ~molis:ott:* ~molis:blocked:* ~molis:log:*                    +get +set +setnx +exists +zadd -@all
user governance-respmod ~molis:ott:* ~molis:blocked:* ~molis:approved:* ~molis:log:*   +get +del +setex +exists +zadd -@all
user mcp-agent          ~molis:blocked:* ~molis:approved:*                              +get +setex +exists +scan -@all
user mcp-admin          ~molis:*                                                        +get +del +setex +set +exists +scan +zadd +setnx -@all
```

### Component 5: CLI Tool (`molis-approve`)

**File**: `polis/crates/molis-approve-cli/src/main.rs`

Rust binary using clap for CLI parsing:
- Subcommands: `approve`, `deny`, `list-pending`, `set-security-level`, `auto-approve`
- Valkey password via `MOLIS_VALKEY_PASS` env var only (not CLI arg)
- Preserves blocked request data in audit log before deletion
- Logs all actions to `molis:log:events`

**Dependencies**: `clap 4.0`, `redis 0.27` (tokio-comp, tls-rustls), `molis-mcp-common`, `serde_json`, `anyhow`

### Component 6: ICAP Build & Config Updates

**Dockerfile** (`polis/build/icap/Dockerfile`):
- Add `libhiredis-dev` to builder, `libhiredis0.14` to runtime
- Compile both `.c` files with `-lhiredis` linking
- Copy both `.so` files to `/usr/lib/c_icap/`

**c-ICAP config** (`polis/config/c-icap.conf`):
- Load both modules, register service aliases `approval_rewrite` and `approvalcheck`

**g3proxy config** (`polis/config/g3proxy.yaml`):
- REQMOD chains DLP → approval rewriter
- RESPMOD routes to approval scanner
- Fail-closed: `icap_reqmod_on_error: block`, `icap_respmod_on_error: block`

## Data Models

### Valkey Key Schema

| Key Pattern | Type | TTL | Written By | Read By |
|---|---|---|---|---|
| `molis:ott:{ott_code}` | String (JSON) | 600s | REQMOD | RESPMOD |
| `molis:blocked:{request_id}` | String (JSON) | 3600s | MCP-Agent | REQMOD, RESPMOD, CLI |
| `molis:approved:{request_id}` | String | 300s | RESPMOD, CLI | MCP-Agent |
| `molis:log:events` | Sorted Set | 86400s | REQMOD, RESPMOD, CLI | Admin |

### OTT Mapping JSON

```json
{
  "ott_code": "ott-x7k9m2p4",
  "request_id": "req-abc12345",
  "armed_after": 1738972815,
  "origin_host": "api.telegram.org"
}
```

### File Layout

```
polis/
├── build/icap/
│   ├── Dockerfile                        # MODIFIED: add hiredis, compile both modules
│   ├── srv_molis_dlp.c                   # EXISTING (from 08-dlp-module)
│   ├── srv_molis_approval_rewrite.c      # NEW: REQMOD OTT rewriter
│   └── srv_molis_approval.c              # NEW: RESPMOD OTT scanner
├── config/
│   ├── c-icap.conf                       # MODIFIED: load approval modules
│   ├── g3proxy.yaml                      # MODIFIED: chain approval, fail-closed
│   ├── molis_approval.conf               # NEW: time-gate + domain config
│   └── molis_dlp.conf                    # EXISTING
├── crates/
│   ├── molis-mcp-common/                 # MODIFIED: add origin_host to OttMapping
│   └── molis-approve-cli/                # NEW: CLI approval tool
│       ├── Cargo.toml
│       └── src/main.rs
└── deploy/
    └── docker-compose.yml                # MODIFIED: mount molis_approval.conf
```

## Error Handling

| Condition | Behavior | Rationale |
|---|---|---|
| `/dev/urandom` unavailable | Fail closed — no OTT rewrite, CRITICAL log | CWE-330: no PRNG fallback |
| `/dev/urandom` short read | Fail closed — no OTT rewrite, CRITICAL log | CWE-457: no uninitialized memory |
| OTT collision (SETNX fails) | Retry once with new OTT | Statistically negligible but handled |
| Valkey unavailable (REQMOD) | Pass through unmodified | Approval flow disabled, not fail-open for security |
| Valkey unavailable (RESPMOD) | Pass through unmodified | Same as above |
| Body > 2MB | Skip scanning, pass through | CWE-400: resource exhaustion prevention |
| Domain not in allowlist | Ignore response entirely | Channel scoping |
| Time-gate not elapsed | Ignore OTT | Echo protection |
| Origin host mismatch | Reject OTT | Cross-channel replay prevention |
| g3proxy ICAP unavailable | Block traffic | Fail-closed via `on_error: block` |

## Testing Strategy

### Manual Integration Tests

Full-stack testing with Docker Compose:

1. **REQMOD rewrite**: Send message with `/polis-approve req-*`, verify body is rewritten with OTT
2. **Time-gate**: Verify OTT in sendMessage echo is ignored (< 15s)
3. **RESPMOD approval**: After time-gate, send OTT from allowlisted domain, verify approval written
4. **Domain scoping**: Send OTT from non-allowlisted domain, verify ignored
5. **Context binding**: Generate OTT via Telegram, replay via Slack, verify rejected
6. **CLI approve**: Run `molis-approve approve req-*`, verify approval written with audit trail
7. **CLI deny**: Run `molis-approve deny req-*`, verify removed with audit trail
8. **Fail-closed**: Stop Valkey, verify REQMOD passes through without rewrite
9. **Body size**: Send > 2MB body, verify scanning is skipped

## Correctness Properties

### Property 1: Agent cannot forge approvals
*For any* request_id, the agent cannot write to `molis:approved:{request_id}` because the `mcp-agent` ACL user lacks write access to that key namespace.

### Property 2: OTT is unpredictable
*For any* OTT generated by the REQMOD module, the OTT has 47.6 bits of entropy (8 chars from 62-char alphabet via `/dev/urandom`). No PRNG fallback exists.

### Property 3: Cross-channel replay is prevented
*For any* OTT generated for host H1, RESPMOD will reject the OTT if the response comes from host H2 where H1 ≠ H2.

### Property 4: Audit trail is complete
*For any* approval (proxy or CLI), the original blocked request data is preserved in the audit log before the blocked key is deleted.
