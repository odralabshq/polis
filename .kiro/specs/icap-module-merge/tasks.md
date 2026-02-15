# Implementation Plan: ICAP Module Merge

## Overview

Extend the existing DLP REQMOD module (`srv_polis_dlp.c`) with OTT approval rewriting, create a new unified RESPMOD module (`srv_polis_sentinel_resp.c`) combining ClamAV + OTT scanning, and update configuration files. All edits to source files are split into parts no greater than 50 lines. The implementation language is C.

## Tasks

- [ ] 1. Extend DLP module with OTT rewrite static state and data structures
  - [x] 1.1 Add OTT rewrite static variables to `srv_polis_dlp.c`
    - Add `approve_pattern` (regex_t), `time_gate_secs`, `ott_ttl_secs`, `valkey_gov_ctx`, `gov_valkey_mutex` static declarations
    - Add after the existing static Valkey/DLP state block
    - _Requirements: 1.1, 1.9, 1.12_

  - [x] 1.2 Add OTT fields to `dlp_req_data_t` struct in `srv_polis_dlp.c`
    - Add `int ott_rewritten` and `size_t ott_body_sent` fields to the existing struct
    - _Requirements: 1.7, 1.8_

  - [x] 1.3 Add `generate_ott()` function to `srv_polis_dlp.c`
    - Port from `srv_polis_approval_rewrite.c` line 117 — reads `/dev/urandom`, fail-closed (no PRNG fallback)
    - _Requirements: 1.5_

  - [x] 1.4 Add `gov_valkey_init()` and `ensure_gov_valkey_connected()` functions to `srv_polis_dlp.c`
    - Port Valkey TLS connection logic for `governance-reqmod` user, reads `/run/secrets/valkey_reqmod_password`
    - Separate from existing `dlp-reader` connection, uses `gov_valkey_mutex`
    - _Requirements: 1.9, 1.11_

- [ ] 2. Extend DLP module callbacks with OTT rewrite logic
  - [x] 2.1 Extend `dlp_init_service()` with OTT initialization
    - Compile approve regex `/polis-approve[[:space:]]+(req-[a-f0-9]{8})`
    - Load `POLIS_APPROVAL_TIME_GATE_SECS` env var (default 15)
    - Call `gov_valkey_init()` for the governance-reqmod connection
    - Add after existing DLP init logic
    - _Requirements: 1.3, 1.9, 1.12_

  - [x] 2.2 Extend `dlp_process()` with OTT rewrite pass
    - After the existing `if (data->blocked == 1) { return CI_MOD_DONE; }` block
    - Scan membuf for approve pattern, validate request_id format (CWE-116)
    - Check `polis:blocked:{request_id}` exists in Valkey, check Host header present
    - Generate OTT via `generate_ott()`, store with `SET NX EX` (JSON payload with ott_code, request_id, armed_after, origin_host)
    - Perform length-preserving substitution in membuf, set `data->ott_rewritten = 1`
    - Log OTT rewrite to `polis:log:events` via ZADD
    - _Requirements: 1.3, 1.4, 1.5, 1.6, 1.7, 1.10_

  - [x] 2.3 Extend `dlp_io()` with OTT rewrite streaming path
    - Add branch: when `data->ott_rewritten && data->body`, stream from modified membuf instead of cached file
    - Track bytes sent via `data->ott_body_sent`, return `CI_EOF` when complete
    - Insert before the existing cached file streaming path
    - _Requirements: 1.8_

  - [x] 2.4 Extend `dlp_close_service()` with OTT cleanup
    - Add `regfree(&approve_pattern)`
    - Free `valkey_gov_ctx` under `gov_valkey_mutex`, destroy mutex
    - _Requirements: 1.1_

  - [x] 2.5 Initialize OTT fields in `dlp_check_preview()` or request data init
    - Set `ott_rewritten = 0` and `ott_body_sent = 0` in per-request data initialization
    - _Requirements: 1.1_

- [ ] 3. Checkpoint — DLP module compiles and existing behavior preserved
  - Build the sentinel container: `docker compose -f docker-compose.yml -f agents/openclaw/compose.override.yaml build sentinel`
  - Verify compilation succeeds with no warnings (`-Wall -Werror`)
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Create new Sentinel RESPMOD module — scaffolding and service registration
  - [x] 4.1 Create directory `polis/services/sentinel/modules/merged/` and scaffold `srv_polis_sentinel_resp.c` with includes, defines, and forward declarations
    - Include c-icap headers, hiredis, zlib, regex, pthread, sys/socket, netinet/in, arpa/inet
    - Define `CLAMD_CHUNK_SIZE` (16384), `CLAMD_TIMEOUT_SECS` (30), `CLAMD_MAX_RESPONSE` (1024), `MAX_BODY_SIZE` (2MB)
    - Forward-declare all callback functions
    - _Requirements: 2.1_

  - [x] 4.2 Add `sentinel_resp_data_t` struct and static state to `srv_polis_sentinel_resp.c`
    - Per-request struct: `body`, `cached`, `total_body_len`, `host`, `is_gzip`, `eof`, `virus_found`, `virus_name`, `ott_found`, `error_page`, `error_page_sent`
    - Static state: `ott_regex`, domain allowlist array, clamd host/port, Valkey connection + mutex
    - _Requirements: 2.1, 2.13_

  - [x] 4.3 Add `CI_DECLARE_MOD_DATA` service registration struct
    - Service name `polis_sentinel_resp`, type `ICAP_RESPMOD`
    - Wire all callback function pointers
    - _Requirements: 2.2_

- [ ] 5. Implement RESPMOD module — init, preview, and request data lifecycle
  - [x] 5.1 Implement `sentinel_resp_init_service()`
    - Compile OTT regex `ott-[a-zA-Z0-9]{8}`
    - Load `POLIS_APPROVAL_DOMAINS` env (default `.api.telegram.org`)
    - Load `POLIS_CLAMD_HOST` / `POLIS_CLAMD_PORT` env vars
    - Connect to Valkey as `governance-respmod` via TLS
    - _Requirements: 2.1, 2.3, 2.13_

  - [x] 5.2 Implement `sentinel_resp_close_service()`
    - Free regex, Valkey connection, destroy mutex
    - _Requirements: 2.1_

  - [x] 5.3 Implement `sentinel_resp_init_request_data()` and `sentinel_resp_release_request_data()`
    - Allocate and zero-init `sentinel_resp_data_t`
    - Release: free membuf, cached file, error page
    - _Requirements: 2.1_

  - [x] 5.4 Implement `sentinel_resp_check_preview()`
    - Extract Host header from response headers
    - Detect `Content-Encoding: gzip` flag
    - Return `CI_MOD_CONTINUE` to receive full body
    - _Requirements: 2.1, 2.12_

- [ ] 6. Implement RESPMOD module — ClamAV INSTREAM client
  - [x] 6.1 Implement `clamd_scan_buffer()` function
    - Connect TCP to clamd host:port with timeout
    - Send `zINSTREAM\0`, stream body as 4-byte big-endian length-prefixed 16KB chunks
    - Send zero-length terminator, read response line
    - Return 0 (clean), 1 (FOUND), -1 (error)
    - Close socket on all paths
    - _Requirements: 2.3, 2.4_

  - [x] 6.2 Implement `ensure_valkey_connected()` for governance-respmod
    - Lazy reconnect with TLS, reads `/run/secrets/valkey_respmod_password`
    - Thread-safe with mutex
    - _Requirements: 2.13_

  - [x] 6.3 Implement `is_allowed_domain()` function
    - Port from `srv_polis_approval.c` — dot-boundary domain matching against allowlist
    - _Requirements: 2.6, 2.7, 2.15_

- [ ] 7. Implement RESPMOD module — OTT approval flow and body processing
  - [x] 7.1 Implement `process_ott_approval()` — the 8-step approval flow
    - Port from `srv_polis_approval.c` line 537
    - Steps: GET OTT mapping → check time-gate → check context binding → check blocked key → preserve audit → DEL blocked + SETEX approved → ZADD audit log → DEL OTT
    - _Requirements: 2.8, 2.9, 2.10_

  - [x] 7.2 Implement gzip decompression and recompression helpers
    - `decompress_gzip()` — inflate gzip body into plain text buffer
    - `compress_gzip()` — deflate plain text back to gzip
    - Use zlib `inflateInit2` / `deflateInit2` with gzip window bits
    - _Requirements: 2.12_

  - [x] 7.3 Implement `sentinel_resp_io()` — body accumulation and write-back
    - Read: accumulate body chunks into `ci_membuf_t` (up to MAX_BODY_SIZE)
    - Write: after processing, stream from modified body or cached file
    - Handle error page streaming for virus blocks
    - Set `eof` flag when read returns `CI_EOF`
    - _Requirements: 2.1, 2.11_

  - [x] 7.4 Implement `sentinel_resp_process()` — main processing pipeline
    - Call `clamd_scan_buffer()` on accumulated body — if FOUND or error, return 403 (fail-closed)
    - If clean AND host in allowlist: decompress if gzip, scan for OTT regex, call `process_ott_approval()` for each match
    - Strip OTT codes (replace with asterisks), recompress if was gzip
    - If clean AND host NOT in allowlist: pass through
    - _Requirements: 2.5, 2.6, 2.7, 2.8, 2.11, 2.14, 2.15_

- [ ] 8. Checkpoint — RESPMOD module compiles
  - Build the sentinel container: `docker compose -f docker-compose.yml -f agents/openclaw/compose.override.yaml build sentinel`
  - Verify both modules compile with no warnings (`-Wall -Werror`)
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 9. Update build system — Dockerfile
  - [x] 9.1 Add RESPMOD module compilation to Dockerfile
    - Add `COPY services/sentinel/modules/merged/srv_polis_sentinel_resp.c /build/`
    - Add `RUN gcc -shared -fPIC -Wall -Werror` with flags `-lhiredis -lhiredis_ssl -lssl -lcrypto -lpthread -lz`
    - Place after existing approval module compilation
    - _Requirements: 4.1, 4.5_

  - [x] 9.2 Add RESPMOD `.so` copy to runtime image in Dockerfile
    - Add `COPY --from=builder /build/srv_polis_sentinel_resp.so /usr/lib/c_icap/`
    - Place after existing approval module copies
    - _Requirements: 4.2_

- [ ] 10. Update configuration files
  - [x] 10.1 Update `c-icap.conf` — register new RESPMOD module
    - Add `Service polis_sentinel_resp /usr/lib/c_icap/srv_polis_sentinel_resp.so`
    - Add `ServiceAlias sentinel_respmod polis_sentinel_resp?allow204=on&allow206=on`
    - Keep all existing module registrations (squidclamav, approval modules) for rollback
    - _Requirements: 3.3, 3.4_

  - [x] 10.2 Update `g3proxy.yaml` — switch RESPMOD URL
    - Change `icap_respmod_service` URL to `icap://sentinel:1344/sentinel_respmod`
    - Comment out old squidclamav URL for rollback reference
    - Keep REQMOD URL unchanged (`icap://sentinel:1344/credcheck`)
    - _Requirements: 3.1, 3.2, 3.5_

- [x] 11. Final checkpoint — full build and configuration verification
  - Build sentinel: `docker compose -f docker-compose.yml -f agents/openclaw/compose.override.yaml build sentinel`
  - Verify both `.so` files are present in the image at `/usr/lib/c_icap/`
  - Verify c-icap.conf has all service registrations
  - Verify g3proxy.yaml has correct REQMOD and RESPMOD URLs
  - Verify original approval modules are preserved (rollback capability)
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- All source file edits are split into parts no greater than 50 lines
- The DLP module is modified in-place — no new REQMOD module or directory
- The new RESPMOD module lives in `polis/services/sentinel/modules/merged/`
- Docker compose commands need both files: `docker compose -f docker-compose.yml -f agents/openclaw/compose.override.yaml ...`
- After restarting polis-gate, MUST also restart gate-init for TPROXY routing
- Original approval modules (`srv_polis_approval_rewrite.c`, `srv_polis_approval.c`) remain unmodified for rollback
- Valkey ACL users (`dlp-reader`, `governance-reqmod`, `governance-respmod`) are already configured — no changes needed
