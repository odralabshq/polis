# Requirements Document

## Introduction

This feature extends the existing DLP REQMOD module (`srv_polis_dlp.c`) with OTT approval rewriting and creates a new unified RESPMOD module (`srv_polis_sentinel_resp.c`) that combines ClamAV virus scanning with OTT approval detection. This works within g3proxy v1.12's single-ICAP-service-per-direction constraint without creating new REQMOD modules — the DLP module already accumulates the full body, has Valkey connectivity, and makes block/pass decisions, making OTT rewriting a natural second pass on the same data. For RESPMOD, a new module replaces squidclamav by implementing the clamd INSTREAM protocol directly alongside OTT scanning.

## Glossary

- **DLP_Module**: The existing `srv_polis_dlp.c` REQMOD service, extended in-place with OTT rewriting. Keeps its `polis_dlp` service name and `credcheck` alias.
- **Sentinel_RESPMOD**: New `srv_polis_sentinel_resp.c` RESPMOD service combining ClamAV client + OTT scanner.
- **DLP_Scan**: Existing credential detection logic — scans request bodies against configurable patterns and applies security policy.
- **OTT_Rewrite**: New logic added to the DLP module — detects `/polis-approve req-*` in outbound bodies and performs length-preserving OTT substitution.
- **ClamAV_Client**: TCP client in the RESPMOD module that speaks the clamd INSTREAM protocol to scan response bodies for malware.
- **OTT_Scanner**: Logic in the RESPMOD module that detects OTT codes in inbound responses from allowlisted domains and processes the approval flow.
- **INSTREAM_Protocol**: clamd wire protocol — send `zINSTREAM\0`, then data chunks as `[4-byte big-endian length][data]`, terminate with `[0x00000000]`, read response line.
- **Security_Policy**: Existing Valkey-backed policy in the DLP module that classifies domains and applies allow/prompt/block decisions.
- **Domain_Allowlist**: Dot-prefixed domain list (e.g., `.api.telegram.org`) used by OTT_Scanner to restrict which response sources are scanned for approval codes.

## Requirements

### Requirement 1: Extend DLP Module with OTT Rewriting

**User Story:** As a security engineer, I want OTT approval rewriting added to the existing DLP REQMOD module, so that both DLP scanning and approval code rewriting work in g3proxy's single REQMOD slot without a new module.

#### Acceptance Criteria

1. THE DLP module SHALL be extended in-place in `polis/services/sentinel/modules/dlp/srv_polis_dlp.c` — no new REQMOD module is created
2. THE module SHALL keep its existing service name `polis_dlp` and alias `credcheck` — the g3proxy REQMOD URL remains unchanged
3. AFTER DLP_Scan and Security_Policy both pass (request not blocked), THE module SHALL scan the body for the pattern `/polis-approve[[:space:]]+(req-[a-f0-9]{8})`
4. WHEN the approve pattern matches, THE module SHALL validate the request_id format (CWE-116), check that `polis:blocked:{request_id}` exists in Valkey, and check that the Host header is present for context binding
5. WHEN all validations pass, THE module SHALL generate a 12-char OTT code using `/dev/urandom` (fail-closed, no PRNG fallback per CWE-330)
6. THE module SHALL store the OTT mapping in Valkey using `SET NX EX` with JSON payload containing `ott_code`, `request_id`, `armed_after` (now + time_gate_secs), and `origin_host`
7. THE module SHALL perform length-preserving substitution of the request_id with the OTT code in the body membuf
8. WHEN OTT rewriting occurs, THE `dlp_io()` function SHALL stream from the modified membuf instead of the cached file (which contains the original unmodified body)
9. THE module SHALL maintain a second Valkey connection as `governance-reqmod` (separate from the existing `dlp-reader` connection) with its own mutex for thread safety
10. THE module SHALL log OTT rewrites to `polis:log:events` via ZADD
11. IF the governance-reqmod Valkey connection is unavailable, THE module SHALL skip OTT rewriting and pass the request through (DLP scanning continues to work independently)
12. THE time-gate duration SHALL be configurable via `POLIS_APPROVAL_TIME_GATE_SECS` env var (default: 15)

### Requirement 2: New RESPMOD Module — ClamAV + OTT Scanner

**User Story:** As a security engineer, I want ClamAV virus scanning and OTT approval detection combined in a single RESPMOD service, so that both functions operate in g3proxy's single RESPMOD slot.

#### Acceptance Criteria

1. THE Sentinel_RESPMOD SHALL be implemented as a new c-ICAP RESPMOD service in `polis/services/sentinel/modules/merged/srv_polis_sentinel_resp.c`
2. THE Sentinel_RESPMOD SHALL register with service name `polis_sentinel_resp` and type `ICAP_RESPMOD`
3. THE Sentinel_RESPMOD SHALL implement a ClamAV_Client that connects to clamd via TCP at `scanner:3310`
4. THE ClamAV_Client SHALL implement the INSTREAM_Protocol: send `zINSTREAM\0`, stream body as 4-byte big-endian length-prefixed chunks (16KB), terminate with a zero-length chunk, and read the response line
5. WHEN the clamd response contains `FOUND`, THE Sentinel_RESPMOD SHALL return HTTP 403 to block the response (virus detected)
6. WHEN the clamd response contains `OK` (clean) AND the response host matches the Domain_Allowlist, THE Sentinel_RESPMOD SHALL scan the body for OTT codes
7. WHEN the clamd response contains `OK` AND the response host does NOT match the Domain_Allowlist, THE Sentinel_RESPMOD SHALL pass the response through unmodified
8. WHEN an OTT code is found in the response body, THE Sentinel_RESPMOD SHALL execute the 8-step approval flow: GET OTT mapping, check time-gate, check context binding, check blocked key exists, preserve audit data, DEL blocked + SETEX approved, ZADD audit log, DEL OTT
9. WHEN the time-gate has NOT elapsed for a detected OTT, THE Sentinel_RESPMOD SHALL ignore the OTT (echo self-approval prevention)
10. WHEN the origin_host in the OTT mapping does NOT match the response host, THE Sentinel_RESPMOD SHALL reject the OTT (cross-channel replay prevention)
11. THE Sentinel_RESPMOD SHALL strip processed OTT codes from the response body before forwarding (replace with asterisks)
12. THE Sentinel_RESPMOD SHALL handle gzip-compressed response bodies by decompressing before scan and recompressing after modification
13. THE Sentinel_RESPMOD SHALL maintain one Valkey connection as `governance-respmod` for OTT lookup and approval writes
14. IF the clamd TCP connection fails or times out, THEN THE Sentinel_RESPMOD SHALL return HTTP 403 (fail-closed behavior)
15. THE Sentinel_RESPMOD SHALL scan ALL responses with ClamAV regardless of Domain_Allowlist membership — the allowlist only gates OTT scanning

### Requirement 3: Configuration Updates

**User Story:** As a developer, I want the proxy configuration updated to route RESPMOD traffic through the new merged module while keeping REQMOD unchanged, so that both DLP+approval and ClamAV+OTT scanning are active simultaneously.

#### Acceptance Criteria

1. THE g3proxy REQMOD URL SHALL remain unchanged: `icap://sentinel:1344/credcheck`
2. THE g3proxy RESPMOD URL SHALL change to `icap://sentinel:1344/sentinel_respmod`
3. THE c-icap configuration SHALL register the new RESPMOD module with `Service polis_sentinel_resp` and alias `ServiceAlias sentinel_respmod`
4. THE c-icap configuration SHALL retain all existing module registrations (squidclamav, approval modules) for rollback
5. THE g3proxy configuration SHALL contain the old squidclamav URL as a comment for rollback reference

### Requirement 4: Build System Updates

**User Story:** As a developer, I want the Dockerfile updated to compile the new RESPMOD module, so that the sentinel container includes the unified RESPMOD service.

#### Acceptance Criteria

1. THE Dockerfile SHALL compile `srv_polis_sentinel_resp.c` with flags `-lhiredis -lhiredis_ssl -lssl -lcrypto -lpthread -lz` and produce `srv_polis_sentinel_resp.so`
2. THE Dockerfile SHALL copy the new `.so` file to `/usr/lib/c_icap/` in the runtime image
3. THE existing DLP module compilation line SHALL remain unchanged (the extended source file compiles with the same flags)
4. THE Dockerfile SHALL retain compilation of the original approval modules for rollback capability
5. THE Dockerfile SHALL use the same compiler flags (`-shared -fPIC -Wall -Werror`) and include paths as the existing module builds

### Requirement 5: Backward Compatibility and Rollback

**User Story:** As a security engineer, I want the original modules preserved alongside the changes, so that I can roll back if issues arise.

#### Acceptance Criteria

1. THE original `srv_polis_approval_rewrite.c` and `srv_polis_approval.c` SHALL remain in their current locations unmodified
2. THE original compiled `.so` files SHALL remain in the runtime image at `/usr/lib/c_icap/`
3. THE c-icap configuration SHALL contain all original service registrations
4. THE g3proxy configuration SHALL contain the old squidclamav RESPMOD URL as a comment
5. THE Valkey ACL SHALL retain all existing users (`dlp-reader`, `governance-reqmod`, `governance-respmod`) with unchanged permissions
6. squidclamav.so SHALL remain loaded in c-icap for potential direct use

### Requirement 6: End-to-End Telegram Approval Flow

**User Story:** As a user, I want to approve blocked requests from Telegram without running any CLI commands, so that the approval flow is seamless.

#### Acceptance Criteria

1. WHEN an agent sends a request to a new domain and the Security_Policy blocks it, THE DLP module SHALL return 403 with `X-polis-Block` headers and the block reason
2. WHEN the agent sends a `/polis-approve req-{hex8}` message through the proxy to Telegram, THE DLP module SHALL rewrite the request_id to an OTT code in the same REQMOD pass
3. WHEN a user types the OTT code in Telegram and the response passes through the proxy, THE Sentinel_RESPMOD SHALL first scan the response with ClamAV
4. WHEN the ClamAV scan is clean AND the Telegram response contains the OTT code AND the time-gate has elapsed AND context binding matches, THE Sentinel_RESPMOD SHALL write the approval to Valkey
5. WHEN the approval is written, THE agent SHALL be able to retry the previously blocked request and have it succeed
6. THE entire flow SHALL work without the user running any CLI commands

## Notes

- Branch: `feature/add-value-based-exceptions`
- The DLP module is modified in-place — no new REQMOD module or directory
- The new RESPMOD module lives in `polis/services/sentinel/modules/merged/`
- Docker compose commands need both files: `docker compose -f docker-compose.yml -f agents/openclaw/compose.override.yaml ...`
- After restarting polis-gate, MUST also restart gate-init for TPROXY routing
- The clamd INSTREAM protocol chunk size should be 16KB to match squidclamav's behavior
- While doing any edits to source files, split all edits into parts no greater than 50 lines
