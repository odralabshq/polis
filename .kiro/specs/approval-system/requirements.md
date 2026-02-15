# Requirements Document

## Introduction

This feature implements a multi-channel HITL (Human-In-The-Loop) approval system for the Molis OSS security control plane. When the DLP module blocks a request, the agent reports the block and the user approves or denies it through one of two channels: proxy interception (ICAP REQMOD/RESPMOD with OTT rewriting) or a standalone CLI tool. The system prevents agent self-approval through a defense-in-depth triad: knowledge separation (OTT rewriting), timing (time-gate), channel scoping (dot-prefixed domain allowlist), and context binding (origin host verification).

## Glossary

- **OTT**: One-Time Token. Format `ott-[a-zA-Z0-9]{8}` (12 chars). Replaces `request_id` in outbound messages so the agent cannot predict the approval code.
- **Time-Gate**: Configurable delay (default 15s) before an OTT becomes valid. Prevents self-approval via API echo responses.
- **Context Binding**: OTT is bound to the originating host. RESPMOD verifies the response comes from the same host that triggered OTT generation.
- **REQMOD Rewriter**: c-ICAP module (`srv_molis_approval_rewrite`) that intercepts `/polis-approve req-*` in outbound bodies and replaces `request_id` with OTT.
- **RESPMOD Scanner**: c-ICAP module (`srv_molis_approval`) that scans inbound responses from allowlisted domains for OTT codes and writes approvals.
- **CLI Tool**: Standalone Rust binary (`molis-approve`) for host-based approval/denial.
- **Dot-Prefixed Domain**: Domain format `.api.slack.com` that enforces dot-boundary matching, preventing `evil-slack.com` from matching `slack.com` (CWE-346).

## Requirements

### Requirement 1: REQMOD Approval Code Rewriter

**User Story:** As a security engineer, I want outbound `/polis-approve` commands rewritten with a random OTT code, so that the agent cannot predict or forge approval codes.

#### Acceptance Criteria

1. THE REQMOD module SHALL be implemented as a c-ICAP REQMOD service in `polis/build/icap/srv_molis_approval_rewrite.c`
2. THE module SHALL scan outbound request bodies for the pattern `/polis-approve[[:space:]]+(req-[a-f0-9]{8})`
3. WHEN a match is found AND the request_id exists in `molis:blocked:*`, THE module SHALL generate a 12-char OTT using `/dev/urandom` and perform length-preserving substitution
4. THE module SHALL validate request_id format (`req-` prefix + 8 hex chars) before processing (CWE-116)
5. IF `/dev/urandom` is unavailable OR `fread()` returns fewer than 8 bytes, THE module SHALL fail closed — pass the request through unmodified and log a CRITICAL error. No PRNG fallback (CWE-330, CWE-457).
6. THE module SHALL store OTT mappings in Valkey using `SET ... NX EX` (not SETEX) to prevent collision overwrites, with retry on collision
7. THE OTT mapping SHALL include: `ott_code`, `request_id`, `armed_after` (now + time_gate_secs), and `origin_host` (destination Host header for context binding)
8. THE module SHALL enforce body size limit (`MAX_BODY_SCAN = 2MB`) before regex scanning (CWE-400)
9. THE module SHALL log all OTT rewrites to `molis:log:events` with full mapping but SHALL NOT log credential values
10. THE time-gate duration SHALL be configurable via `MOLIS_APPROVAL_TIME_GATE_SECS` env var (default: 15)

### Requirement 2: RESPMOD OTT Scanner

**User Story:** As a security engineer, I want inbound responses from messaging platforms scanned for OTT codes, so that user approvals are detected and processed automatically.

#### Acceptance Criteria

1. THE RESPMOD module SHALL be implemented as a c-ICAP RESPMOD service in `polis/build/icap/srv_molis_approval.c`
2. THE module SHALL only scan responses from domains matching the dot-prefixed allowlist (CWE-346)
3. THE domain matching function SHALL enforce dot-boundary: `.slack.com` matches `api.slack.com` but NOT `evil-slack.com`
4. THE default allowlist SHALL use dot-prefixed domains: `.api.telegram.org`, `.api.slack.com`, `.discord.com`
5. THE domain allowlist SHALL be configurable via `MOLIS_APPROVAL_DOMAINS` env var (comma-separated, dot-prefixed)
6. WHEN an OTT is found AND the time-gate has elapsed AND the origin_host matches the response host, THE module SHALL resolve OTT → request_id and write approval
7. WHEN an OTT is found BUT the time-gate has NOT elapsed, THE module SHALL ignore it (sendMessage echo protection)
8. WHEN an OTT is found BUT origin_host does NOT match response host, THE module SHALL reject it (cross-channel replay prevention)
9. THE module SHALL preserve blocked request data in the audit log BEFORE deleting the blocked key
10. THE module SHALL strip processed OTT codes from the response body before forwarding to the agent
11. THE module SHALL enforce body size limit (`MAX_BODY_SCAN = 2MB`) before regex scanning (CWE-400)
12. THE module SHALL handle gzip-compressed response bodies (decompress before scan, recompress after)

### Requirement 3: Approval Configuration File

**User Story:** As a security engineer, I want time-gate and domain settings in a configuration file, so that I can tune approval behavior without recompiling.

#### Acceptance Criteria

1. THE config file SHALL be created at `polis/config/molis_approval.conf`
2. THE config SHALL define `time_gate_secs` (default: 15)
3. THE config SHALL define dot-prefixed domain allowlist entries using `approval_domain.N` format
4. THE config SHALL define `ott_ttl_secs` (default: 600) and `approval_ttl_secs` (default: 300)
5. ALL domain entries SHALL use dot-prefixed format per `01-foundation-types.md` guidance

### Requirement 4: Valkey ACL Rules

**User Story:** As a security engineer, I want per-component Valkey ACL rules, so that the agent cannot directly write approvals (CWE-285).

#### Acceptance Criteria

1. THE design SHALL define a `governance-reqmod` user with access to `molis:ott:*`, `molis:blocked:*`, `molis:log:*` (get, set, setnx, exists, zadd)
2. THE design SHALL define a `governance-respmod` user with access to `molis:ott:*`, `molis:blocked:*`, `molis:approved:*`, `molis:log:*` (get, del, setex, exists, zadd)
3. THE design SHALL define a `mcp-agent` user with read-only access to `molis:blocked:*`, `molis:approved:*` (get, setex, exists, scan)
4. THE design SHALL define a `mcp-admin` user with full access to `molis:*` namespace
5. THE agent (mcp-agent) SHALL NOT have write access to `molis:approved:*` or `molis:ott:*`

### Requirement 5: CLI Approval Tool

**User Story:** As a system administrator, I want a CLI tool to approve or deny blocked requests from the host, so that I can manage approvals without a messaging platform.

#### Acceptance Criteria

1. THE CLI SHALL be implemented as a Rust binary at `polis/crates/molis-approve-cli/src/main.rs`
2. THE CLI SHALL support subcommands: `approve`, `deny`, `list-pending`, `set-security-level`, `auto-approve`
3. THE CLI SHALL accept Valkey password ONLY via `MOLIS_VALKEY_PASS` env var, NOT as a CLI argument (CWE-214)
4. THE CLI SHALL preserve blocked request data in the audit log before deleting the blocked key
5. THE CLI SHALL log approve/deny actions to `molis:log:events` with event_type `approved_via_cli` / `denied_via_cli`
6. THE CLI SHALL connect to Valkey with TLS (`rediss://`) and ACL authentication as `mcp-admin`

### Requirement 6: ICAP Build & Configuration

**User Story:** As a developer, I want both approval modules compiled and configured in the existing ICAP container, so that they integrate with the DLP module.

#### Acceptance Criteria

1. THE Dockerfile SHALL compile both `srv_molis_approval_rewrite.c` and `srv_molis_approval.c` with `-lhiredis` linking
2. THE c-ICAP config SHALL load both modules and register service aliases
3. THE g3proxy config SHALL chain REQMOD: DLP → approval rewriter, and RESPMOD: approval scanner
4. THE g3proxy config SHALL specify fail-closed behavior: `icap_reqmod_on_error: block` and `icap_respmod_on_error: block`
5. THE Docker Compose SHALL mount `molis_approval.conf` read-only into the ICAP container

### Requirement 7: Foundation Types Updates

**User Story:** As a platform developer, I want OTT-related types and context binding fields in `molis-mcp-common`, so that all consumers use consistent structures.

#### Acceptance Criteria

1. THE `OttMapping` struct SHALL include `origin_host: String` field for context binding
2. THE `approval` module SHALL export `DEFAULT_APPROVAL_DOMAINS` with dot-prefixed domains
3. THE `validate_ott_code()` and `validate_request_id()` functions SHALL be used by the CLI before any Valkey operations

## Notes

- Source of truth: `odralabs-docs/docs/linear-issues/molis-oss/10-approval-system.md`
- Security review: `odralabs-docs/docs/review/10-approval-system-review.md`
- Depends on: `01-foundation-types` (molis-mcp-common crate), `07-redis-state` (Valkey), `08-dlp-module` (ICAP container)
- The existing ICAP Dockerfile from `08-dlp-module` must be extended, not replaced
- When editing files, split all edits into chunks no greater than 50 lines
