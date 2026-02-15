# Implementation Plan: Approval System

## Overview

Incremental implementation of the multi-channel HITL approval system. Tasks are ordered so the build pipeline works at each step: config and types first, then ICAP modules, then CLI, then integration wiring. The existing ICAP container from `08-dlp-module` is extended, not replaced.

## Tasks

- [x] 1. Update foundation types (`molis-mcp-common`)
  - [x] 1.1 Add `origin_host` field to `OttMapping` struct
    - Add `origin_host: String` field to `OttMapping` in `polis/crates/molis-mcp-common/src/lib.rs`
    - Update any existing serialization tests
    - _Requirements: 7.1_

  - [x] 1.2 Verify approval constants exist
    - Confirm `DEFAULT_APPROVAL_DOMAINS` uses dot-prefixed domains (`.api.telegram.org`, `.api.slack.com`, `.discord.com`)
    - Confirm `validate_request_id()` and `validate_ott_code()` are exported
    - _Requirements: 7.2, 7.3_

- [x] 2. Create approval configuration file
  - [x] 2.1 Create `polis/config/molis_approval.conf`
    - Set `time_gate_secs = 15`
    - Set dot-prefixed domain allowlist: `.api.telegram.org`, `.api.slack.com`, `.discord.com`
    - Set `ott_ttl_secs = 600`, `approval_ttl_secs = 300`
    - Add comments explaining dot-prefix requirement (CWE-346)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [x] 3. Create REQMOD rewriter module
  - [x] 3.1 Create `polis/build/icap/srv_molis_approval_rewrite.c` — includes and constants
    - Add c-ICAP, regex, hiredis includes
    - Define `MAX_BODY_SCAN 2097152`, `OTT_LEN 12`
    - Define static config vars: `time_gate_secs`, `ott_ttl_secs`, `approve_pattern`, `valkey_ctx`
    - Define service module struct (ICAP_REQMOD)
    - _Requirements: 1.1_

  - [x] 3.2 Implement `generate_ott()` — fail-closed, no PRNG fallback
    - Open `/dev/urandom`, fail closed with CRITICAL log if unavailable
    - Read 8 bytes, check `fread()` return value, fail closed on short read
    - Map bytes to alphanumeric charset, return 0 on success, -1 on failure
    - _Requirements: 1.5_

  - [x] 3.3 Implement `rewrite_init_service()` — config loading
    - Load `MOLIS_APPROVAL_TIME_GATE_SECS` from env
    - Compile approve pattern regex
    - Connect to Valkey with TLS + ACL as `governance-reqmod`
    - Set preview to 8192, enable 204
    - _Requirements: 1.10_

  - [x] 3.4 Implement `rewrite_process()` — body scanning and OTT rewriting
    - Enforce `MAX_BODY_SCAN` check before regex scan (CWE-400)
    - Extract and validate request_id format (CWE-116)
    - Check `molis:blocked:{request_id}` exists in Valkey
    - Call `generate_ott()`, abort on failure (fail-closed)
    - Capture destination Host header for context binding
    - Store OTT mapping with `SET ... NX EX` (collision-safe), retry once on collision
    - Log rewrite to `molis:log:events` with full mapping
    - Perform length-preserving body substitution
    - _Requirements: 1.2, 1.3, 1.4, 1.6, 1.7, 1.8, 1.9_

  - [x] 3.5 Implement lifecycle callbacks
    - `rewrite_init_request_data`: allocate request data struct
    - `rewrite_release_request_data`: free request data
    - `rewrite_check_preview` and `rewrite_io`: accumulate body
    - `rewrite_close_service`: free regex, disconnect Valkey
    - _Requirements: 1.1_

- [x] 4. Create RESPMOD scanner module
  - [x] 4.1 Create `polis/build/icap/srv_molis_approval.c` — includes and constants
    - Add c-ICAP, regex, hiredis includes
    - Define `MAX_BODY_SCAN 2097152`, `APPROVAL_TTL_SECS 300`, `MAX_DOMAINS 16`, `OTT_LEN 12`
    - Define static domain allowlist array, OTT regex, Valkey context
    - Define service module struct (ICAP_RESPMOD)
    - _Requirements: 2.1_

  - [x] 4.2 Implement `is_allowed_domain()` — dot-boundary matching
    - For dot-prefixed entries: check suffix match with implicit dot boundary
    - Also match exact domain without leading dot (e.g., `slack.com` matches `.slack.com`)
    - For non-dot-prefixed entries: exact match only
    - _Requirements: 2.2, 2.3_

  - [x] 4.3 Implement `approval_init_service()` — config and domain loading
    - Compile OTT regex pattern
    - Load domain allowlist from `MOLIS_APPROVAL_DOMAINS` env or defaults (dot-prefixed)
    - Connect to Valkey with TLS + ACL as `governance-respmod`
    - _Requirements: 2.4, 2.5_

  - [x] 4.4 Implement `process_ott_approval()` — context-bound approval with audit
    - Accept `ott_code` and `resp_host` parameters
    - Look up OTT mapping in Valkey, parse JSON
    - Check time-gate: `now >= armed_after`
    - Check context binding: `resp_host == origin_host`
    - Check blocked request exists
    - GET blocked request data for audit preservation
    - DEL blocked key, SETEX approved key with 5-min TTL
    - DEL OTT key
    - ZADD audit log with `approved_via_proxy`, request_id, ott_code, origin_host, blocked_request data
    - _Requirements: 2.6, 2.7, 2.8, 2.9_

  - [x] 4.5 Implement `approval_process()` — body scanning
    - Check Host against domain allowlist
    - Enforce `MAX_BODY_SCAN` check (CWE-400)
    - Handle gzip Content-Encoding (decompress before scan)
    - Scan for OTT regex, call `process_ott_approval(ott, host)`
    - Strip OTT from response body on successful approval
    - _Requirements: 2.10, 2.11, 2.12_

  - [x] 4.6 Implement lifecycle callbacks
    - Same pattern as REQMOD: init/release request data, preview, io, close
    - _Requirements: 2.1_

- [x] 5. Create CLI approval tool
  - [x] 5.1 Create `polis/crates/molis-approve-cli/Cargo.toml`
    - Add dependencies: molis-mcp-common, clap 4.0 (derive), redis 0.27 (tokio-comp, tls-rustls), tokio, serde_json, anyhow
    - Set binary name to `molis-approve`
    - _Requirements: 5.1_

  - [x] 5.2 Create `polis/crates/molis-approve-cli/src/main.rs` — CLI struct and subcommands
    - Define `Cli` struct with `valkey_url`, `valkey_user` (CLI args), `valkey_pass` (`#[arg(skip)]`, loaded from env)
    - Define `Commands` enum: Approve, Deny, ListPending, SetSecurityLevel, AutoApprove
    - In `main()`: parse CLI, load `MOLIS_VALKEY_PASS` from env, connect to Valkey with TLS + ACL
    - _Requirements: 5.2, 5.3, 5.6_

  - [x] 5.3 Implement `approve` and `deny` subcommands
    - Validate request_id format using `molis-mcp-common::validate_request_id()`
    - Check blocked request exists
    - GET blocked request data for audit preservation
    - DEL blocked key, SETEX approved key (approve) or just DEL (deny)
    - ZADD audit log with `approved_via_cli` / `denied_via_cli` + blocked_request data
    - _Requirements: 5.4, 5.5_

  - [x] 5.4 Implement `list-pending`, `set-security-level`, `auto-approve` subcommands
    - `list-pending`: SCAN for `molis:blocked:*`, GET and display each
    - `set-security-level`: validate against SecurityLevel enum, SET to Valkey
    - `auto-approve`: validate against AutoApproveAction enum, SET rule to Valkey
    - _Requirements: 5.2_

- [x] 6. Update ICAP Dockerfile
  - [x] 6.1 Update `polis/build/icap/Dockerfile` builder stage
    - Add `libhiredis-dev` to build dependencies
    - COPY both `.c` files into `/build/`
    - Compile both modules with `-lhiredis` linking and `-Wall -Werror`
    - _Requirements: 6.1_

  - [x] 6.2 Update `polis/build/icap/Dockerfile` runtime stage
    - Add `libhiredis0.14` to runtime dependencies
    - COPY both `.so` files to `/usr/lib/c_icap/`
    - _Requirements: 6.1_

- [x] 7. Update c-ICAP configuration
  - [x] 7.1 Update `polis/config/c-icap.conf`
    - Load both approval modules
    - Register service aliases: `approval_rewrite` and `approvalcheck`
    - Include `molis_approval.conf`
    - _Requirements: 6.2_

- [x] 8. Update g3proxy configuration
  - [x] 8.1 Update `polis/config/g3proxy.yaml`
    - Configure REQMOD chaining: DLP → approval rewriter
    - Configure RESPMOD: approval OTT scanner
    - Add fail-closed: `icap_reqmod_on_error: block`, `icap_respmod_on_error: block`
    - _Requirements: 6.3, 6.4_

- [x] 9. Update Docker Compose
  - [x] 9.1 Update `polis/deploy/docker-compose.yml`
    - Mount `molis_approval.conf` read-only into ICAP container
    - _Requirements: 6.5_

- [x] 10. Configure Valkey ACL rules
  - [x] 10.1 Add ACL rules to Valkey configuration
    - Define `governance-reqmod`, `governance-respmod`, `mcp-agent`, `mcp-admin` users
    - Each user has least-privilege access to specific `molis:*` key patterns
    - Verify `mcp-agent` cannot write to `molis:approved:*` or `molis:ott:*`
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

- [x] 11. Build verification
  - [x] 11.1 Verify Docker build succeeds
    - Run `docker compose build icap` from `polis/deploy/`
    - Verify no compilation errors in either approval module
    - Verify the built image contains both `.so` files in `/usr/lib/c_icap/`
    - _Requirements: 6.1_

  - [x] 11.2 Verify CLI builds
    - Run `cargo build -p molis-approve-cli`
    - Verify binary compiles with zero warnings
    - _Requirements: 5.1_

- [x] 12. Checkpoint — Full stack verification
  - Verify `docker compose up` starts all services
  - Verify c-ICAP logs show both approval modules loaded
  - Verify g3proxy connects to REQMOD and RESPMOD services
  - Verify Valkey ACL rules are applied
  - Ask user to run manual integration tests

## Notes

- The existing ICAP Dockerfile from `08-dlp-module` is extended, not replaced
- Both ICAP modules link against `libhiredis` for Valkey communication
- The CLI tool uses the `redis` Rust crate with TLS support
- All domain entries must use dot-prefixed format (CWE-346)
- Source of truth: `odralabs-docs/docs/linear-issues/molis-oss/10-approval-system.md`
- Security review: `odralabs-docs/docs/review/10-approval-system-review.md`
- When editing files, split all edits into chunks no greater than 50 lines
