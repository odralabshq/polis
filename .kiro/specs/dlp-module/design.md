# Design Document: DLP Module

## Overview

This design adds a custom c-ICAP DLP module (`srv_molis_dlp`) to the existing ICAP container for credential detection. The implementation consists of four deliverables:

1. **DLP module source** at `polis/build/icap/srv_molis_dlp.c` — c-ICAP REQMOD service that scans request bodies for credential patterns
2. **DLP configuration** at `polis/config/molis_dlp.conf` — credential patterns, allow rules, and actions
3. **Dockerfile update** to `polis/build/icap/Dockerfile` — compile DLP module in existing builder stage
4. **Config updates** to `polis/config/c-icap.conf`, `polis/config/g3proxy.yaml`, and `polis/deploy/docker-compose.yml`

The DLP module operates in REQMOD mode, scanning outbound HTTP request bodies for credential patterns. When a credential is detected going to an unexpected destination, the module returns HTTP 403 with diagnostic headers. Credentials going to their expected API (e.g., Anthropic keys to `api.anthropic.com`) pass through.

## Architecture

```
Agent sends HTTP request
    │
    ▼
g3proxy (TPROXY :18080)
    │
    ▼ REQMOD
c-ICAP :1344 → srv_molis_dlp (credcheck)
    │
    ├─ No credential found → 204 (pass through)
    ├─ Credential to expected destination → 204 (pass through)
    ├─ Credential to unexpected destination → 403 + X-Molis headers
    └─ Private key detected → 403 (always block)
    │
    ▼ RESPMOD
c-ICAP :1344 → squidclamav (malware scan)
    │
    ▼
Upstream server
```

### Integration with Existing ICAP Container

The ICAP container already builds c-ICAP 0.6.4 from source and SquidClamav 7.3. The DLP module is compiled in the same builder stage using the c-ICAP headers produced by the source build. This avoids version mismatches between the module and the server.

**Current REQMOD flow:** `g3proxy → icap:1344/echo` (passthrough)
**New REQMOD flow:** `g3proxy → icap:1344/credcheck` (DLP scan)
**RESPMOD flow (unchanged):** `g3proxy → icap:1344/squidclamav` (malware scan)

## Components and Interfaces

### Component 1: DLP Module Source

**File**: `polis/build/icap/srv_molis_dlp.c`

A c-ICAP REQMOD service module that:
- Loads credential patterns from `/etc/c-icap/molis_dlp.conf` at initialization
- Extracts the `Host` header from each REQMOD request
- Accumulates the request body (up to 1MB) via the preview + IO callbacks, plus a 10KB tail buffer for bodies exceeding 1MB
- Scans the body against all loaded patterns using POSIX `regex.h`
- For each match, checks if the Host matches the pattern's allow rule
- If blocked: returns 403 with `X-Molis-Block`, `X-Molis-Reason`, `X-Molis-Pattern` headers
- If allowed: returns 204 (no modification)

**Data structures**:
```c
#define MAX_PATTERNS 32
#define MAX_BODY_SCAN 1048576  /* 1MB main scan */
#define TAIL_SCAN_SIZE 10240   /* 10KB tail scan for padding bypass prevention */

typedef struct {
    char name[64];           /* Pattern name (e.g., "anthropic") */
    regex_t regex;           /* Compiled credential regex */
    char allow_domain[256];  /* Expected destination regex (empty = always block) */
    int always_block;        /* 1 if no allow rule (private keys) */
} dlp_pattern_t;

typedef struct {
    ci_membuf_t *body;       /* Accumulated request body (first 1MB) */
    char tail[TAIL_SCAN_SIZE]; /* Last 10KB ring buffer for tail scan */
    size_t tail_len;         /* Bytes in tail buffer */
    size_t total_body_len;   /* Total body length seen */
    char host[256];          /* Host header value */
    int blocked;             /* Whether request was blocked */
    char matched_pattern[64]; /* Name of matched pattern */
} dlp_req_data_t;
```

**Service definition**:
- Name: `molis_dlp`
- Type: `ICAP_REQMOD`
- Preview: 4096 bytes
- 204 enabled (no-modification shortcut)

**Config parsing**: The module reads `molis_dlp.conf` line by line at init:
- Lines starting with `pattern.` → compile regex, store in patterns array
- Lines starting with `allow.` → associate domain regex with named pattern
- Lines starting with `action.` → set always_block flag
- Lines starting with `#` or blank → skip

**Blocking response**: When a credential is detected going to an unexpected destination:
```
HTTP/1.1 403 Forbidden
X-Molis-Block: true
X-Molis-Reason: credential_detected
X-Molis-Pattern: <pattern_name>
```

### Component 2: DLP Configuration File

**File**: `polis/config/molis_dlp.conf`

INI-style configuration with three sections:

**Credential patterns** (`pattern.<name> = <regex>`):
| Name | Regex | Description |
|---|---|---|
| `anthropic` | `sk-ant-api[a-zA-Z0-9_-]{20,128}` | Anthropic API keys |
| `openai` | `sk-proj-[a-zA-Z0-9_-]{20,128}` | OpenAI API keys |
| `github_pat` | `ghp_[a-zA-Z0-9]{36}` | GitHub PATs |
| `github_oauth` | `gho_[a-zA-Z0-9]{36}` | GitHub OAuth tokens |
| `aws_access` | `AKIA[A-Z0-9]{16}` | AWS access key IDs |
| `aws_secret` | `[a-zA-Z0-9/+=]{40}` | AWS secret keys |
| `rsa_key` | `-----BEGIN RSA PRIVATE KEY-----` | RSA private keys |
| `openssh_key` | `-----BEGIN OPENSSH PRIVATE KEY-----` | OpenSSH private keys |
| `ec_key` | `-----BEGIN EC PRIVATE KEY-----` | EC private keys |

**Allow rules** (`allow.<name> = <domain_regex>`):
| Pattern | Allowed Domain |
|---|---|
| `anthropic` | `^api\.anthropic\.com$` |
| `openai` | `^api\.openai\.com$` |
| `github_pat` | `^(api\.)?github\.com$` |
| `github_oauth` | `^(api\.)?github\.com$` |
| `aws_access` | `^[a-z0-9-]+\.amazonaws\.com$` |
| `aws_secret` | `^[a-z0-9-]+\.amazonaws\.com$` |

**Actions** (`action.<name> = block`):
| Pattern | Action |
|---|---|
| `rsa_key` | Always block |
| `openssh_key` | Always block |
| `ec_key` | Always block |

### Component 3: Dockerfile Update

**File**: `polis/build/icap/Dockerfile`

Add DLP module compilation to the existing builder stage, after c-ICAP is built from source (so headers are available).

**Builder stage addition** (after SquidClamav build):
```dockerfile
# Copy and compile DLP module against c-ICAP source headers
COPY build/icap/srv_molis_dlp.c /build/
RUN gcc -shared -fPIC -Wall -Werror -o /build/srv_molis_dlp.so /build/srv_molis_dlp.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/lib -licapapi
```

**Runtime stage addition** (after existing COPY):
```dockerfile
COPY --from=builder /build/srv_molis_dlp.so /usr/lib/c_icap/
```

Key: The `-I` flags point to the c-ICAP source tree already extracted in the builder, ensuring header compatibility with the compiled server.

### Component 4: c-ICAP Configuration Update

**File**: `polis/config/c-icap.conf`

Add DLP module loading after the existing SquidClamav service:

```ini
# DLP module for credential detection (REQMOD)
Service molis_dlp srv_molis_dlp.so
ServiceAlias credcheck molis_dlp

# DLP configuration
Include /etc/c-icap/molis_dlp.conf
```

The echo service remains for testing/debugging but is no longer the REQMOD target.

### Component 5: g3proxy Configuration Update

**File**: `polis/config/g3proxy.yaml`

Change REQMOD routing from echo to credcheck:

```yaml
# REQMOD: DLP credential scanning (replaces echo passthrough)
icap_reqmod_service:
  url: icap://icap:1344/credcheck
  no_preview: true
```

The `no_preview` setting avoids ICAP preview failures on requests with small or empty bodies (same pattern used for RESPMOD/squidclamav).

### Component 6: Docker Compose Update

**File**: `polis/deploy/docker-compose.yml`

Add DLP config volume mount to the ICAP service:

```yaml
volumes:
  - ../config/c-icap.conf:/etc/c-icap/c-icap.conf:ro
  - ../config/squidclamav.conf:/etc/squidclamav.conf:ro
  - ../config/molis_dlp.conf:/etc/c-icap/molis_dlp.conf:ro  # NEW
```

## Data Models

### Pattern Matching Flow

```
1. Accumulate body: first 1MB into ci_membuf, keep rolling 10KB tail buffer
2. On process:
   a. Scan the first 1MB buffer against all patterns
   b. If body exceeded 1MB, also scan the 10KB tail buffer
   c. If either scan matches:
      - If P.always_block → BLOCK
      - If P.allow_domain set and Host matches → ALLOW
      - If P.allow_domain set and Host doesn't match → BLOCK
      - If no allow_domain → BLOCK (default)
   d. If no matches in either buffer → ALLOW (204)
```

### File Layout

```
polis/
├── build/
│   └── icap/
│       ├── Dockerfile           # MODIFIED: Add DLP compilation step
│       └── srv_molis_dlp.c      # NEW: DLP module source
├── config/
│   ├── c-icap.conf              # MODIFIED: Load DLP module
│   ├── g3proxy.yaml             # MODIFIED: REQMOD → credcheck
│   ├── molis_dlp.conf           # NEW: DLP patterns and rules
│   └── squidclamav.conf         # UNCHANGED
└── deploy/
    └── docker-compose.yml       # MODIFIED: Mount molis_dlp.conf
```

## Error Handling

### Module Initialization

| Condition | Behavior |
|---|---|
| Config file missing | Module logs error, starts with zero patterns (all requests pass) |
| Regex compilation fails | Pattern skipped, error logged, other patterns still active |
| Too many patterns (>32) | Extra patterns ignored, warning logged |

### Runtime

| Condition | Behavior |
|---|---|
| Body exceeds 1MB | First 1MB scanned + last 10KB tail scanned; middle passes through |
| Host header missing | Request passes through (no destination to check) |
| Module crash | c-ICAP restarts the service; g3proxy retries or passes through |
| Memory allocation failure | Request passes through (fail-open for availability) |

### Security Note on Fail-Open

The DLP module is defense-in-depth. If it fails, traffic passes through. This is intentional — the primary security layer is network isolation (the agent can only reach the internet through the proxy). DLP adds credential-specific protection on top.

### Known Bypass Vectors (Documented, Accepted)

These are inherent limitations of regex-based DLP and are mitigated by other layers:

1. **Encoding bypass**: Base64, URL-encoding, or JSON-escaping credentials will evade regex matching. Mitigation: the domain allowlist in g3proxy restricts where data can go regardless of encoding.
2. **Credential splitting**: Splitting a credential across multiple HTTP requests. Mitigation: API keys are useless when split; the destination must reassemble them.
3. **Novel credential formats**: New API key formats not in the pattern list. Mitigation: patterns are in a config file and can be updated without recompilation.

These are accepted risks for MVP. A canonicalization/decoding layer (Base64, URL-decode before scanning) is a post-MVP enhancement.

## Testing Strategy

### Manual Integration Tests

Since the DLP module is a compiled C shared library running inside a Docker container, testing requires the full stack running. Tests are manual verification commands run from inside the workspace container.

**Test scenarios**:
1. Credential to wrong destination → 403 blocked
2. Credential to correct destination → allowed
3. Private key to any destination → 403 blocked
4. No credential → passes through
5. Response headers contain X-Molis-Block, X-Molis-Reason, X-Molis-Pattern

### Build Verification

The Dockerfile build itself serves as a compilation test — if `srv_molis_dlp.c` has syntax errors or missing symbols, `gcc` will fail and the Docker build will abort.

## Correctness Properties

### Property 1: Credentials to unexpected destinations are blocked

*For any* request containing a credential matching pattern P, *if* the Host header does not match P's allow rule, *then* the module returns HTTP 403 with X-Molis headers.

**Validates: Requirements 1.4**

### Property 2: Credentials to expected destinations pass through

*For any* request containing a credential matching pattern P, *if* the Host header matches P's allow rule, *then* the module returns 204 (no modification).

**Validates: Requirements 1.5**

### Property 3: Private keys are always blocked

*For any* request containing a private key pattern (RSA, OpenSSH, EC), *regardless* of the Host header value, the module returns HTTP 403.

**Validates: Requirements 1.6**

### Property 4: Credential values are never logged

*For any* blocked request, the module logs only the pattern name (e.g., "anthropic"), never the matched credential string.

**Validates: Requirements 1.7**

### Property 5: Existing RESPMOD is unaffected

*After* the DLP module is added, SquidClamav RESPMOD scanning continues to function for malware detection on HTTP responses.

**Validates: Requirements 4.4, 5.2**
