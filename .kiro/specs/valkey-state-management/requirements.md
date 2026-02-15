# Requirements Document

## Introduction

This feature adds a Valkey 8 (Redis-compatible) service to the Molis OSS infrastructure for state management. Valkey stores blocked request queues, approval allowlists, auto-approve rules, and security event logs. The service must be hardened for production use with TLS-only connections, per-service ACL authentication, Docker secrets for credential storage, and container security hardening (read-only filesystem, dropped capabilities, resource limits). Valkey sits on the `gateway-bridge` network only and is not externally accessible.

## Glossary

- **Valkey**: An open-source, BSD-licensed, Redis-compatible in-memory data store (Linux Foundation project). Drop-in replacement for Redis.
- **Valkey_Service**: The Docker Compose service running the `valkey/valkey:8-alpine` container within the Molis infrastructure.
- **TLS**: Transport Layer Security. All Valkey connections use mutual TLS (mTLS) with client certificates.
- **ACL**: Access Control List. Valkey's per-user permission system controlling which commands and key patterns each service user can access.
- **Docker_Secrets**: Docker's mechanism for securely passing sensitive data to containers via files mounted at `/run/secrets/`, avoiding environment variable exposure.
- **Cert_Generator**: The shell script (`generate-valkey-certs.sh`) that creates CA, server, and client TLS certificates for Valkey.
- **Secrets_Generator**: The shell script (`generate-valkey-secrets.sh`) that creates passwords, ACL file, and credential references for Valkey users.
- **Health_Check**: The shell script (`valkey-health.sh`) that verifies Valkey connectivity, memory pressure, and AOF persistence status.
- **Blocked_Request**: A JSON-serialized request stored at `molis:blocked:{request_id}` with no TTL, protected from eviction by the `volatile-lru` policy.
- **Approved_Request**: A key stored at `molis:approved:{request_id}` with a 5-minute TTL, representing a temporarily allowlisted request.
- **Event_Log**: A sorted set at `molis:log:events` scored by Unix timestamp, storing JSON security log entries with 24-hour application-level retention.
- **volatile-lru**: A Valkey eviction policy that only evicts keys that have a TTL set, protecting keys without TTL (such as blocked requests) from memory pressure eviction.
- **AOF**: Append-Only File persistence mode with `everysec` fsync, providing durability for Valkey data across restarts.
- **gateway-bridge**: The existing Docker bridge network (`10.30.1.0/24`) used for inter-service communication between gateway-tier services.

## Requirements

### Requirement 1: Valkey Docker Service Definition

**User Story:** As an infrastructure operator, I want a hardened Valkey Docker service added to the compose stack, so that the Molis platform has a secure state management backend.

#### Acceptance Criteria

1. WHEN `docker compose up valkey` is executed, THE Valkey_Service SHALL start using the `valkey/valkey:8-alpine` image with TLS enabled on port 6379
2. THE Valkey_Service SHALL connect exclusively to the `gateway-bridge` network and have no access to `internal-bridge` or `external-bridge` networks
3. THE Valkey_Service SHALL mount Docker_Secrets for `valkey_password` and `valkey_acl` at `/run/secrets/`
4. THE Valkey_Service SHALL run with `no-new-privileges` security option, all capabilities dropped, and a read-only root filesystem
5. THE Valkey_Service SHALL enforce resource limits of 512MB memory and 1.0 CPU with 256MB memory reservation
6. THE Valkey_Service SHALL use `json-file` logging driver with 50MB max size and 5 file rotation
7. THE Valkey_Service SHALL restart automatically using the `unless-stopped` restart policy
8. THE Valkey_Service SHALL disable non-TLS connections by setting `port 0` in the configuration

### Requirement 2: Valkey Configuration

**User Story:** As a security engineer, I want Valkey configured with TLS, memory management, persistence, and dangerous command restrictions, so that the state store is secure and reliable.

#### Acceptance Criteria

1. THE Valkey_Service SHALL require TLS for all client connections and authenticate clients via TLS certificates (`tls-auth-clients yes`)
2. THE Valkey_Service SHALL use ACL file-based authentication loaded from `/run/secrets/valkey_acl` with the default user disabled
3. THE Valkey_Service SHALL set `maxmemory` to 256MB with `volatile-lru` eviction policy
4. THE Valkey_Service SHALL enable AOF persistence with `everysec` fsync and store data in `/data`
5. THE Valkey_Service SHALL enable RDB snapshots at intervals of 900/1, 300/10, and 60/10000 seconds/changes
6. THE Valkey_Service SHALL disable dangerous commands: FLUSHALL, FLUSHDB, DEBUG, CONFIG, SHUTDOWN, SLAVEOF, REPLICAOF, MODULE, BGSAVE, BGREWRITEAOF, and KEYS by renaming them to empty strings
7. THE Valkey_Service SHALL enable multi-threading with 4 IO threads and read offloading (`io-threads-do-reads yes`)
8. THE Valkey_Service SHALL limit maximum clients to 100 with a 300-second idle timeout

### Requirement 3: ACL Per-Service Authentication

**User Story:** As a security engineer, I want per-service Valkey users with minimal permissions, so that each component can only access the keys and commands it needs.

#### Acceptance Criteria

1. THE Valkey_Service SHALL disable the default user to prevent anonymous access
2. WHEN the `mcp-agent` user authenticates, THE Valkey_Service SHALL grant read and write access only to `molis:blocked:*`, `molis:approved:*`, and `molis:config:*` key patterns, excluding DEL and UNLINK commands
3. WHEN the `mcp-admin` user authenticates, THE Valkey_Service SHALL grant full access to the `molis:*` key namespace excluding dangerous commands (FLUSHALL, FLUSHDB, DEBUG, CONFIG, SHUTDOWN)
4. WHEN the `log-writer` user authenticates, THE Valkey_Service SHALL grant access only to `molis:log:events` with ZADD, ZRANGEBYSCORE, ZCARD, and PING commands
5. WHEN the `healthcheck` user authenticates, THE Valkey_Service SHALL grant access only to PING and INFO commands with no key access
6. IF an ACL-unauthorized command is attempted, THEN THE Valkey_Service SHALL reject the command and return an error

### Requirement 4: TLS Certificate Generation

**User Story:** As a developer, I want a script to generate TLS certificates for Valkey, so that I can set up secure connections without manual OpenSSL commands.

#### Acceptance Criteria

1. WHEN the Cert_Generator is executed, THE Cert_Generator SHALL create a CA key (4096-bit RSA), CA certificate, server key (2048-bit RSA), server certificate, client key (2048-bit RSA), and client certificate in the specified output directory
2. THE Cert_Generator SHALL sign server and client certificates with the generated CA using SHA-256
3. THE Cert_Generator SHALL set private key files to permission 600 and certificate files to permission 644
4. THE Cert_Generator SHALL remove CSR files after certificate generation
5. THE Cert_Generator SHALL accept an optional output directory argument defaulting to `./certs/valkey`
6. IF the output directory does not exist, THEN THE Cert_Generator SHALL create it

### Requirement 5: Secrets Generation

**User Story:** As a developer, I want a script to generate Valkey passwords and ACL configuration, so that secrets are created securely and consistently.

#### Acceptance Criteria

1. WHEN the Secrets_Generator is executed, THE Secrets_Generator SHALL generate four unique 32-character passwords using `openssl rand`
2. THE Secrets_Generator SHALL create a `valkey_password.txt` file containing the healthcheck user password
3. THE Secrets_Generator SHALL create a `valkey_users.acl` file with SHA-256 hashed passwords for all four users (mcp-agent, mcp-admin, log-writer, healthcheck)
4. THE Secrets_Generator SHALL create a `credentials.env.example` file containing plaintext credentials for reference
5. THE Secrets_Generator SHALL set file permissions to 600 on all generated secret files
6. THE Secrets_Generator SHALL accept an optional output directory argument defaulting to `./secrets`

### Requirement 6: Health Check Script

**User Story:** As an infrastructure operator, I want a health check script that verifies Valkey connectivity, memory pressure, and persistence status, so that service health is monitored.

#### Acceptance Criteria

1. WHEN the Health_Check is executed, THE Health_Check SHALL connect to Valkey using TLS with client certificates and authenticate using the password from the secrets file
2. THE Health_Check SHALL verify Valkey responds to PING with PONG
3. WHEN memory usage exceeds the configurable warning threshold (default 80%), THE Health_Check SHALL output a WARNING message including the current memory percentage
4. THE Health_Check SHALL verify AOF persistence is enabled and exit with code 1 if AOF is disabled
5. THE Health_Check SHALL read the password from a file (not command-line arguments) and export it via REDISCLI_AUTH so it is not visible in `ps aux` output
6. IF the Valkey host or port environment variables contain invalid values, THEN THE Health_Check SHALL exit with code 1 and a CRITICAL error message
7. THE Health_Check SHALL exit with code 0 and output "OK" when all checks pass

### Requirement 7: Data Persistence and Eviction

**User Story:** As a platform developer, I want blocked requests to survive memory pressure and container restarts, so that pending approval requests are never lost.

#### Acceptance Criteria

1. THE Valkey_Service SHALL persist data to a named Docker volume (`molis-valkey-data`) so that data survives container restarts
2. WHILE the `volatile-lru` eviction policy is active, THE Valkey_Service SHALL evict only keys that have a TTL set, preserving Blocked_Request keys that have no TTL
3. THE Valkey_Service SHALL use AOF with `everysec` fsync to persist state changes within one second of occurrence
4. WHEN the Valkey_Service container restarts, THE Valkey_Service SHALL restore all previously persisted data from the AOF file

### Requirement 8: Container Security Hardening

**User Story:** As a security engineer, I want the Valkey container hardened against privilege escalation and resource abuse, so that a compromised Valkey instance cannot affect the host system.

#### Acceptance Criteria

1. THE Valkey_Service SHALL run with `no-new-privileges:true` security option to prevent privilege escalation
2. THE Valkey_Service SHALL drop all Linux capabilities via `cap_drop: ALL`
3. THE Valkey_Service SHALL use a read-only root filesystem with a tmpfs mount at `/tmp` (10MB, mode 1777)
4. THE Valkey_Service SHALL enforce a 512MB memory limit and 1.0 CPU limit via Docker deploy resource constraints
5. THE Valkey_Service SHALL not expose any ports to the host machine
6. WHEN the password is used for authentication, THE Valkey_Service SHALL ensure the password is not visible in process listings (`ps aux`)

## PS
- When editing files, you must split all your edits into chunks non-greater than 50 lines.