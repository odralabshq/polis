# Requirements Document

## Introduction

This feature adds a custom c-ICAP DLP (Data Loss Prevention) module (`srv_molis_dlp`) to the Molis OSS ICAP container. The module performs regex-based credential detection on outbound HTTP request bodies, blocking credentials sent to unexpected destinations while allowing them to their expected APIs (e.g., Anthropic keys to `api.anthropic.com`). Private keys are always blocked regardless of destination. The module integrates into the existing c-ICAP build pipeline (which already compiles c-ICAP 0.6.4 from source with SquidClamav) and replaces the current REQMOD echo passthrough with DLP scanning.

## Glossary

- **DLP_Module**: The custom c-ICAP service module (`srv_molis_dlp.so`) that scans HTTP request bodies for credential patterns.
- **DLP_Config**: The configuration file (`molis_dlp.conf`) defining credential patterns, expected destination allow rules, and actions.
- **Pattern**: A named regex rule that matches a specific credential format (e.g., `sk-ant-api[a-zA-Z0-9_-]{20,}` for Anthropic API keys).
- **Allow_Rule**: A domain regex associated with a pattern that defines the expected destination. Credentials matching the pattern are only blocked when sent to domains NOT matching the allow rule.
- **Always_Block**: A pattern with no allow rule — matched credentials are blocked regardless of destination (used for private keys).
- **REQMOD**: ICAP Request Modification mode. The DLP module operates in REQMOD, scanning outbound HTTP request bodies before they leave the proxy.
- **RESPMOD**: ICAP Response Modification mode. Not used by the DLP module (SquidClamav handles RESPMOD for malware scanning).
- **c-ICAP**: The ICAP server (version 0.6.4) compiled from source in the existing ICAP container.
- **SquidClamav**: The existing RESPMOD module for ClamAV malware scanning, already built in the ICAP container.
- **X-Molis Headers**: Custom HTTP response headers added when a request is blocked: `X-Molis-Block`, `X-Molis-Reason`, `X-Molis-Pattern`.
- **credcheck**: The c-ICAP service alias for the DLP module, used in g3proxy ICAP routing configuration.

## Requirements

### Requirement 1: DLP Module Source Code

**User Story:** As a security engineer, I want a c-ICAP module that scans HTTP request bodies for credential patterns, so that credentials are not exfiltrated to unauthorized destinations.

#### Acceptance Criteria

1. THE DLP_Module SHALL be implemented as a c-ICAP REQMOD service in `polis/build/icap/srv_molis_dlp.c`
2. THE DLP_Module SHALL compile as a shared library (`srv_molis_dlp.so`) against the c-ICAP 0.6.4 headers already built in the existing ICAP Dockerfile builder stage
3. THE DLP_Module SHALL scan HTTP request bodies up to 1MB for credential patterns defined in DLP_Config. For bodies exceeding 1MB, the module SHALL additionally scan the last 10KB (tail scan) to prevent trivial padding bypass.
4. WHEN a credential pattern matches AND the request Host header does NOT match the pattern's Allow_Rule, THE DLP_Module SHALL return HTTP 403 with `X-Molis-Block: true`, `X-Molis-Reason: credential_detected`, and `X-Molis-Pattern: <pattern_name>` headers
5. WHEN a credential pattern matches AND the request Host header matches the pattern's Allow_Rule, THE DLP_Module SHALL allow the request (return 204 No Modification)
6. WHEN a Pattern is marked Always_Block, THE DLP_Module SHALL block the request regardless of destination
7. THE DLP_Module SHALL log blocked requests with the pattern name but SHALL NOT log the actual credential value
8. WHEN the request body exceeds 1MB, THE DLP_Module SHALL scan the first 1MB plus the last 10KB of the body (tail scan) to prevent trivial padding bypass
9. IF a regex compilation fails during initialization, THE DLP_Module SHALL skip that pattern and log an error
10. THE DLP_Module SHALL be compiled with `-Wall -Werror` flags to enforce strict compiler warnings as errors
11. Credential patterns with unbounded quantifiers SHALL use upper bounds (e.g., `{20,128}` not `{20,}`) to limit regex execution time

### Requirement 2: DLP Configuration File

**User Story:** As a security engineer, I want credential patterns and allow rules defined in a configuration file, so that I can update detection rules without recompiling the module.

#### Acceptance Criteria

1. THE DLP_Config SHALL be created at `polis/config/molis_dlp.conf` and mounted read-only into the ICAP container at `/etc/c-icap/molis_dlp.conf`
2. THE DLP_Config SHALL define credential patterns using the format `pattern.<name> = <regex>`
3. THE DLP_Config SHALL define expected destination allow rules using the format `allow.<pattern_name> = <domain_regex>`
4. THE DLP_Config SHALL define always-block actions using the format `action.<pattern_name> = block`
5. THE DLP_Config SHALL include patterns for: Anthropic API keys, OpenAI API keys, GitHub PATs, GitHub OAuth tokens, AWS access keys, AWS secret keys, RSA private keys, OpenSSH private keys, EC private keys
6. THE DLP_Config SHALL include allow rules mapping each credential type to its expected API domain
7. THE DLP_Config SHALL mark all private key patterns as always-block

### Requirement 3: ICAP Dockerfile Update

**User Story:** As a developer, I want the DLP module compiled and installed in the existing ICAP container image, so that it's available as a c-ICAP service.

#### Acceptance Criteria

1. THE existing builder stage in `polis/build/icap/Dockerfile` SHALL be extended to compile `srv_molis_dlp.c` into `srv_molis_dlp.so` using the c-ICAP headers already built from source
2. THE runtime stage SHALL copy `srv_molis_dlp.so` to the c-ICAP modules directory alongside the existing SquidClamav module
3. THE Dockerfile SHALL NOT break the existing c-ICAP or SquidClamav build
4. THE DLP module SHALL be compiled with `-shared -fPIC -Wall -Werror` flags and linked against `libicapapi`

### Requirement 4: c-ICAP Configuration Update

**User Story:** As a developer, I want the DLP module loaded and configured in c-ICAP, so that it processes REQMOD requests.

#### Acceptance Criteria

1. THE c-ICAP configuration (`polis/config/c-icap.conf`) SHALL load the DLP module and register it as a service
2. THE c-ICAP configuration SHALL define a service alias `credcheck` pointing to the DLP service
3. THE c-ICAP configuration SHALL include the DLP configuration file (`molis_dlp.conf`)
4. THE existing echo service and SquidClamav service SHALL remain configured and functional

### Requirement 5: g3proxy ICAP Routing Update

**User Story:** As a developer, I want g3proxy to route REQMOD traffic to the DLP module instead of the echo passthrough, so that outbound requests are scanned for credentials.

#### Acceptance Criteria

1. THE g3proxy configuration (`polis/config/g3proxy.yaml`) SHALL change `icap_reqmod_service` from `icap://icap:1344/echo` to `icap://icap:1344/credcheck`
2. THE RESPMOD routing to SquidClamav SHALL remain unchanged
3. THE `no_preview` setting SHALL be applied to the REQMOD service to avoid preview failures on requests with small or empty bodies

### Requirement 6: Docker Compose Volume Mount

**User Story:** As a developer, I want the DLP configuration file mounted into the ICAP container, so that the module can load its patterns at startup.

#### Acceptance Criteria

1. THE ICAP service in `polis/deploy/docker-compose.yml` SHALL mount `../config/molis_dlp.conf` to `/etc/c-icap/molis_dlp.conf` as read-only
2. THE existing volume mounts for `c-icap.conf` and `squidclamav.conf` SHALL remain unchanged

## PS
- When editing files, you must split all your edits into chunks no greater than 50 lines.
- The existing ICAP Dockerfile builds c-ICAP 0.6.4 from source (not from apt packages). The DLP module must compile against those same headers.
- The ICAP container uses TCP port 1344 (not Unix sockets). g3proxy connects via `icap://icap:1344/`.
- The ICAP container runs as a dedicated `c-icap` user via gosu privilege dropping.
- While editing files, you must split all edints into chunks of max 50 lines.