# Implementation Plan: Valkey State Management

## Overview

Incremental implementation of the Valkey 8 state management service for Molis OSS. Each task builds on the previous, starting with scripts and configuration, then the Docker Compose service, and finally tests. All files are shell scripts or configuration — no compiled language needed.

## Tasks

- [x] 1. Create TLS certificate generation script
  - [x] 1.1 Create `polis/scripts/generate-valkey-certs.sh`
    - Implement CA key (4096-bit RSA) and certificate generation
    - Implement server key (2048-bit RSA) and certificate generation signed by CA
    - Implement client key (2048-bit RSA) and certificate generation signed by CA
    - Accept optional output directory argument (default: `./certs/valkey`)
    - Create output directory if it doesn't exist
    - Remove CSR files after generation
    - Set key files to permission 600, cert files to permission 644
    - Use SHA-256 for signing, 365-day validity
    - CA subject: `/CN=Molis-Valkey-CA/O=OdraLabs`, Server: `/CN=valkey/O=OdraLabs`, Client: `/CN=valkey-client/O=OdraLabs`
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6_

  - [x] 1.2 Write property test for certificate file permissions
    - **Property 6: Certificate file permissions**
    - **Validates: Requirements 4.3**

- [x] 2. Create secrets generation script
  - [x] 2.1 Create `polis/scripts/generate-valkey-secrets.sh`
    - Generate four unique 32-character passwords using `openssl rand -base64 32 | tr -d '/+=' | head -c 32`
    - Create `valkey_password.txt` with healthcheck password
    - Create `valkey_users.acl` with SHA-256 hashed passwords for all four users (default off, mcp-agent, mcp-admin, log-writer, healthcheck)
    - Create `credentials.env.example` with plaintext credentials
    - Set all file permissions to 600
    - Accept optional output directory argument (default: `./secrets`)
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_

  - [x] 2.2 Write property tests for secrets generation
    - **Property 7: Password uniqueness and length**
    - **Property 8: ACL password hash consistency**
    - **Property 9: Secret file permissions**
    - **Validates: Requirements 5.1, 5.3, 5.5**

- [x] 3. Create Valkey configuration file
  - [x] 3.1 Create `polis/config/valkey.conf`
    - TLS section: `tls-port 6379`, `port 0`, cert/key/CA paths at `/etc/valkey/tls/`, `tls-auth-clients yes`
    - Network section: `bind valkey 127.0.0.1`, `protected-mode yes`
    - ACL section: `aclfile /run/secrets/valkey_acl`
    - Dangerous commands: rename FLUSHALL, FLUSHDB, DEBUG, CONFIG, SHUTDOWN, SLAVEOF, REPLICAOF, MODULE, BGSAVE, BGREWRITEAOF, KEYS to `""`
    - Memory section: `maxmemory 256mb`, `maxmemory-policy volatile-lru`
    - Persistence: `appendonly yes`, `appendfsync everysec`, `appendfilename "molis.aof"`, `dir /data`
    - RDB: `save 900 1`, `save 300 10`, `save 60 10000`, `dbfilename molis.rdb`
    - Logging: `loglevel notice`, `logfile ""`
    - Limits: `maxclients 100`, `timeout 300`, `tcp-keepalive 300`, client output buffer limits
    - Multi-threading: `io-threads 4`, `io-threads-do-reads yes`
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8_

- [x] 4. Create health check script
  - [x] 4.1 Create `polis/scripts/valkey-health.sh`
    - Environment variable defaults: VALKEY_HOST=valkey, VALKEY_PORT=6379, password file, TLS cert/key/CA paths, MEMORY_WARN_PERCENT=80
    - Input validation: host regex `^[a-zA-Z0-9._-]+$`, port numeric 1-65535
    - Read password from file, export as REDISCLI_AUTH (not visible in ps)
    - Connectivity check: `valkey-cli --tls ... ping` must return PONG
    - Memory pressure check: parse `info memory`, warn if usage >= threshold
    - AOF check: parse `info persistence`, exit 1 if aof_enabled != 1
    - Exit 0 with "OK" on success, exit 1 with "CRITICAL" on failure
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7_

  - [x] 4.2 Write property test for health check input validation
    - **Property 10: Health check input validation**
    - **Validates: Requirements 6.6**

- [x] 5. Checkpoint - Verify scripts
  - Ensure all three scripts are executable and syntactically valid (bash -n)
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Add Valkey service to Docker Compose
  - [x] 6.1 Add Valkey service block to `polis/deploy/docker-compose.yml`
    - Image: `valkey/valkey:8-alpine`, container_name: `polis-v2-valkey`
    - Command: `sh -c "valkey-server /etc/valkey/valkey.conf"`
    - Network: `gateway-bridge` only (use hyphens, not underscores)
    - Secrets: `valkey_password`, `valkey_acl`
    - Volumes: `../config/valkey.conf:/etc/valkey/valkey.conf:ro`, `../certs/valkey:/etc/valkey/tls:ro`, `valkey-data:/data`
    - Health check: TLS ping via valkey-cli using REDISCLI_AUTH from secrets file
    - Security: `no-new-privileges:true`, `cap_drop: ALL`, `read_only: true`, tmpfs `/tmp:size=10M,mode=1777`
    - Resources: 512M memory limit, 1.0 CPU, 256M reservation
    - Logging: json-file, 50m max-size, 5 max-file
    - Restart: unless-stopped
    - Labels: `service: "molis-valkey"`
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 7.1, 8.1, 8.2, 8.3, 8.4, 8.5_

  - [x] 6.2 Add secrets definitions to `polis/deploy/docker-compose.yml`
    - Add top-level `secrets:` block with `valkey_password` (file: `./secrets/valkey_password.txt`) and `valkey_acl` (file: `./secrets/valkey_users.acl`)
    - _Requirements: 1.3_

  - [x] 6.3 Add valkey-data volume to `polis/deploy/docker-compose.yml`
    - Add `valkey-data` to top-level `volumes:` block with `name: molis-valkey-data`
    - _Requirements: 7.1_

- [x] 7. Update .gitignore for secrets and cert keys
  - [x] 7.1 Update `polis/.gitignore`
    - Add `secrets/` directory
    - Add `certs/valkey/*.key` for private keys
    - _Requirements: 8.6_

- [x] 8. Write Valkey unit tests
  - [x] 8.1 Create `polis/tests/unit/valkey.bats`
    - Container state tests: exists, running, healthy
    - Security tests: no-new-privileges, cap_drop ALL, read-only fs, tmpfs /tmp
    - Network tests: on gateway-bridge, not on internal-bridge or external-bridge, no host ports
    - Secrets tests: /run/secrets/valkey_password exists, /run/secrets/valkey_acl exists
    - Config tests: TLS port 6379 listening, valkey.conf mounted, AOF enabled
    - Resource tests: memory limit 512M, CPU limit 1.0
    - Volume tests: /data directory exists, valkey-data volume mounted
    - _Requirements: 1.1–1.8, 2.1–2.4, 7.1–7.4, 8.1–8.5_

  - [x] 8.2 Create `polis/tests/unit/valkey-properties.bats`
    - **Property 1: Dangerous commands disabled** — iterate over all 11 commands, verify each returns error
    - **Property 2: mcp-agent ACL enforcement** — test denied keys and denied commands (DEL, UNLINK)
    - **Property 3: mcp-admin ACL enforcement** — test denied dangerous commands
    - **Property 4: log-writer ACL enforcement** — test denied commands and denied keys
    - **Property 5: healthcheck ACL enforcement** — test denied commands and key access
    - **Validates: Requirements 2.6, 3.2, 3.3, 3.4, 3.5**

- [x] 9. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties via parameterized loops (bats does not have a native PBT library)
- Unit tests validate specific examples and edge cases
- All edits to files must be split into chunks no greater than 50 lines per the requirements PS section
