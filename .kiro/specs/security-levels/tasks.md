# Implementation Plan: Security Levels & Protected Paths

## Overview

Incremental implementation of security level support in the DLP module, protected path restrictions, and Valkey ACL patches. Tasks are ordered so that infrastructure changes (ACL, secrets, Docker) come first, then the DLP module code changes, then workspace protection. Issue 07 (Valkey) is already deployed — ACL changes are patches to the running system.

## Tasks

- [x] 1. Patch Valkey ACL configuration (Issue 07 — already deployed)
  - [x] 1.1 Update `polis/secrets/valkey_users.acl`
    - Change `mcp-agent` line from `~molis:blocked:* ~molis:approved:* ~molis:config:* +@read +@write +@connection -@admin -@dangerous -DEL -UNLINK` to `~molis:blocked:* ~molis:approved:* +GET +SETEX +EXISTS +SCAN +PING -@all`
    - Add new line: `user dlp-reader on #<SHA256_HASH_DLP_PASSWORD> ~molis:config:security_level +GET +PING -@all`
    - _Requirements: 7.1, 7.2, 7.3_

  - [x] 1.2 Update `polis/scripts/generate-valkey-secrets.sh`
    - Add `DLP_PASS=$(generate_password)` after existing password generation
    - Change `mcp-agent` ACL line in heredoc to match 1.1
    - Add `dlp-reader` ACL line in heredoc
    - Add `echo -n "$DLP_PASS" > "$SECRETS_DIR/valkey_dlp_password.txt"` after ACL file creation
    - Add `VALKEY_DLP_USER=dlp-reader` and `VALKEY_DLP_PASS=${DLP_PASS}` to credentials.env.example heredoc
    - _Requirements: 7.3, 7.4, 7.5_

  - [x] 1.3 Regenerate secrets and restart Valkey
    - Run `./scripts/generate-valkey-secrets.sh` to regenerate all secrets with new ACL
    - Run `docker compose restart valkey` to reload ACL
    - Verify: `mcp-agent` user cannot `SET molis:config:security_level` (expect NOPERM)
    - Verify: `dlp-reader` user can `GET molis:config:security_level` (expect nil or value)
    - Verify: `dlp-reader` user cannot `SET` anything (expect NOPERM)
    - _Requirements: 7.1, 7.2_

- [x] 2. Create Molis configuration file
  - [x] 2.1 Create `polis/config/molis.yaml`
    - Set `security_level: balanced`
    - List all 6 protected paths under `protected_paths`
    - Define auto-approve rules under `auto_approve` (sk-ant→anthropic, ghp→github, private keys→block)
    - Add comment: credential patterns managed in `molis_dlp.conf` (single source of truth)
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

- [x] 3. Add DLP secret to Docker Compose
  - [x] 3.1 Update `polis/deploy/docker-compose.yml`
    - Add `valkey_dlp_password` to top-level `secrets:` block with `file: ./secrets/valkey_dlp_password.txt`
    - Add `valkey_dlp_password` to the ICAP service `secrets:` list
    - _Requirements: 1.8_

- [x] 4. Update Dockerfile — add hiredis to DLP compile
  - [x] 4.1 Update `polis/build/icap/Dockerfile`
    - Find the DLP module gcc line and append `-lhiredis` to the link flags
    - Before: `gcc ... -o srv_molis_dlp.so srv_molis_dlp.c ... -licapapi`
    - After: `gcc ... -o srv_molis_dlp.so srv_molis_dlp.c ... -licapapi -lhiredis`
    - `libhiredis-dev` and `libhiredis0.14` are already present from issue 10
    - _Requirements: 1.9_

- [x] 5. Extend DLP module with security level logic
  - [x] 5.1 Add hiredis include and global variables to `polis/build/icap/srv_molis_dlp.c`
    - Add `#include <hiredis/hiredis.h>` to includes
    - Add `security_level_t` enum (LEVEL_RELAXED=0, LEVEL_BALANCED=1, LEVEL_STRICT=2)
    - Add static globals: `valkey_level_ctx`, `current_level`, `request_counter`, `current_poll_interval`
    - Add constants: `LEVEL_POLL_INTERVAL 100`, `LEVEL_POLL_MAX 10000`
    - _Requirements: 1.1_

  - [x] 5.2 Implement `refresh_security_level()` function
    - If `valkey_level_ctx` is NULL, return immediately
    - Execute `GET molis:config:security_level` via `redisCommand`
    - On failure: keep `current_level`, double `current_poll_interval` (cap at LEVEL_POLL_MAX), log with backoff value
    - On success: reset `current_poll_interval` to LEVEL_POLL_INTERVAL
    - Parse value: handle both `"relaxed"` and `relaxed` (with/without JSON quotes)
    - Unknown values default to LEVEL_BALANCED
    - _Requirements: 1.3, 1.4, 1.5, 1.6_

  - [x] 5.3 Implement `is_new_domain()` function with dot-boundary matching
    - Define known_domains array with leading dots: `.api.anthropic.com`, `.api.openai.com`, `.api.github.com`, `.github.com`, `.amazonaws.com`
    - For each domain: check suffix match using `strcasecmp(host + (hlen - dlen), domain)`
    - Also check exact match without leading dot: `strcasecmp(host, domain + 1)`
    - Return 0 if any match (known), 1 if no match (new)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6_

  - [x] 5.4 Implement `apply_security_policy()` function
    - Increment `request_counter`, poll Valkey when `request_counter % current_poll_interval == 0`
    - Call `is_new_domain(host)` to check domain status
    - Switch on `current_level`: RELAXED (credential→prompt, new→allow), BALANCED (credential or new→prompt), STRICT (credential→prompt, new→block)
    - Return 0 (allow), 1 (prompt), 2 (block)
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [x] 5.5 Implement `dlp_valkey_init()` function
    - Read `MOLIS_VALKEY_HOST` env var (default: `valkey`), port 6379
    - Create TLS context with CA, client cert, client key from `/etc/valkey/tls/`
    - Connect with `redisConnectWithOptions`, initiate TLS
    - Read password from `/run/secrets/valkey_dlp_password`, strip newline
    - AUTH as `dlp-reader`, scrub password from stack with `memset`
    - Call `refresh_security_level()` for initial level read
    - Return 0 on success, -1 on any failure
    - _Requirements: 1.1, 1.2, 1.7, 1.8_

  - [x] 5.6 Update `dlp_init_service()` with fail-closed check and Valkey init
    - After existing config parsing code, add: `if (pattern_count == 0) return CI_ERROR;` with CRITICAL log
    - After pattern check, call `dlp_valkey_init()` — log WARNING on failure but do NOT return CI_ERROR (non-fatal)
    - _Requirements: 4.1, 4.2, 4.3, 1.7_

  - [x] 5.7 Integrate `apply_security_policy()` into `dlp_process()`
    - After credential pattern matching, call `apply_security_policy(data->host, data->blocked)`
    - If result is 2 (block) and not already blocked by credential match, set blocked=1 with reason "new_domain_blocked"
    - If result is 1 (prompt) and not already blocked, trigger HITL prompt mechanism
    - _Requirements: 2.1, 2.2, 2.3_

- [x] 6. Checkpoint — Verify DLP module builds
  - Run `docker compose build icap` from `polis/deploy/`
  - Verify no compilation errors
  - Verify c-ICAP logs show "molis_dlp: Valkey connected" or "molis_dlp: Valkey init failed" (depending on Valkey availability)
  - Verify c-ICAP logs show pattern count > 0

- [x] 7. Add protected path tmpfs mounts
  - [x] 7.1 Update `polis/deploy/docker-compose.yml` workspace service
    - Add 6 tmpfs volume entries under workspace `volumes:` section
    - Each: `type: tmpfs`, `target: /root/<path>`, `tmpfs: mode: 0000`
    - Paths: `.ssh`, `.aws`, `.gnupg`, `.config/gcloud`, `.kube`, `.docker`
    - _Requirements: 5.1_

  - [x] 7.2 Update `polis/scripts/workspace-init.sh`
    - Add `protect_sensitive_paths()` function
    - Iterate over 6 paths, chmod 000 existing directories
    - Create and chmod 000 missing directories (decoys)
    - Call function from main init flow
    - _Requirements: 5.2, 5.3_

- [x] 8. Checkpoint — Verify protected paths
  - Run `docker compose up -d workspace`
  - Verify: `docker exec polis-v2-workspace ls /root/.ssh` → Permission denied
  - Verify: `docker exec polis-v2-workspace ls /root/.aws` → Permission denied
  - Verify: `docker exec polis-v2-workspace ls /root/.gnupg` → Permission denied

- [x] 9. Final verification
  - [x] 9.1 End-to-end security level test
    - Verify default level is `balanced` (or nil → defaults to balanced)
    - Run `molis-approve set-security-level strict`
    - Verify DLP blocks requests to new/unknown domains
    - Run `molis-approve set-security-level relaxed`
    - Verify DLP allows requests to new domains
    - Reset to `balanced`
    - _Requirements: 2.1, 2.2, 2.3, 2.6_

  - [x] 9.2 Verify fail-closed behavior
    - Temporarily rename `molis_dlp.conf` → rebuild ICAP container
    - Verify c-ICAP logs show CRITICAL and DLP module does NOT start
    - Restore config → rebuild → verify DLP starts normally
    - _Requirements: 4.1, 4.2, 4.3_

  - [x] 9.3 Verify ACL enforcement
    - As `mcp-agent`: attempt `SET molis:config:security_level strict` → expect NOPERM
    - As `dlp-reader`: attempt `GET molis:config:security_level` → expect success
    - As `dlp-reader`: attempt `SET molis:config:security_level strict` → expect NOPERM
    - _Requirements: 7.1, 7.2_

## Notes

- Issue 07 (Valkey) is already deployed. Task 1 patches the running system — regenerate secrets and restart Valkey.
- The DLP module source already exists from issue 08. Tasks 5.x extend it, they don't create it from scratch.
- `libhiredis` is already in the ICAP Dockerfile from issue 10 (approval system). Only the DLP compile line needs updating.
- The approval modules (`srv_molis_approval_rewrite.c`, `srv_molis_approval.c`) already demonstrate the hiredis+TLS+ACL pattern — use them as reference.
- All edits to files must be split into chunks no greater than 50 lines per project convention.
