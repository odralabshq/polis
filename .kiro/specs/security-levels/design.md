# Design Document: Security Levels & Protected Paths

## Overview

This design extends the existing DLP module (`srv_molis_dlp.c`) with dynamic security level support via Valkey, implements protected path restrictions in the workspace container, and patches the Valkey ACL configuration (issue 07) to tighten `mcp-agent` permissions and add a `dlp-reader` user.

The implementation consists of six deliverables:

1. **DLP module extension** — add hiredis Valkey connection, `refresh_security_level()`, `is_new_domain()` with dot-boundary matching, `apply_security_policy()`, and fail-closed init check
2. **Valkey ACL patch** — tighten `mcp-agent`, add `dlp-reader` user in both ACL config and secrets generation script
3. **Protected paths** — tmpfs mounts in docker-compose.yml and chmod 000 in workspace-init.sh
4. **Molis config file** — `config/molis.yaml` with security level, protected paths, auto-approve rules
5. **Dockerfile update** — add `-lhiredis` to DLP module compile line
6. **Docker secret** — `valkey_dlp_password.txt` for dlp-reader authentication

## Architecture

```
CLI: molis-approve set-security-level strict
    │
    ▼
Valkey: SET molis:config:security_level "strict"
    │
    ▼ (polled every 100 requests by DLP module)
srv_molis_dlp.c: refresh_security_level()
    │
    ├─ current_level = LEVEL_STRICT
    │
    ▼ (per-request)
apply_security_policy(host, has_credential)
    │
    ├─ is_new_domain(host)?
    │   ├─ YES + STRICT → BLOCK (return 2)
    │   ├─ YES + BALANCED → PROMPT (return 1)
    │   └─ YES + RELAXED → ALLOW (return 0)
    ├─ has_credential? → PROMPT (return 1) at any level
    └─ known domain, no credential → ALLOW (return 0)
```

### Valkey Polling with Exponential Backoff

```
Normal:   poll every 100 requests
Failure:  100 → 200 → 400 → 800 → 1600 → 3200 → 6400 → 10000 (cap)
Recovery: first success → reset to 100
```

### Protected Paths Defense-in-Depth

```
Layer 1 (PRIMARY):   tmpfs mount with mode 0000 — kernel-enforced, cannot be
                     reversed from inside the container
Layer 2 (SECONDARY): chmod 000 in workspace-init.sh — defense-in-depth,
                     reversible with root but adds friction
```

## Components and Interfaces

### Component 1: DLP Module Valkey Integration

**File**: `polis/build/icap/srv_molis_dlp.c` (MODIFIED — extends existing)

New additions to the existing DLP module:

**New includes and globals:**
```c
#include <hiredis/hiredis.h>

static redisContext *valkey_level_ctx = NULL;
static security_level_t current_level = LEVEL_BALANCED;
#define LEVEL_POLL_INTERVAL 100
#define LEVEL_POLL_MAX      10000
static unsigned long request_counter = 0;
static unsigned long current_poll_interval = LEVEL_POLL_INTERVAL;
```

**New functions:**
| Function | Purpose |
|---|---|
| `refresh_security_level()` | GET `molis:config:security_level` from Valkey, update `current_level`. Exponential backoff on failure. |
| `is_new_domain(host)` | Dot-boundary suffix match against known-good domain list. Returns 1 if new, 0 if known. |
| `apply_security_policy(host, has_credential)` | Per-request policy check. Polls Valkey every `current_poll_interval` requests. Returns 0 (allow), 1 (prompt), or 2 (block). |
| `dlp_valkey_init()` | Connect to Valkey as `dlp-reader` with TLS+ACL. Read initial security level. Called from `dlp_init_service()`. |

**Modified function — `dlp_init_service()`:**
After config parsing, add:
1. Fail-closed check: if `pattern_count == 0`, return `CI_ERROR`
2. Call `dlp_valkey_init()` — non-fatal if it fails (DLP works without dynamic levels)

**Valkey connection details:**
- Host: `MOLIS_VALKEY_HOST` env var (default: `valkey`)
- Port: 6379 (TLS)
- User: `dlp-reader`
- Password: read from `/run/secrets/valkey_dlp_password`
- TLS certs: `/etc/valkey/tls/ca.crt`, `client.crt`, `client.key`

### Component 2: Valkey ACL Patch

**Files**: `polis/secrets/valkey_users.acl` (MODIFIED), `polis/scripts/generate-valkey-secrets.sh` (MODIFIED)

**Change 1 — Tighten mcp-agent:**

Before:
```
user mcp-agent on #<hash> ~molis:blocked:* ~molis:approved:* ~molis:config:* +@read +@write +@connection -@admin -@dangerous -DEL -UNLINK
```

After:
```
user mcp-agent on #<hash> ~molis:blocked:* ~molis:approved:* +GET +SETEX +EXISTS +SCAN +PING -@all
```

Rationale: Removes `~molis:config:*` key pattern (prevents privilege escalation — agent cannot write security level or auto-approve rules). Replaces category grants (`+@read +@write`) with explicit command allowlist.

**Change 2 — Add dlp-reader:**
```
user dlp-reader on #<hash> ~molis:config:security_level +GET +PING -@all
```

**Change 3 — Secrets generation script:**
- Add `DLP_PASS=$(generate_password)` 
- Add `dlp-reader` line to ACL heredoc
- Write `echo -n "$DLP_PASS" > "$SECRETS_DIR/valkey_dlp_password.txt"`
- Add `VALKEY_DLP_USER`/`VALKEY_DLP_PASS` to `credentials.env.example`

### Component 3: Protected Paths — Docker Compose

**File**: `polis/deploy/docker-compose.yml` (MODIFIED — workspace service)

Add 6 tmpfs mounts to the workspace service volumes:
```yaml
- type: tmpfs
  target: /root/.ssh
  tmpfs:
    mode: 0000
# ... repeat for .aws, .gnupg, .config/gcloud, .kube, .docker
```

### Component 4: Protected Paths — Workspace Init Script

**File**: `polis/scripts/workspace-init.sh` (MODIFIED)

Add `protect_sensitive_paths()` function:
- Iterate over 6 paths
- chmod 000 existing directories
- Create and chmod 000 missing directories (decoys)

### Component 5: Molis Configuration File

**File**: `polis/config/molis.yaml` (NEW)

Static YAML config defining:
- `security_level: balanced` (default)
- `protected_paths` list (6 paths)
- `auto_approve` rules (credential pattern → destination mapping)
- Comment referencing `molis_dlp.conf` as single source of truth for credential patterns

### Component 6: Dockerfile Update

**File**: `polis/build/icap/Dockerfile` (MODIFIED)

Change DLP compile line from:
```dockerfile
RUN gcc -shared -fPIC -Wall -Werror -o srv_molis_dlp.so srv_molis_dlp.c \
    -I/usr/include/c_icap -licapapi
```
To:
```dockerfile
RUN gcc -shared -fPIC -Wall -Werror -o srv_molis_dlp.so srv_molis_dlp.c \
    -I/usr/include/c_icap -licapapi -lhiredis
```

`libhiredis-dev` (build) and `libhiredis0.14` (runtime) are already present from issue 10.

## Data Models

### Security Level Values

| Valkey Value | Enum | New Domain Behavior |
|---|---|---|
| `"relaxed"` or `"\"relaxed\""` | `LEVEL_RELAXED (0)` | Auto-allow |
| `"balanced"` or `"\"balanced\""` | `LEVEL_BALANCED (1)` | Prompt (HITL) |
| `"strict"` or `"\"strict\""` | `LEVEL_STRICT (2)` | Block |
| `(nil)` / missing / unknown | `LEVEL_BALANCED (1)` | Prompt (default) |

Note: The parser handles both raw strings (`relaxed`) and JSON-quoted strings (`"relaxed"`) because the CLI tool uses `serde_json::to_string()` which adds quotes.

### Known-Good Domain List

| Domain (dot-prefixed) | Matches | Does NOT Match |
|---|---|---|
| `.api.anthropic.com` | `api.anthropic.com` | `evil-api.anthropic.com.attacker.io` |
| `.api.openai.com` | `api.openai.com` | — |
| `.api.github.com` | `api.github.com` | — |
| `.github.com` | `github.com`, `api.github.com` | `evil-github.com` |
| `.amazonaws.com` | `s3.amazonaws.com`, `ec2.amazonaws.com` | `evil-amazonaws.com` |

### File Layout

```
polis/
├── build/icap/
│   ├── Dockerfile              # MODIFIED: add -lhiredis to DLP compile
│   └── srv_molis_dlp.c         # MODIFIED: add Valkey integration + security levels
├── config/
│   └── molis.yaml              # NEW: security configuration
├── deploy/
│   └── docker-compose.yml      # MODIFIED: tmpfs mounts, dlp secret
├── scripts/
│   ├── workspace-init.sh       # MODIFIED: protected paths
│   └── generate-valkey-secrets.sh  # MODIFIED: add dlp-reader
└── secrets/
    ├── valkey_users.acl        # MODIFIED: tighten mcp-agent, add dlp-reader
    ├── valkey_dlp_password.txt # NEW: dlp-reader password
    └── credentials.env.example # MODIFIED: add dlp-reader credentials
```

## Error Handling

### DLP Module Initialization

| Condition | Behavior | Severity |
|---|---|---|
| `molis_dlp.conf` missing or 0 patterns loaded | Return `CI_ERROR` — fail-closed (CWE-636) | FATAL |
| Valkey unreachable at init | Log WARNING, start with `balanced` default | NON-FATAL |
| `dlp-reader` AUTH fails | Log CRITICAL, no dynamic levels | NON-FATAL |
| TLS context creation fails | Log error, no Valkey connection | NON-FATAL |
| Password file missing | Log error, no Valkey connection | NON-FATAL |

### DLP Module Runtime

| Condition | Behavior |
|---|---|
| Valkey unreachable during poll | Keep last-known level, double poll interval |
| Valkey returns unexpected value | Default to `balanced` |
| Valkey poll succeeds after backoff | Reset poll interval to 100 |
| `valkey_level_ctx` is NULL | Skip polling entirely (no Valkey connection) |

### ACL Enforcement

| Condition | Behavior |
|---|---|
| `mcp-agent` attempts `SET molis:config:security_level` | Valkey returns NOPERM error |
| `dlp-reader` attempts `SET` on any key | Valkey returns NOPERM error |
| `dlp-reader` attempts `GET` on `molis:blocked:*` | Valkey returns NOPERM error (wrong key pattern) |

## Correctness Properties

### Property 1: Security level changes propagate to DLP

*For any* security level change via `SET molis:config:security_level`, the DLP module SHALL reflect the new level within `LEVEL_POLL_INTERVAL` requests (default: 100).

**Validates: Requirements 1.3**

### Property 2: Valkey failure never weakens security

*For any* Valkey connectivity failure during polling, the DLP module SHALL keep the last-known `current_level` and SHALL NOT reset to `LEVEL_RELAXED`.

**Validates: Requirements 1.4**

### Property 3: Dot-boundary prevents suffix spoofing

*For any* domain `D` that does not end with a known domain at a dot boundary, `is_new_domain(D)` SHALL return 1 (new). Specifically, `evil-github.com` SHALL NOT match `.github.com`.

**Validates: Requirements 3.2, 3.4**

### Property 4: Fail-closed on missing config

*For any* execution of `dlp_init_service` where `pattern_count == 0` after config loading, the function SHALL return `CI_ERROR` and the DLP module SHALL NOT process any requests.

**Validates: Requirements 4.1**

### Property 5: mcp-agent cannot write config keys

*For any* attempt by the `mcp-agent` Valkey user to execute a write command on `molis:config:*` keys, Valkey SHALL return a NOPERM error.

**Validates: Requirements 7.1**

### Property 6: Protected paths are inaccessible

*For any* of the 6 protected paths, attempting to list or read files from inside the workspace container SHALL return "Permission denied".

**Validates: Requirements 5.1, 5.4**

## Testing Strategy

### Manual Integration Tests

Since the DLP module is a compiled C shared library running inside Docker, testing requires the full stack. Tests are manual verification commands.

**Security level tests:**
1. Start stack → verify default level is `balanced` (or nil)
2. `molis-approve set-security-level strict` → verify DLP blocks new domains
3. Stop Valkey → verify DLP keeps last-known level
4. Restart Valkey → verify DLP reconnects and resumes polling

**Domain matching tests:**
5. `curl` to `api.github.com` → allowed (known domain)
6. `curl` to `evil-github.com` → treated as new domain (dot-boundary)

**Protected path tests:**
7. `ls ~/.ssh` from workspace → Permission denied
8. `ls ~/.aws` from workspace → Permission denied

**ACL tests:**
9. `mcp-agent` user attempts `SET molis:config:security_level` → NOPERM
10. `dlp-reader` user attempts `SET` → NOPERM

### Build Verification

The Dockerfile build serves as a compilation test — if the hiredis integration has errors, `gcc` will fail and the Docker build will abort.
