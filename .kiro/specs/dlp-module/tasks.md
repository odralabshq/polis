# Implementation Plan: DLP Module

## Overview

Incremental implementation of the c-ICAP DLP module for credential detection. The module compiles against the c-ICAP 0.6.4 source headers already built in the existing ICAP Dockerfile. Tasks are ordered so the build pipeline works at each step: config first, then source code, then Dockerfile integration, then routing updates.

## Tasks

- [x] 1. Create DLP configuration file
  - [x] 1.1 Create `polis/config/molis_dlp.conf`
    - Add credential patterns section with `pattern.<name> = <regex>` format
    - Patterns: anthropic (`sk-ant-api[a-zA-Z0-9_-]{20,128}`), openai (`sk-proj-[a-zA-Z0-9_-]{20,128}`), github_pat (`ghp_[a-zA-Z0-9]{36}`), github_oauth (`gho_[a-zA-Z0-9]{36}`), aws_access (`AKIA[A-Z0-9]{16}`), aws_secret (`[a-zA-Z0-9/+=]{40}`), rsa_key, openssh_key, ec_key
    - Add allow rules section with `allow.<name> = <domain_regex>` format
    - Allow rules: anthropic→`^api\.anthropic\.com$`, openai→`^api\.openai\.com$`, github_pat/oauth→`^(api\.)?github\.com$`, aws_access/secret→`^[a-z0-9-]+\.amazonaws\.com$`
    - Add actions section: `action.rsa_key = block`, `action.openssh_key = block`, `action.ec_key = block`
    - Add `default_action = block`
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7_

- [x] 2. Create DLP module source code
  - [x] 2.1 Create `polis/build/icap/srv_molis_dlp.c`
    - Implement includes: `c_icap/c-icap.h`, `c_icap/service.h`, `c_icap/header.h`, `c_icap/body.h`, `c_icap/simple_api.h`, `regex.h`, `string.h`, `stdio.h`, `stdlib.h`
    - Define constants: `MAX_PATTERNS 32`, `MAX_PATTERN_LEN 256`, `MAX_BODY_SCAN 1048576`, `TAIL_SCAN_SIZE 10240`
    - Define `dlp_pattern_t` struct: name[64], regex, allow_domain[256], always_block
    - Define `dlp_req_data_t` struct: body (ci_membuf_t*), tail[TAIL_SCAN_SIZE], tail_len, total_body_len, host[256], blocked, matched_pattern[64]
    - Define static pattern array and count
    - _Requirements: 1.1, 1.2_

  - [x] 2.2 Implement config parser in `dlp_init_service`
    - Open and parse `/etc/c-icap/molis_dlp.conf` line by line
    - Parse `pattern.<name> = <regex>` lines: compile regex with `regcomp`, store in patterns array
    - Parse `allow.<name> = <domain_regex>` lines: find matching pattern by name, store allow_domain
    - Parse `action.<name> = block` lines: find matching pattern by name, set always_block=1
    - Skip comment lines (starting with `#`) and blank lines
    - Log errors for failed regex compilation, skip bad patterns
    - Set preview to 4096 bytes, enable 204
    - _Requirements: 1.9, 2.2, 2.3, 2.4_

  - [x] 2.3 Implement service definition and lifecycle functions
    - `CI_DECLARE_MOD_DATA ci_service_module_t service` with name "molis_dlp", type ICAP_REQMOD
    - `dlp_close_service`: free all compiled regexes
    - `dlp_init_request_data`: allocate dlp_req_data_t, create ci_membuf, extract Host header
    - `dlp_release_request_data`: free membuf and struct
    - _Requirements: 1.1, 1.2_

  - [x] 2.4 Implement pattern matching logic (`check_patterns`)
    - Iterate all loaded patterns
    - For each: run `regexec` against body
    - If match + always_block → return blocked with pattern name
    - If match + allow_domain set → compile allow regex, check against host
    - If host doesn't match allow → return blocked
    - If host matches allow → continue to next pattern
    - If no matches → return allowed
    - _Requirements: 1.4, 1.5, 1.6_

  - [x] 2.5 Implement ICAP callbacks (preview, process, io)
    - `dlp_check_preview`: accumulate preview data into body membuf, return CI_MOD_CONTINUE
    - `dlp_io`: accumulate body data up to MAX_BODY_SCAN in membuf; for bytes beyond 1MB, maintain a rolling 10KB tail buffer (overwrite oldest bytes)
    - `dlp_process`: call check_patterns on accumulated body; if body exceeded 1MB, log `DLP_PARTIAL_SCAN` warning with total body size, then also call check_patterns on tail buffer; if blocked, create 403 response with X-Molis-Block, X-Molis-Reason, X-Molis-Pattern headers and return CI_MOD_DONE; if not blocked, return CI_MOD_ALLOW204
    - Log blocked requests with pattern name only (never credential value)
    - _Requirements: 1.3, 1.4, 1.7, 1.8_

- [x] 3. Update ICAP Dockerfile
  - [x] 3.1 Update `polis/build/icap/Dockerfile` builder stage
    - After the SquidClamav build step, add: COPY of `srv_molis_dlp.c` into `/build/`
    - Add gcc compilation step: `gcc -shared -fPIC -Wall -Werror -o /build/srv_molis_dlp.so /build/srv_molis_dlp.c -I/build/c-icap-server-C_ICAP_0.6.4 -I/build/c-icap-server-C_ICAP_0.6.4/include -L/usr/lib -licapapi`
    - _Requirements: 3.1, 3.2, 3.4_

  - [x] 3.2 Update `polis/build/icap/Dockerfile` runtime stage
    - Add: `COPY --from=builder /build/srv_molis_dlp.so /usr/lib/c_icap/`
    - Ensure this comes after the existing `COPY --from=builder /install/usr /usr` line
    - Verify existing SquidClamav and c-ICAP copies are not affected
    - _Requirements: 3.2, 3.3_

- [x] 4. Update c-ICAP configuration
  - [x] 4.1 Update `polis/config/c-icap.conf`
    - Add after existing SquidClamav service line: `Service molis_dlp srv_molis_dlp.so`
    - Add service alias: `ServiceAlias credcheck molis_dlp`
    - Add config include: `Include /etc/c-icap/molis_dlp.conf`
    - Keep existing echo service and SquidClamav service lines
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

- [x] 5. Update g3proxy ICAP routing
  - [x] 5.1 Update `polis/config/g3proxy.yaml`
    - Change `icap_reqmod_service` from `icap://icap:1344/echo` to map format with `url: icap://icap:1344/credcheck` and `no_preview: true`
    - Keep RESPMOD routing to squidclamav unchanged
    - Update comment from "echo (passthrough for requests)" to "DLP credential scanning"
    - _Requirements: 5.1, 5.2, 5.3_

- [x] 6. Update Docker Compose
  - [x] 6.1 Update `polis/deploy/docker-compose.yml` ICAP service
    - Add volume mount: `../config/molis_dlp.conf:/etc/c-icap/molis_dlp.conf:ro`
    - Keep existing volume mounts for c-icap.conf and squidclamav.conf
    - _Requirements: 6.1, 6.2_

- [x] 7. Build verification
  - [x] 7.1 Verify Docker build succeeds
    - Run `docker compose build icap` from `polis/deploy/` directory
    - Verify no compilation errors in DLP module
    - Verify the built image contains `/usr/lib/c_icap/srv_molis_dlp.so`
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

- [x] 8. Checkpoint — Verify full stack
  - Verify `docker compose up` starts all services (gateway, icap, clamav, workspace, valkey)
  - Verify ICAP container is healthy
  - Verify c-ICAP logs show "molis_dlp: Initializing service" and loaded pattern count
  - Verify g3proxy connects to credcheck REQMOD service
  - Ask user to run manual integration tests from workspace container

## Notes

- The DLP module is pure C with no external dependencies beyond c-ICAP and POSIX regex
- The existing ICAP Dockerfile builds c-ICAP from source — the DLP module compiles against those same source headers for ABI compatibility
- g3proxy uses TCP connections to ICAP (icap://icap:1344/), not Unix sockets
- The echo service remains in c-icap.conf for debugging but is no longer the REQMOD target
- All edits to files must be split into chunks no greater than 50 lines per project convention
