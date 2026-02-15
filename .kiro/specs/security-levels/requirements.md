# Requirements Document

## Introduction

This feature implements security level configuration (`relaxed`, `balanced`, `strict`) that controls how the Molis DLP module handles credentials, new domains, and malware. It also implements protected path restrictions in the workspace container. The DLP module reads the active security level from Valkey in real-time using hiredis, so that CLI changes (`molis-approve set-security-level`) take effect on live traffic without restarting the ICAP service.

This spec also incorporates required modifications to the already-implemented Valkey state management (issue 07): tightening the `mcp-agent` ACL to remove write access to config keys, and adding a new `dlp-reader` ACL user for the DLP module.

## Glossary

- **Security_Level**: One of three modes (`relaxed`, `balanced`, `strict`) stored in Valkey at `molis:config:security_level`. Controls DLP behavior for new domains.
- **DLP_Module**: The existing c-ICAP REQMOD service (`srv_molis_dlp.so`) from issue 08, extended here with Valkey connectivity and security level logic.
- **Known_Domain**: A domain in the hardcoded known-good list (e.g., `.github.com`, `.amazonaws.com`). Matched using dot-boundary suffix comparison.
- **New_Domain**: Any domain not in the known-good list. Treatment depends on the active Security_Level.
- **Dot_Boundary_Matching**: Domain comparison using leading-dot suffix match. `.github.com` matches `api.github.com` but NOT `evil-github.com` (CWE-346 prevention).
- **Protected_Path**: A sensitive directory in the workspace container (e.g., `~/.ssh`, `~/.aws`) made inaccessible via tmpfs mounts (primary) and chmod 000 (secondary).
- **dlp-reader**: A read-only Valkey ACL user with access only to `molis:config:security_level` via `+GET +PING`. Used by the DLP module for security level polling.
- **Poll_Interval**: The number of requests between Valkey reads for security level changes. Default 100, with exponential backoff on failure (cap: 10000).
- **Fail_Closed_Init**: The DLP module refuses to start if zero credential patterns are loaded from `molis_dlp.conf` (CWE-636 prevention).
- **hiredis**: The C client library for Redis/Valkey, used by the DLP module for TLS+ACL connections to Valkey.

## Requirements

### Requirement 1: DLP Module Valkey Integration

**User Story:** As a security engineer, I want the DLP module to read the security level from Valkey in real-time, so that CLI changes to the security level take effect on live traffic without restarting the ICAP service.

#### Acceptance Criteria

1. THE DLP_Module SHALL connect to Valkey at startup using hiredis with TLS and authenticate as the `dlp-reader` ACL user
2. THE DLP_Module SHALL read the initial security level from `molis:config:security_level` during initialization and set `current_level` accordingly
3. THE DLP_Module SHALL poll Valkey for security level changes every `LEVEL_POLL_INTERVAL` requests (default: 100)
4. WHEN Valkey is unreachable during a poll, THE DLP_Module SHALL keep the last-known `current_level` and SHALL NOT reset to a weaker level
5. WHEN Valkey is unreachable on consecutive polls, THE DLP_Module SHALL double the poll interval (exponential backoff) up to `LEVEL_POLL_MAX` (default: 10000 requests)
6. WHEN a Valkey poll succeeds after a backoff period, THE DLP_Module SHALL reset the poll interval to `LEVEL_POLL_INTERVAL`
7. IF Valkey is unreachable at DLP initialization, THE DLP_Module SHALL log a WARNING and start with `balanced` as the default level. This is non-fatal — DLP still operates, just without dynamic level changes.
8. THE DLP_Module SHALL read the `dlp-reader` password from the Docker secret file at `/run/secrets/valkey_dlp_password` and scrub it from memory after AUTH
9. THE DLP_Module SHALL link against `libhiredis` (`-lhiredis`) in the ICAP Dockerfile build step

### Requirement 2: Security Level Policy Enforcement

**User Story:** As a security engineer, I want the DLP module to enforce different policies based on the active security level, so that the system behavior adapts to the configured risk tolerance.

#### Acceptance Criteria

1. WHEN the security level is `relaxed` AND a new domain is detected, THE DLP_Module SHALL auto-allow the request
2. WHEN the security level is `balanced` AND a new domain is detected, THE DLP_Module SHALL trigger a HITL prompt
3. WHEN the security level is `strict` AND a new domain is detected, THE DLP_Module SHALL block the request
4. WHEN a credential is detected at ANY security level, THE DLP_Module SHALL trigger a HITL prompt (credentials are never auto-allowed)
5. WHEN malware is detected at ANY security level, THE DLP_Module SHALL block the request (malware is always blocked)
6. THE default security level SHALL be `balanced`

### Requirement 3: Dot-Boundary Domain Matching

**User Story:** As a security engineer, I want domain matching to use dot-boundary suffix comparison, so that substring spoofing attacks are prevented (CWE-346).

#### Acceptance Criteria

1. THE `is_new_domain` function SHALL store known domains with a leading dot (e.g., `.github.com`)
2. THE `is_new_domain` function SHALL match domains using case-insensitive suffix comparison with dot boundary
3. WHEN the host is `api.github.com`, THE function SHALL return 0 (known domain) because it ends with `.github.com`
4. WHEN the host is `evil-github.com`, THE function SHALL return 1 (new domain) because `evil-github.com` does NOT end with `.github.com` at a dot boundary
5. WHEN the host is `github.com`, THE function SHALL return 0 (known domain) via exact match against the domain without the leading dot
6. THE function SHALL use `strcasecmp` for case-insensitive comparison

### Requirement 4: Fail-Closed DLP Initialization

**User Story:** As a security engineer, I want the DLP module to refuse to start if no credential patterns are loaded, so that a missing or corrupt config file does not silently disable all DLP protection.

#### Acceptance Criteria

1. AFTER loading `molis_dlp.conf`, IF `pattern_count` is 0, THE DLP_Module SHALL return `CI_ERROR` from `dlp_init_service` to abort startup
2. THE DLP_Module SHALL log a CRITICAL message indicating fail-closed behavior and the CWE-636 reference
3. WHEN `dlp_init_service` returns `CI_ERROR`, c-ICAP SHALL NOT route any REQMOD requests to the DLP module

### Requirement 5: Protected Paths (Workspace)

**User Story:** As a security engineer, I want sensitive directories in the workspace container to be inaccessible, so that credentials and cloud configs cannot be exfiltrated.

#### Acceptance Criteria

1. THE workspace container SHALL mount empty tmpfs volumes with mode 0000 over all 6 protected paths: `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gcloud`, `~/.kube`, `~/.docker`
2. THE `workspace-init.sh` script SHALL set chmod 000 on all protected paths as a secondary defense-in-depth layer
3. THE `workspace-init.sh` script SHALL create decoy directories for paths that don't exist and set chmod 000 on them
4. WHEN a process in the workspace attempts to access any protected path, THE system SHALL return "Permission denied"

### Requirement 6: Molis Configuration File

**User Story:** As a developer, I want a central configuration file for security settings, so that security level, protected paths, and auto-approve rules are documented in one place.

#### Acceptance Criteria

1. THE `config/molis.yaml` file SHALL define the default security level as `balanced`
2. THE `config/molis.yaml` file SHALL list all 6 protected paths
3. THE `config/molis.yaml` file SHALL define auto-approve rules mapping credential patterns to allowed destinations
4. THE `config/molis.yaml` SHALL NOT duplicate credential patterns — a comment SHALL reference `molis_dlp.conf` as the single source of truth

### Requirement 7: Valkey ACL Modifications (Issue 07 Patch)

**User Story:** As a security engineer, I want the mcp-agent Valkey ACL tightened and a new dlp-reader user added, so that the agent cannot escalate privileges by writing config keys and the DLP module has least-privilege Valkey access.

#### Acceptance Criteria

1. THE `mcp-agent` ACL user SHALL be changed from `~molis:config:* +@read +@write +@connection` to `~molis:blocked:* ~molis:approved:* +GET +SETEX +EXISTS +SCAN +PING -@all` (removing config key access and category grants)
2. A new `dlp-reader` ACL user SHALL be added with `~molis:config:security_level +GET +PING -@all`
3. THE ACL changes SHALL be applied in BOTH the ACL config section (`secrets/valkey_users.acl`) AND the secrets generation script (`scripts/generate-valkey-secrets.sh`)
4. THE secrets generation script SHALL generate a password for `dlp-reader` and write it to `secrets/valkey_dlp_password.txt`
5. THE `credentials.env.example` SHALL include the `dlp-reader` credentials

## PS
- When editing files, you must split all your edits into chunks no greater than 50 lines.
- Issue 07 (Valkey state management) is already implemented. The ACL changes in Requirement 7 are patches to the existing deployment.
- The DLP module source (`srv_molis_dlp.c`) already exists from issue 08. This spec extends it with Valkey connectivity and security level logic.
- The ICAP Dockerfile already includes `libhiredis-dev` (build) and `libhiredis0.14` (runtime) from issue 10 (approval system). Only the DLP compile line needs `-lhiredis` added.
- While editing files ensure each edit is at most 50 lines. Split larger edits into smaller chunks