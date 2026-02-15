# Polis Test Suite — Complete Rewrite Plan

> **Author:** SDET Lead · **Date:** 2026-02-14 · **Status:** ✅ COMPLETED (2026-02-15)  
> **Scope:** Replace all existing BATS tests with a properly tiered, mockable, deterministic suite.
>
> **Archive Note:** This plan has been fully executed (Issues 00–16). The new test suite is live.
> For current test development guidance, see [TEST-DEVELOPMENT-GUIDE.md](./TEST-DEVELOPMENT-GUIDE.md).

---

## Table of Contents

1. [Audit of Current State](#1-audit-of-current-state)
2. [Design Principles](#2-design-principles)
3. [Tier Definitions](#3-tier-definitions)
4. [Mocking Strategy](#4-mocking-strategy)
5. [Directory Layout](#5-directory-layout)
6. [Helper Library Architecture](#6-helper-library-architecture)
7. [Test Cases — Tier 1: Unit](#7-test-cases--tier-1-unit)
8. [Test Cases — Tier 2: Integration](#8-test-cases--tier-2-integration)
9. [Test Cases — Tier 3: E2E](#9-test-cases--tier-3-e2e)
10. [Test Runner & CI](#10-test-runner--ci)
11. [Migration Plan](#11-migration-plan)

---

## 1. Audit of Current State

### 1.1 Inventory

| Location | Files | Tests (approx) | Actual Tier |
|---|---|---|---|
| `tests/unit/` | 1 | 45 | ✅ True unit (CLI script grep) |
| `tests/integration/` | 5 | 35 | ✅ Integration (needs containers) |
| `tests/e2e/` | 7 | 120 | ✅ E2E (full traffic flow) |
| `services/gate/tests/unit/` | 2 | 50 | ❌ **Mislabeled** — requires running containers |
| `services/gate/tests/integration/` | 4 | 35 | ✅ Integration |
| `services/sentinel/tests/unit/` | 2 | 30 | ❌ **Mislabeled** — requires running containers |
| `services/sentinel/tests/integration/` | 4 | 70 | ✅ Integration |
| `services/scanner/tests/integration/` | 2 | 55 | ✅ Integration |
| `services/state/tests/unit/` | 2 | 60 | ⚠️ Mixed — some true unit, some need containers |
| `services/resolver/tests/unit/` | 1 | 10 | ✅ True unit |
| `services/resolver/tests/integration/` | 1 | 15 | ✅ Integration |
| `services/workspace/tests/unit/` | 1 | 45 | ❌ **Mislabeled** — requires running containers |
| `services/workspace/tests/integration/` | 4 | 12 | ✅ Integration |
| `services/toolbox/tests/unit/` | 1 | 15 | ❌ **Mislabeled** — requires running containers |
| `services/toolbox/tests/integration/` | 1 | 15 | ✅ Integration |
| **Total** | **38** | **~638** | |

### 1.2 Critical Problems

| # | Problem | Impact | Example |
|---|---|---|---|
| P1 | **Wrong tier classification** | Unit tests fail without Docker | `services/gate/tests/unit/gateway.bats` calls `docker exec` |
| P2 | **Massive duplication** | Same assertions in 3+ files | ICAP hardening tested in `sentinel/integration`, `tests/e2e`, `tests/integration` |
| P3 | **No mocking** | All tests hit real containers | Unit tests for scripts can't run in CI without full stack |
| P4 | **Monolithic helpers** | `common.bash` is 300+ lines | Single file mixes assertions, guards, utilities, fixtures |
| P5 | **Uncentralized IPs/networks** | Tests break on subnet change | Container names are centralized in `common.bash`, but IPs, network names, ports, and subnets are hardcoded across test files |
| P6 | **Network-dependent E2E** | Flaky on slow/offline networks | `httpbin.org` calls with insufficient retry/skip logic |
| P7 | **No test isolation** | Tests modify shared state | `relax_security_level` in setup_file affects other tests |
| P8 | **Scattered organization** | Tests in 14+ directories | No clear ownership or grouping by concern |

---

## 2. Design Principles

1. **Strict tier enforcement** — A test file belongs to exactly one tier. The tier determines what external dependencies are allowed.
2. **Group by concern, not by service** — Tests are organized by what they verify (security, networking, config), not which container they touch.
3. **Modular helpers** — Small, focused helper files loaded à la carte. No god-object `common.bash`.
4. **Mock at the boundary** — Unit tests mock `docker`, `curl`, `openssl`. Integration tests use real containers but mock external networks. E2E tests mock nothing.
5. **Deterministic by default** — Every test must produce the same result on every run. Network-dependent tests use explicit skip guards.
6. **Single source of truth** — Container names, IPs, paths defined once in `lib/constants.bash`.
7. **Fail-fast tagging** — Every file has `bats file_tags=` for selective execution.

---

## 3. Tier Definitions

### Tier 1: Unit (tag: `unit`)

| Property | Value |
|---|---|
| **Dependencies** | None. No Docker, no network, no containers. |
| **What it tests** | Script logic, config file syntax, static security policies, Dockerfile analysis |
| **Mocking** | Native mock helpers (function override + call tracking) for external commands (`docker`, `curl`, `openssl`, `nft`) |
| **Speed** | < 30 seconds for entire tier |
| **When to run** | Every commit, pre-push hook, CI on every PR |

### Tier 2: Integration (tag: `integration`)

| Property | Value |
|---|---|
| **Dependencies** | Running Docker containers (started by `setup_suite`) |
| **What it tests** | Container state, inter-service connectivity, security hardening, mounted configs |
| **Mocking** | External network calls mocked/skipped. Internal Docker calls are real. |
| **Speed** | < 3 minutes (excluding container startup) |
| **When to run** | CI on every PR, after `docker compose up` |

### Tier 3: E2E (tag: `e2e`)

| Property | Value |
|---|---|
| **Dependencies** | Full stack + external network access |
| **What it tests** | Traffic flow through entire proxy chain, malware detection, DLP, DNS blocking |
| **Mocking** | Nothing mocked. Real traffic to real endpoints. |
| **Speed** | < 10 minutes |
| **When to run** | CI nightly, pre-release, manual |

---

## 4. Mocking Strategy

### 4.1 Framework Selection

| Tool | Purpose | Used In |
|---|---|---|
| **bats-core** | Test runner | All tiers |
| **bats-assert** | Output assertions | All tiers |
| **bats-support** | Helper utilities | All tiers |
| **bats-file** | File system assertions | Unit, Integration |
| **Native mock helper** (`lib/mocks/mock_helper.bash`) | Command mocking + call tracking | Unit only |
| **Function override** | Bash function stubbing | Unit only |

### 4.2 Mocking Patterns

#### Pattern A: Command Mocking with Native Helper (Unit Tests)

For testing scripts that call `docker`, `curl`, `openssl`, etc.
Uses a zero-dependency mock helper (`lib/mocks/mock_helper.bash`) instead of
external frameworks like `bats-mock` (abandoned since 2018, incompatible with
modern bats-core):

```bash
# lib/mocks/mock_helper.bash — lightweight call-tracking mock
# Security: validates inputs before eval to prevent CWE-78 injection.
_MOCK_CALLS=()
mock_command() {
    local cmd="$1" output="${2:-}" rc="${3:-0}"
    # Guard: cmd must be a valid bash identifier (prevents eval injection)
    [[ "$cmd" =~ ^[a-zA-Z_][a-zA-Z0-9_]*$ ]] || { echo "mock_command: invalid name '$cmd'" >&2; return 1; }
    [[ "$rc" =~ ^[0-9]+$ ]]                    || { echo "mock_command: invalid rc '$rc'" >&2; return 1; }
    # Use heredoc for output to avoid shell metacharacter interpretation
    eval "${cmd}() { _MOCK_CALLS+=(\"\$*\"); cat <<'_MOCK_EOF_'
${output}
_MOCK_EOF_
return ${rc}; }"
    export -f "${cmd}"
}
mock_call_count() { local n=0; for c in "${_MOCK_CALLS[@]}"; do [[ "$c" == "$1"* ]] && ((n++)); done; echo "$n"; }
mock_call_args()  { echo "${_MOCK_CALLS[$1]:-}"; }
mock_reset()      { _MOCK_CALLS=(); }
```

Usage in tests:

```bash
setup() {
    load "../lib/test_helper.bash"
    load "../lib/mocks/mock_helper.bash"
    mock_command "docker" "0" 0  # docker returns "0" with exit code 0
}

@test "init.sh validates certificates exist" {
    run bash "$PROJECT_ROOT/services/gate/scripts/init.sh"
    assert_success
    # Verify docker was called with expected args
    assert_equal "$(mock_call_args 0)" "exec polis-gate test -f /etc/g3proxy/ssl/ca.pem"
}
```

#### Pattern B: Function Override (Unit Tests)

For testing functions within a script:

```bash
setup() {
    # Override external calls before sourcing
    docker() { echo "mock-docker-output"; return 0; }
    export -f docker
    source "$PROJECT_ROOT/services/gate/scripts/init.sh" --source-only
}
```

#### Pattern C: Fixture Files (Unit Tests)

For negative testing with intentionally invalid configs:

```bash
# tests/lib/fixtures/invalid/g3proxy-missing-resolver.yaml

@test "g3proxy config has resolver section" {
    # Test against REAL config (not a fixture copy — avoids drift)
    run grep -q "^resolver:" "$PROJECT_ROOT/services/gate/config/g3proxy.yaml"
    assert_success
}

@test "missing resolver section is detected" {
    run grep -q "^resolver:" "$FIXTURE_DIR/invalid/g3proxy-missing-resolver.yaml"
    assert_failure
}
```

#### Pattern D: Docker Inspect Cache (Integration Tests)

Reduce flakiness by caching container metadata:

```bash
setup_file() {
    # Cache all container inspections once
    export GATE_INSPECT="$(docker inspect polis-gate 2>/dev/null)"
    export SENTINEL_INSPECT="$(docker inspect polis-sentinel 2>/dev/null)"
}

@test "gate: has NET_ADMIN capability" {
    run jq -r '.[0].HostConfig.CapAdd[]' <<< "$GATE_INSPECT"
    assert_output --partial "NET_ADMIN"
}
```


---

## 5. Directory Layout

```
tests/
├── lib/                              # Shared test infrastructure
│   ├── test_helper.bash              # Core: paths, BATS lib loading, constants
│   ├── constants.bash                # Container names, IPs, subnets, ports
│   ├── guards.bash                   # Skip guards (require_container, require_network)
│   ├── assertions/
│   │   ├── container.bash            # assert_container_running, assert_container_healthy
│   │   ├── network.bash              # assert_on_network, assert_can_reach, assert_port_listening
│   │   ├── security.bash             # assert_cap_drop_all, assert_no_new_privs, assert_read_only
│   │   ├── config.bash               # assert_yaml_key, assert_conf_value
│   │   └── process.bash              # assert_process_running, assert_pid_valid
│   ├── mocks/
│   │   ├── mock_helper.bash          # Native command mock + call tracking (zero deps)
│   │   ├── docker_mock.bash          # Docker command mock setup/teardown
│   │   └── network_mock.bash         # curl/nc mock setup/teardown
│   └── fixtures/
│       ├── invalid/                  # Intentionally broken configs for negative testing
│       │   ├── g3proxy-missing-resolver.yaml
│       │   └── valkey-bad-acl.conf
│       └── expected/                 # Expected outputs for comparison
│           └── container-names.bash
│
├── unit/                             # TIER 1 — No containers, no Docker
│   ├── config/
│   │   ├── g3proxy-config.bats       # g3proxy.yaml static validation
│   │   ├── cicap-config.bats         # c-icap.conf static validation
│   │   ├── corefile-config.bats      # CoreDNS Corefile validation
│   │   ├── valkey-config.bats        # valkey.conf static validation
│   │   ├── squidclamav-config.bats   # squidclamav.conf static validation
│   │   ├── compose-config.bats       # docker-compose.yml static validation
│   │   └── polis-yaml-config.bats    # config/polis.yaml validation
│   ├── scripts/
│   │   ├── gate-init.bats            # gate init.sh logic (mocked)
│   │   ├── gate-setup-network.bats   # setup-network.sh logic (mocked)
│   │   ├── gate-health.bats          # health.sh logic (mocked)
│   │   ├── workspace-init.bats       # workspace init.sh logic (mocked)
│   │   ├── scanner-init.bats         # scanner init.sh logic (mocked)
│   │   ├── state-generate-secrets.bats  # generate-secrets.sh (real, no Docker)
│   │   ├── state-generate-certs.bats   # generate-certs.sh (real, no Docker)
│   │   └── state-health.bats         # health.sh input validation (real, no Docker)
│   ├── security/
│   │   ├── seccomp-profiles.bats     # Seccomp JSON validation
│   │   ├── dockerfile-hardening.bats # Dockerfile static analysis
│   │   ├── blocklist-validation.bats # DNS/URL blocklist validation
│   │   └── acl-structure.bats        # Valkey ACL file structure
│   ├── cli/
│   │   ├── polis-script.bats         # polis.sh syntax & function existence
│   │   └── agent-manifests.bats      # agent.yaml validation
│   └── dlp/
│       └── dlp-config.bats           # DLP pattern config validation
│
├── integration/                      # TIER 2 — Requires running containers
│   ├── container/
│   │   ├── lifecycle.bats            # All containers: exists, running, healthy
│   │   ├── images.bats               # Correct images, tags
│   │   ├── resources.bats            # Memory limits, CPU limits, ulimits
│   │   ├── restart-policy.bats       # Restart policies, logging drivers
│   │   └── dependencies.bats         # Start order, depends_on
│   ├── network/
│   │   ├── topology.bats             # Network membership per container
│   │   ├── isolation.bats            # Cross-network blocking
│   │   ├── ipv6.bats                 # IPv6 disabled everywhere
│   │   ├── dns.bats                  # DNS resolution, CoreDNS blocking
│   │   └── tproxy.bats              # TPROXY rules, policy routing, nftables
│   ├── security/
│   │   ├── capabilities.bats         # cap_add, cap_drop per container
│   │   ├── privileges.bats           # privileged, no-new-privileges, read-only
│   │   ├── users.bats                # Process user, UID/GID
│   │   ├── mounts.bats              # Read-only mounts, tmpfs, volumes
│   │   └── seccomp-runtime.bats     # Seccomp applied at runtime
│   ├── service/
│   │   ├── gate-processes.bats       # g3proxy, g3fcgen running, ports, certs
│   │   ├── sentinel-processes.bats   # c-icap running, ports, modules loaded
│   │   ├── scanner-processes.bats    # clamd running, ports, signatures
│   │   ├── resolver-processes.bats   # CoreDNS running, ports
│   │   ├── state-processes.bats      # Valkey running, TLS, ports
│   │   ├── toolbox-processes.bats    # MCP agent running, health endpoint
│   │   └── workspace-processes.bats  # Systemd, init service, CA trust
│   ├── config/
│   │   ├── mounted-configs.bats      # All config files mounted correctly
│   │   └── runtime-config.bats       # Config values inside running containers
│   └── state/
│       ├── valkey-acl.bats           # ACL enforcement per user
│       ├── valkey-tls.bats           # TLS connectivity
│       └── valkey-persistence.bats   # AOF, RDB, volume mounts
│
├── e2e/                              # TIER 3 — Full stack + external network
│   ├── traffic/
│   │   ├── http-flow.bats            # HTTP requests through proxy
│   │   ├── https-flow.bats           # HTTPS/TLS interception
│   │   ├── edge-cases.bats           # Timeouts, redirects, large bodies, errors
│   │   └── concurrent.bats           # Parallel requests
│   ├── scanning/
│   │   ├── malware-detection.bats    # EICAR detection, ClamAV integration
│   │   └── scan-bypass.bats          # No Content-Type bypass, whitelist anchoring
│   ├── dlp/
│   │   └── credential-detection.bats # DLP blocking, allow rules, large payloads
│   ├── dns/
│   │   └── domain-blocking.bats      # Blocked domains, allowed domains
│   ├── toolbox/
│   │   ├── mcp-tools.bats            # report_block, check_status, get_security
│   │   └── approval-system.bats      # Approval workflow
│   └── agents/
│       └── agent-system.bats         # Agent manifests, workspace runtime
│
├── setup_suite.bash                  # Auto-start containers for integration/e2e
├── run-tests.sh                      # Test runner with tier selection
└── bats/                             # BATS submodules (existing)
    ├── bats-core/
    ├── bats-assert/
    ├── bats-support/
    └── bats-file/
```

---

## 6. Helper Library Architecture

### 6.1 `lib/test_helper.bash` — Core Loader

```bash
# Loaded by every test file. Sets up paths and loads BATS libraries.
export PROJECT_ROOT="$(cd "$(dirname "${BATS_TEST_FILENAME}")" && while [[ ! -f Justfile ]] && [[ "$PWD" != "/" ]]; do cd ..; done; pwd)"
export TESTS_DIR="${PROJECT_ROOT}/tests"
load "${TESTS_DIR}/bats/bats-support/load.bash"
load "${TESTS_DIR}/bats/bats-assert/load.bash"
```

### 6.2 `lib/constants.bash` — Single Source of Truth

```bash
# Container names (match docker-compose.yml service → container_name)
export CTR_RESOLVER="polis-resolver"
export CTR_GATE="polis-gate"
export CTR_SENTINEL="polis-sentinel"
export CTR_SCANNER="polis-scanner"
export CTR_STATE="polis-state"
export CTR_TOOLBOX="polis-toolbox"
export CTR_WORKSPACE="polis-workspace"

# Network names
export NET_INTERNAL="polis_internal-bridge"
export NET_GATEWAY="polis_gateway-bridge"
export NET_EXTERNAL="polis_external-bridge"
export NET_INTERNET="polis_internet"

# Subnets
export SUBNET_INTERNAL="10.10.1.0/24"
export SUBNET_GATEWAY="10.30.1.0/24"
export SUBNET_EXTERNAL="10.20.1.0/24"

# Static IPs
export IP_RESOLVER_GW="10.30.1.10"
export IP_RESOLVER_INT="10.10.1.2"
export IP_GATE_INT="10.10.1.10"
export IP_GATE_GW="10.30.1.6"
export IP_GATE_EXT="10.20.1.3"
export IP_SENTINEL="10.30.1.5"
export IP_TOOLBOX_INT="10.10.1.20"
export IP_TOOLBOX_GW="10.30.1.20"

# Ports
export PORT_TPROXY=18080
export PORT_ICAP=1344
export PORT_CLAMAV=3310
export PORT_VALKEY=6379
export PORT_MCP=8080
export PORT_DNS=53
export PORT_G3FCGEN=2999
```

### 6.3 `lib/guards.bash` — Skip Guards

```bash
require_container() {
    for c in "$@"; do
        local state=$(docker inspect --format '{{.State.Status}}' "$c" 2>/dev/null || echo "missing")
        [[ "$state" == "running" ]] || skip "Container $c not running ($state)"
        local health=$(docker inspect --format '{{.State.Health.Status}}' "$c" 2>/dev/null || echo "none")
        [[ "$health" == "none" || "$health" == "healthy" ]] || skip "Container $c not healthy ($health)"
    done
}

require_network() {
    local host="$1" port="${2:-443}"
    timeout 3 bash -c "echo > /dev/tcp/$host/$port" 2>/dev/null || skip "$host:$port unreachable"
}

# Set security_level to relaxed with a TTL safety net.
# If teardown_file never fires (OOM, SIGKILL), the key auto-expires
# so subsequent test files start from a clean baseline.
#
# Auth: REDISCLI_AUTH env var is the safest available method per
# valkey-cli docs (avoids --pass in /proc/*/cmdline). valkey-cli 8.x
# does not support --pass-file. The env var is scoped to the sh -c
# subshell inside the container, not the host.
relax_security_level() {
    local ttl="${1:-120}"
    docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-admin --no-auth-warning \
            SET polis:config:security_level relaxed EX $ttl" 2>/dev/null || true
}

restore_security_level() {
    docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-admin --no-auth-warning \
            DEL polis:config:security_level" 2>/dev/null || true
}

# Defensive reset — called from setup_suite to guarantee clean baseline
# regardless of prior crashes or leaked state.
reset_test_state() {
    restore_security_level
}
```

Each module in `lib/assertions/` exports focused assertion functions. Tests load only what they need:

```bash
# In a test file:
setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    load "../../lib/assertions/container.bash"
    load "../../lib/assertions/security.bash"
    require_container "$CTR_GATE"
}
```


---

## 7. Test Cases — Tier 1: Unit

> **Rule:** Zero external dependencies. No Docker daemon, no network, no containers.

### 7.1 `unit/config/g3proxy-config.bats` — g3proxy Static Validation (8 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Config file exists | `services/gate/config/g3proxy.yaml` exists |
| 2 | Has resolver section | Contains `resolver:` block |
| 3 | Resolver uses Docker DNS | Contains `127.0.0.11` |
| 4 | Has ICAP REQMOD configured | Contains `icap_reqmod_service:` with `credcheck` |
| 5 | Has ICAP RESPMOD configured | Contains `icap_respmod_service:` with `squidclamav` |
| 6 | TLS cert agent on port 2999 | Contains `query_peer_addr: 127.0.0.1:2999` |
| 7 | TPROXY listener on 18080 | Contains `listen: 0.0.0.0:18080` |
| 8 | Audit ratio is 1.0 | Contains `task_audit_ratio: 1.0` |

### 7.2 `unit/config/cicap-config.bats` — c-ICAP Static Validation (10 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Config file exists | `services/sentinel/config/c-icap.conf` exists |
| 2 | Port is 0.0.0.0:1344 | Contains `Port 0.0.0.0:1344` |
| 3 | StartServers is 3 | Contains `StartServers 3` |
| 4 | Echo service loaded | Contains `Service echo srv_echo.so` |
| 5 | SquidClamav service loaded | Contains `Service squidclamav squidclamav.so` |
| 6 | DLP module loaded | Contains `Service polis_dlp srv_polis_dlp.so` |
| 7 | Credcheck alias configured | Contains `ServiceAlias credcheck polis_dlp` |
| 8 | Approval modules loaded | Contains `polis_approval_rewrite` and `polis_approval` |
| 9 | Server log path set | Contains `ServerLog /var/log/c-icap/server.log` |
| 10 | Access log path set | Contains `AccessLog /var/log/c-icap/access.log` |

### 7.3 `unit/config/corefile-config.bats` — CoreDNS Validation (4 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Corefile exists | File exists |
| 2 | Has blocklist plugin | Contains `blocklist` |
| 3 | Has forward directive | Contains `forward` |
| 4 | AAAA records return NOERROR | Contains `template ANY AAAA` |

### 7.4 `unit/config/valkey-config.bats` — Valkey Static Validation (12 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Config file exists | File exists |
| 2 | TLS port is 6379 | Contains `tls-port 6379` |
| 3 | Non-TLS port disabled | Contains `port 0` |
| 4 | TLS client auth enabled | Contains `tls-auth-clients yes` |
| 5 | Protected mode on | Contains `protected-mode yes` |
| 6 | ACL file configured | Contains `aclfile /run/secrets/valkey_acl` |
| 7 | AOF enabled | Contains `appendonly yes` |
| 8 | Max memory 256mb | Contains `maxmemory 256mb` |
| 9 | Eviction policy volatile-lru | Contains `maxmemory-policy volatile-lru` |
| 10 | IO threads 4 | Contains `io-threads 4` |
| 11 | Max clients 100 | Contains `maxclients 100` |
| 12 | Data dir is /data | Contains `dir /data` |

### 7.5 `unit/config/squidclamav-config.bats` — SquidClamav Validation (8 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Config file exists | File exists |
| 2 | No abortcontent directives | `grep ^abortcontent` fails |
| 3 | No abort directives | `grep ^abort` fails |
| 4 | All whitelists are anchored | Every `^whitelist` line starts with `^` |
| 5 | Maxsize is 100M | Contains `maxsize 100M` |
| 6 | Scan mode is ScanAllExcept | Contains `scan_mode ScanAllExcept` |
| 7 | ClamAV host is scanner | Contains `clamd_ip scanner` |
| 8 | ClamAV port is 3310 | Contains `clamd_port 3310` |

### 7.6 `unit/config/compose-config.bats` — docker-compose.yml Validation (15 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | File exists and is valid YAML | `docker compose config` or grep-based |
| 2 | All 3 networks defined | `internal-bridge`, `gateway-bridge`, `external-bridge` |
| 3 | All networks have IPv6 disabled | `enable_ipv6: false` for each |
| 4 | internal-bridge is internal | `internal: true` |
| 5 | Correct subnets | 10.10.1.0/24, 10.30.1.0/24, 10.20.1.0/24 |
| 6 | scanner-db volume named correctly | `name: polis-scanner-db` |
| 7 | state-data volume named correctly | `name: polis-state-data` |
| 8 | No profiles directives | `grep profiles:` fails |
| 9 | All services have restart policy | `restart: unless-stopped` |
| 10 | All services have logging config | `json-file` driver |
| 11 | Workspace uses sysbox runtime | `runtime: sysbox-runc` |
| 12 | Gate has required sysctls | `ip_forward=1`, `ip_nonlocal_bind=1` |
| 13 | Workspace has IPv6 disabled sysctls | `disable_ipv6=1` |
| 14 | Secrets section defines all 8 secrets | All valkey_* secrets present |
| 15 | Workspace DNS points to resolver | `dns: - 10.10.1.2` |

### 7.7 `unit/scripts/state-generate-secrets.bats` — Secret Generation (4 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | All passwords are 32 characters | Length check on each file |
| 2 | All passwords are mutually unique | Pairwise comparison |
| 3 | ACL hashes match SHA-256 of passwords | Hash verification |
| 4 | All files have permission 644 | `stat` check |

### 7.8 `unit/scripts/state-generate-certs.bats` — Certificate Generation (4 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | All .key files have permission 600 | `stat` check |
| 2 | All .crt files have permission 644 | `stat` check |
| 3 | Generated certs are valid X.509 | `openssl x509 -noout` succeeds |
| 4 | Generated CA key is at least 2048 bits | `openssl rsa -text` bit length ≥ 2048 |

### 7.9 `unit/scripts/state-health.bats` — Health Check Input Validation (3 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Invalid VALKEY_HOST rejected | Special chars → exit 1 + CRITICAL |
| 2 | Non-numeric VALKEY_PORT rejected | Letters → exit 1 + CRITICAL |
| 3 | Out-of-range VALKEY_PORT rejected | 0, 65536 → exit 1 + CRITICAL |

### 7.10 `unit/security/seccomp-profiles.bats` — Seccomp JSON Validation (6 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Gateway seccomp exists | File exists |
| 2 | Workspace seccomp exists | File exists |
| 3 | Gateway default action is ERRNO | `SCMP_ACT_ERRNO` |
| 4 | Supports x86_64 | `SCMP_ARCH_X86_64` |
| 5 | Supports aarch64 | `SCMP_ARCH_AARCH64` |
| 6 | Gateway allows setsockopt (TPROXY) | `setsockopt` in syscalls |

### 7.11 `unit/security/dockerfile-hardening.bats` — Dockerfile Analysis (4 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Gate Dockerfile has SHA256 verification | `sha256sum -c` present |
| 2 | Gate Dockerfile pins G3_SHA256 hash | `ENV G3_SHA256=` present |
| 3 | Sentinel Dockerfile has SHA256 verification | `sha256sum -c` present |
| 4 | Gate Dockerfile creates non-root user | `useradd` present |

### 7.12 `unit/security/blocklist-validation.bats` — Blocklist Files (5 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | DNS blocklist exists | File exists |
| 2 | DNS blocklist has common blocked domains | webhook.site, ngrok.io |
| 3 | URL blocklist exists | Sentinel blocklist exists |
| 4 | validate-blocklist.sh passes on valid file | Exit 0 |
| 5 | validate-blocklist.sh fails on empty file | Exit 1 + CRITICAL |

### 7.13 `unit/cli/polis-script.bats` — CLI Script Validation (20 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | polis.sh exists and is executable | File check |
| 2 | Passes bash syntax check | `bash -n` |
| 3 | --help prints usage | Contains `Usage:` |
| 4 | Unknown command exits non-zero | Exit 1 |
| 5 | Has all required commands | init, up, down, start, stop, status, logs, build, shell, test |
| 6 | Has --agent flag parsing | grep |
| 7 | Has --local flag | grep |
| 8 | Has --no-cache flag | grep |
| 9 | Has load_agent_yaml function | grep |
| 10 | Has validate_manifest_security function | grep |
| 11 | Has generate_compose_override function | grep |
| 12 | Has discover_agents function | grep |
| 13 | Old agent.conf references removed | grep fails |
| 14 | Validates metadata.name regex | grep |
| 15 | Rejects root user | grep |
| 16 | Checks path traversal | grep |
| 17 | Generates into .generated/ directory | grep |
| 18 | Defines reserved platform ports | grep |
| 19 | init.sh has SHA-256 integrity check | grep |
| 20 | init.sh has batched daemon-reload | Count = 1 |

### 7.14 `unit/cli/agent-manifests.bats` — Agent YAML Validation (10 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | openclaw agent.yaml exists | File exists |
| 2 | template agent.yaml exists | File exists |
| 3 | openclaw has required fields | apiVersion, kind, metadata, spec |
| 4 | openclaw has runtime command | `command:` present |
| 5 | openclaw has health check | `health:` present |
| 6 | openclaw install.sh exists and executable | File + exec check |
| 7 | openclaw scripts/init.sh exists | File exists |
| 8 | Old agent.conf removed | File does NOT exist |
| 9 | Old compose.override.yaml removed | File does NOT exist |
| 10 | .gitignore includes .generated/ | grep |

### 7.15 `unit/dlp/dlp-config.bats` — DLP Pattern Config (4 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | DLP config file exists | File exists |
| 2 | Has at least 5 credential patterns | `grep -c ^pattern.` >= 5 |
| 3 | Has at least 4 allow rules | `grep -c ^allow.` >= 4 |
| 4 | Has at least 3 action rules | `grep -c ^action.` >= 3 |

**Tier 1 Total: ~117 tests across 15 files**


---

## 8. Test Cases — Tier 2: Integration

> **Rule:** Requires running Docker containers. No external network calls.

### 8.1 `integration/container/lifecycle.bats` — Container Lifecycle (27 tests)

For each of the 7 long-running containers (resolver, gate, sentinel, scanner, state, toolbox, workspace):

| # | Test Pattern | Assertion |
|---|---|---|
| 1-7 | `{service}: container exists` | `docker ps -a` shows container |
| 8-14 | `{service}: container is running` | Status contains "Up" |
| 15-21 | `{service}: container is healthy` | Health status is "healthy" |

For each of the 3 init containers (gate-init, scanner-init, state-init):

| # | Test Pattern | Assertion |
|---|---|---|
| 22-24 | `{init}: completed successfully` | ExitCode=0, FinishedAt is non-zero |
| 25 | `gate-init: completed within 60 seconds` | FinishedAt − StartedAt < 60s |
| 26 | `gate-init: is not still running` | State.Status = exited |
| 27 | `gate-init: ran as root` | Config.User = root (documents intent) |

### 8.2 `integration/container/images.bats` — Image Verification (7 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Resolver uses polis-resolver-oss image | Image name check |
| 2 | Gate uses polis-gate-oss image | Image name check |
| 3 | Sentinel uses polis-sentinel-oss image | Image name check |
| 4 | Scanner uses polis-scanner-oss image | Image name check |
| 5 | State uses valkey/valkey:8-alpine | Image name check |
| 6 | Toolbox uses polis-toolbox-oss image | Image name check |
| 7 | Workspace uses polis-workspace-oss image | Image name check |

### 8.3 `integration/container/resources.bats` — Resource Limits (10 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Sentinel memory limit 3GB | 3221225472 bytes |
| 2 | Sentinel memory reservation 1GB | 1073741824 bytes |
| 3 | Scanner memory limit 3GB | 3221225472 bytes |
| 4 | Scanner memory reservation 1GB | 1073741824 bytes |
| 5 | State memory limit 512MB | 536870912 bytes |
| 6 | State memory reservation 256MB | 268435456 bytes |
| 7 | State CPU limit 1.0 | 1000000000 NanoCpus |
| 8 | Workspace memory limit 4GB | 4294967296 bytes |
| 9 | Workspace CPU limit 2.0 | 2000000000 NanoCpus |
| 10 | Gate ulimits nofile 65536 | Ulimit check |

### 8.4 `integration/container/restart-policy.bats` — Restart & Logging (14 tests)

For each of 7 containers:

| # | Test Pattern | Assertion |
|---|---|---|
| 1-7 | `{service}: restart policy is unless-stopped` | RestartPolicy check |
| 8-14 | `{service}: uses json-file logging driver` | LogConfig check |

### 8.5 `integration/network/topology.bats` — Network Membership (20 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Resolver on gateway-bridge | Network membership |
| 2 | Resolver on internal-bridge | Network membership |
| 3 | Resolver on external-bridge | Network membership |
| 4 | Gate on internal-bridge | Network membership |
| 5 | Gate on gateway-bridge | Network membership |
| 6 | Gate on external-bridge | Network membership |
| 7 | Sentinel on gateway-bridge only | On gateway, NOT on internal/external |
| 8 | Scanner on gateway-bridge | Network membership |
| 9 | Scanner on internet network | Network membership |
| 10 | Scanner NOT on internal-bridge | Negative check |
| 11 | State on gateway-bridge only | On gateway, NOT on internal/external |
| 12 | Toolbox on internal-bridge | Network membership |
| 13 | Toolbox on gateway-bridge | Network membership |
| 14 | Toolbox NOT on external-bridge | Negative check |
| 15 | Workspace on internal-bridge only | On internal, NOT on gateway/external |
| 16 | internal-bridge is internal | `docker network inspect` → Internal=true |
| 17 | gateway-bridge is internal | `docker network inspect` → Internal=true |
| 18 | All networks have IPv6 disabled | EnableIPv6=false |
| 19 | No containers expose ports to host | `docker port` empty for all |
| 20 | internet network exists | `docker network inspect` succeeds |

### 8.6 `integration/network/isolation.bats` — Cross-Network Blocking (10 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Workspace cannot reach sentinel directly | TCP to 10.30.1.5:1344 fails |
| 2 | Workspace cannot reach external-bridge | TCP to 10.20.1.3 fails |
| 3 | Sentinel cannot reach workspace | TCP to workspace IP fails |
| 4 | Workspace default route via gate | `ip route` shows 10.10.1.10 |
| 5 | Workspace has exactly 1 interface (+lo) | Interface count = 1 |
| 6 | Gate has 3 interfaces (+lo) | Interface count = 3 |
| 7 | Workspace cannot reach cloud metadata | TCP to 169.254.169.254:80 fails |
| 8 | Workspace HTTP to gateway-bridge IP blocked | curl 10.30.1.5 from workspace → refused/timeout |
| 9 | Scanner cannot reach internal-bridge | TCP to 10.10.1.10 from scanner fails |
| 10 | Scanner cannot reach workspace | TCP to workspace IP from scanner fails |

### 8.7 `integration/network/ipv6.bats` — IPv6 Disabled (6 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Workspace has no global IPv6 addresses | `ip -6 addr` empty |
| 2 | Gateway has no global IPv6 addresses | `ip -6 addr` empty |
| 3 | Workspace sysctl disable_ipv6=1 | sysctl check |
| 4 | Gateway ip6tables DROP policy (if available) | ip6tables check or skip |
| 5 | Gate-init logs show IPv6 disable | `docker logs polis-gate-init` |
| 6 | Gate-init logs show completion | Contains "Networking setup complete" |

### 8.8 `integration/network/dns.bats` — DNS Resolution & Blocking (12 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Resolver has static IP 10.30.1.10 | IP check |
| 2 | Blocks webhook.site (NXDOMAIN) | nslookup returns NXDOMAIN |
| 3 | Blocks ngrok.io (NXDOMAIN) | nslookup returns NXDOMAIN |
| 4 | Blocks ngrok-free.app (NXDOMAIN) | nslookup returns NXDOMAIN |
| 5 | Blocks transfer.sh (NXDOMAIN) | nslookup returns NXDOMAIN |
| 6 | Blocks burpcollaborator.net (NXDOMAIN) | nslookup returns NXDOMAIN |
| 7 | Blocks githab.com (typosquatting) | nslookup returns NXDOMAIN |
| 8 | Resolves github.com | nslookup succeeds |
| 9 | Resolves google.com | nslookup succeeds |
| 10 | Corefile mounted at /etc/coredns/Corefile | File exists in container |
| 11 | Blocklist mounted at /etc/coredns/blocklist.txt | File exists in container |
| 12 | Resolver has no-new-privileges | SecurityOpt check |

### 8.9 `integration/network/tproxy.bats` — TPROXY & nftables (12 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | g3proxy listening on port 18080 | `ss -tln` check |
| 2 | ip rule for fwmark 0x2 exists | `ip rule show` |
| 3 | Routing table 102 has local route | `ip route show table 102` |
| 4 | nft inet polis table exists | `nft list tables` |
| 5 | No old WSL2 tables (polis_nat, polis_mangle) | Negative check |
| 6 | No masquerade rule | Negative check |
| 7 | Forward chain policy is drop | `nft list chain` |
| 8 | TPROXY rule in prerouting_tproxy | Contains `tproxy to :18080` |
| 9 | DNS DNAT rule exists | Contains `dnat ip to 10.30.1.10` |
| 10 | IPv6 drop in prerouting | Contains `meta nfproto ipv6 drop` |
| 11 | Internal subnets excluded from TPROXY | All 3 subnets in exclusion |
| 12 | Docker DNS rules preserved | `ip nat` table has 127.0.0.11 |

### 8.10 `integration/security/capabilities.bats` — Capabilities (12 tests)

> **Prerequisite:** `docker-compose.yml` gate service must have `privileged: true` removed,
> `cap_drop: [ALL]` added, and `security_opt: seccomp=./services/gate/config/seccomp/gateway.json`
> uncommented. Gate only needs `NET_ADMIN` + `NET_RAW` for TPROXY; `gate-init` (separate
> container) retains `privileged: true` for one-shot network namespace setup.

| # | Test | Assertion |
|---|---|---|
| 1 | Gate has NET_ADMIN | CapAdd check |
| 2 | Gate has NET_RAW | CapAdd check |
| 3 | Gate drops ALL | CapDrop = ALL |
| 4 | Gate effective caps ⊆ NET_ADMIN+NET_RAW | /proc/1/status CapEff ∩ 0xFFFFFFFFFFFF ≤ 0x3000 |
| 5 | Sentinel drops ALL | CapDrop = ALL |
| 6 | Sentinel has CHOWN only | CapAdd = [CHOWN] |
| 7 | Sentinel does NOT have SETGID | CapAdd does not contain SETGID |
| 8 | Scanner drops ALL | CapDrop = ALL |
| 9 | Scanner has CHOWN only | CapAdd = [CHOWN] |
| 10 | State drops ALL | CapDrop = ALL |
| 11 | Toolbox drops ALL | CapDrop = ALL |
| 12 | Workspace drops ALL | CapDrop = ALL |

### 8.11 `integration/security/privileges.bats` — Privilege Hardening (14 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Gate is NOT privileged | Privileged=false |
| 2 | Sentinel is NOT privileged | Privileged=false |
| 3 | Scanner is NOT privileged | Privileged=false |
| 4 | State is NOT privileged | Privileged=false |
| 5 | Workspace is NOT privileged | Privileged=false |
| 6 | Gate has no-new-privileges | SecurityOpt check |
| 7 | Sentinel has no-new-privileges | SecurityOpt check |
| 8 | Scanner has no-new-privileges | SecurityOpt check |
| 9 | State has no-new-privileges | SecurityOpt check |
| 10 | Toolbox has no-new-privileges | SecurityOpt check |
| 11 | Scanner has read-only rootfs | ReadonlyRootfs=true |
| 12 | State has read-only rootfs | ReadonlyRootfs=true |
| 13 | Gate has seccomp profile applied | SecurityOpt contains seccomp= |
| 14 | Workspace has seccomp profile applied | SecurityOpt contains seccomp |

### 8.12 `integration/security/users.bats` — Process Users (7 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Gate g3proxy runs as gate user | `ps -o user=` check |
| 2 | Sentinel c-icap runs as sentinel user | `ps -o user=` check |
| 3 | Scanner runs as scanner user (UID 100) | `id` check |
| 4 | Resolver runs as UID 200 | User config check |
| 5 | State runs as UID 999 | User config check |
| 6 | Workspace polis user exists (UID 1000) | `id polis` check |
| 7 | Workspace root has nologin shell | `getent passwd root` check |

### 8.13 `integration/security/mounts.bats` — Volume & Mount Security (12 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Gate configs mounted read-only | RW=false for g3proxy.yaml |
| 2 | Sentinel configs mounted read-only | RW=false for c-icap.conf |
| 3 | Sentinel ClamAV DB mounted read-only | RW=false for /var/lib/clamav |
| 4 | Scanner ClamAV DB mounted read-write | RW=true for /var/lib/clamav |
| 5 | Sentinel has /tmp tmpfs 2GB | Tmpfs check |
| 6 | Sentinel has /var/log tmpfs 100M | Tmpfs check |
| 7 | Scanner has /tmp tmpfs | Tmpfs check |
| 8 | State has /tmp tmpfs | Tmpfs check |
| 9 | scanner-db volume exists | `docker volume ls` |
| 10 | state-data volume exists | `docker volume ls` |
| 11 | Workspace sensitive paths are tmpfs | /root/.ssh, .aws, .gnupg, etc. |
| 12 | Workspace CA cert mounted read-only | Mount check |

### 8.14 `integration/service/gate-processes.bats` — Gate Runtime (15 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | g3proxy process running | pgrep check |
| 2 | g3fcgen process running | pgrep check |
| 3 | g3proxy binary at /usr/bin/g3proxy | which check |
| 4 | g3fcgen binary at /usr/bin/g3fcgen | which check |
| 5 | g3proxy version is 1.12.x | --version check |
| 6 | g3proxy listening on 18080 | ss check |
| 7 | g3fcgen listening on UDP 2999 | ss check |
| 8 | CA certificate exists and valid | openssl check |
| 9 | CA key exists | File check |
| 10 | CA cert not expired | openssl checkend |
| 11 | CA key matches cert | Modulus comparison |
| 12 | Health check passes | /scripts/health-check.sh returns OK |
| 13 | CA cert uses SHA-256+ signature | `openssl x509 -text` → sha256 or ecdsa-with-SHA |
| 14 | CA cert has CA:TRUE basic constraint | `openssl x509 -text` → CA:TRUE |
| 15 | CA cert chain is valid | `openssl verify -CAfile ca.pem ca.pem` succeeds |

### 8.15 `integration/service/sentinel-processes.bats` — Sentinel Runtime (12 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | c-icap process running | pgrep check |
| 2 | Multiple worker processes | pgrep -c >= 2 |
| 3 | Listening on TCP 1344 | /proc/net/tcp check |
| 4 | Port bound to all interfaces | 00000000:0540 |
| 5 | PID file exists and valid | PID file → ps check |
| 6 | Entrypoint script exists and executable | File checks |
| 7 | Echo service module exists | find srv_echo.so |
| 8 | SquidClamav module exists | find squidclamav.so |
| 9 | DLP module exists | find srv_polis_dlp.so |
| 10 | Approval modules exist | find srv_polis_approval*.so |
| 11 | Server log writable | test -w check |
| 12 | Server log exists | test -f check |

### 8.16 `integration/service/scanner-processes.bats` — Scanner Runtime (8 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | ClamAV listening on 3310 | netstat check |
| 2 | Responds to PING | `echo PING | nc scanner 3310` → PONG |
| 3 | Returns version info | VERSION command |
| 4 | Signature database loaded (main.cvd) | File exists |
| 5 | Daily signatures loaded | daily.cvd or daily.cld exists |
| 6 | freshclam.conf mounted | File exists |
| 7 | freshclam configured for updates | DatabaseMirror check |
| 8 | Database volume mounted | Volume name check |

### 8.17 `integration/service/state-processes.bats` — State Runtime (6 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Valkey listening on TLS 6379 | /proc/net/tcp check |
| 2 | Config mounted at /etc/valkey/valkey.conf | File exists |
| 3 | TLS certificates mounted | /etc/valkey/tls/ exists |
| 4 | /data directory exists and writable | test -d -w |
| 5 | Secrets mounted | /run/secrets/valkey_password exists |
| 6 | ACL file mounted | /run/secrets/valkey_acl exists |

### 8.18 `integration/service/toolbox-processes.bats` — Toolbox Runtime (5 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Health endpoint responds | curl localhost:8080/health |
| 2 | LISTEN_ADDR env set | printenv check |
| 3 | VALKEY_URL env set | printenv check |
| 4 | VALKEY_USER env set | printenv check |
| 5 | Valkey TLS certs mounted | /etc/valkey/tls/ exists |

### 8.19 `integration/service/workspace-processes.bats` — Workspace Runtime (12 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Uses sysbox runtime | Runtime check |
| 2 | Systemd is PID 1 | ps -p 1 check |
| 3 | polis-init service exists | systemctl cat |
| 4 | CA certificate mounted | File exists |
| 5 | CA certificate valid | openssl check |
| 6 | Init script exists and executable | File checks |
| 7 | Has default route | ip route check |
| 8 | Can resolve gateway hostname | getent hosts gate |
| 9 | curl available | which check |
| 10 | Based on Debian | /etc/os-release check |
| 11 | polis user exists (UID 1000) | id check |
| 12 | polis user has /bin/bash | getent passwd check |

### 8.20 `integration/state/valkey-acl.bats` — ACL Enforcement (12 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Dangerous commands blocked (FLUSHALL, CONFIG, etc.) | 11 commands return error |
| 2 | mcp-agent denied unauthorized keys | NOPERM on non-polis keys |
| 3 | mcp-agent denied DEL/UNLINK | NOPERM |
| 4 | mcp-admin denied dangerous commands | NOPERM |
| 5 | log-writer denied non-allowed commands | NOPERM on SET, GET, etc. |
| 6 | log-writer denied non-allowed keys | NOPERM on non-log keys |
| 7 | healthcheck denied non-allowed commands | NOPERM |
| 8 | healthcheck denied key access | NOPERM |
| 9 | mcp-agent cannot set security_level | NOPERM |
| 10 | dlp-reader can GET but not SET security_level | Read OK, write NOPERM |
| 11 | mcp-agent cannot SETEX polis:approved:* | NOPERM (prevents self-approval) |
| 12 | mcp-agent cannot SET polis:approved:* | NOPERM (prevents self-approval) |

### 8.21 `integration/state/valkey-tls.bats` — TLS Enforcement (3 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Non-TLS connection rejected | `valkey-cli` (no `--tls`) PING → connection error |
| 2 | TLS connection with valid cert succeeds | `valkey-cli --tls` PING → PONG |
| 3 | TLS connection with wrong CA rejected | `valkey-cli --tls --cacert /wrong` → handshake error |

**Tier 2 Total: ~219 tests across 20 files**


---

## 9. Test Cases — Tier 3: E2E

> **Rule:** Full stack running + external network access. Tests real traffic flow.

### 9.0 Local httpbin Strategy

To eliminate flaky external dependencies on `httpbin.org`, E2E traffic tests
target a local [`go-httpbin`](https://github.com/mccutchen/go-httpbin) container
(single Go binary, zero deps, API-compatible with httpbin.org).

Add to `docker-compose.yml` under the `test` profile:

```yaml
  httpbin:
    image: mccutchen/go-httpbin
    container_name: polis-httpbin
    profiles: ["test"]
    networks:
      external-bridge:
        ipv4_address: 10.20.1.100
    read_only: true
    cap_drop: [ALL]
    security_opt:
      - no-new-privileges:true
    deploy:
      resources:
        limits:
          memory: 128M
          cpus: "0.5"
    healthcheck:
      test: ["CMD-SHELL", "wget -q --spider http://localhost:8080/get || exit 1"]
      interval: 10s
      timeout: 5s
      retries: 3
    restart: "no"
```

The test runner starts it automatically:

```bash
# In run-tests.sh, before E2E tier:
docker compose --profile test up -d httpbin 2>/dev/null
```

E2E tests use `$HTTPBIN_HOST` (defaults to local, falls back to httpbin.org):

```bash
# In setup_file() of traffic test files:
export HTTPBIN_HOST="${HTTPBIN_HOST:-10.20.1.100:8080}"
```

For environments without the local container, `require_network "$HTTPBIN_HOST"` skip-guards apply.

### 9.1 `e2e/traffic/http-flow.bats` — HTTP Traffic (8 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | HTTP GET returns 200 | curl httpbin.org/get → 200 |
| 2 | HTTP response body is valid | Contains expected JSON |
| 3 | HTTP POST works | POST data echoed back |
| 4 | HTTP headers preserved | Custom header round-trips |
| 5 | JSON content-type preserved | Content-Type: application/json |
| 6 | HTML content-type preserved | Content-Type: text/html |
| 7 | Custom user agent preserved | User-Agent round-trips |
| 8 | Traffic passes through ICAP | Gateway can reach sentinel:1344 |

### 9.2 `e2e/traffic/https-flow.bats` — HTTPS/TLS Traffic (6 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | HTTPS GET returns 200 | curl https://httpbin.org/get → 200 |
| 2 | HTTPS response body valid | Contains expected JSON |
| 3 | HTTPS POST works | POST data echoed back |
| 4 | HTTPS to different domains works | api.github.com → 200 or 403 |
| 5 | Workspace trusts Polis CA | No certificate errors |
| 6 | Via: ICAP header present | Response includes Via header |

### 9.3 `e2e/traffic/edge-cases.bats` — Edge Cases (12 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Slow response handled (2s delay) | 200 within 30s |
| 2 | HTTP redirects followed | redirect/1 → 200 |
| 3 | HTTPS redirects followed | redirect/1 → 200 |
| 4 | 404 responses passed through | status/404 → 404 |
| 5 | 500 responses passed through | status/500 → 500 |
| 6 | Large response (1KB) handled | wc -c >= 1000 |
| 7 | Streaming response works | stream/5 → 5+ lines |
| 8 | Empty HTTP body handled | POST with no body succeeds |
| 9 | Very long URL handled | 200-char path → 404 (not crash) |
| 10 | Connection timeout handled | Non-routable IP → timeout/error |
| 11 | DNS failure handled gracefully | nonexistent.invalid → failure |
| 12 | Direct IP access intercepted | http://1.1.1.1 → not 000 |

### 9.4 `e2e/traffic/concurrent.bats` — Concurrency (2 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | 3 concurrent HTTP requests succeed | All return 200 |
| 2 | Mixed HTTP/HTTPS concurrent requests | All succeed |

### 9.5 `e2e/scanning/malware-detection.bats` — Malware Scanning (10 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Clean HTTP file passes through | robots.txt → 200 |
| 2 | Clean HTTPS file passes through | robots.txt → 200 |
| 3 | ClamAV detects EICAR pattern | clamdscan → FOUND |
| 4 | EICAR download blocked | eicar.org → not 200 |
| 5 | Malware renamed as .png detected | EICAR in .png → FOUND |
| 6 | EICAR with spoofed Content-Type detected | .mp4 extension → FOUND |
| 7 | ClamAV has main signature database | main.cvd exists |
| 8 | ClamAV has daily signatures | daily.cvd or daily.cld |
| 9 | ICAP can reach ClamAV | PING → PONG |
| 10 | ClamAV responds within timeout | VERSION within 3s |

### 9.6 `e2e/scanning/scan-bypass.bats` — No Scan Bypass (8 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | No Content-Type bypass in running config | No abortcontent directives |
| 2 | No abort directives in running config | No abort directives |
| 3 | Video files scanned (no bypass) | No abort.*video |
| 4 | Audio files scanned (no bypass) | No abort.*audio |
| 5 | Image files scanned (no bypass) | No abort.*image |
| 6 | Scan mode is ScanAllExcept | Config check |
| 7 | No unanchored whitelist patterns | No `whitelist .*` |
| 8 | Whitelist prevents suffix attack | Regex test |

### 9.7 `e2e/dlp/credential-detection.bats` — DLP (5 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Anthropic key to api.anthropic.com ALLOWED | No x-polis-block header |
| 2 | Anthropic key to google.com BLOCKED | x-polis-block: true |
| 3 | RSA private key to any destination BLOCKED | x-polis-block: true |
| 4 | Plain traffic without credentials ALLOWED | No x-polis-block header |
| 5 | Credential in tail of >1MB body BLOCKED | x-polis-block: true |

### 9.8 `e2e/dns/domain-blocking.bats` — DNS E2E (4 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Workspace resolves external domains | getent hosts httpbin.org |
| 2 | Workspace resolves internal services | getent hosts resolver → 10.10.1.2 |
| 3 | Whitelisted repos accessible (Debian) | curl deb.debian.org → 200 |
| 4 | Whitelisted repos accessible (npm) | curl registry.npmjs.org → 200/301 |

### 9.9 `e2e/toolbox/mcp-tools.bats` — MCP Tool Operations (8 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | report_block stores in Valkey | Key created with data |
| 2 | report_block sets TTL | TTL > 0 and <= 3600 |
| 3 | report_block returns approval command | Contains /polis-approve |
| 4 | report_block redacts pattern from response | Pattern not in response |
| 5 | check_request_status returns pending | Status = pending |
| 6 | check_request_status returns not_found | Unknown ID → not_found |
| 7 | get_security_status returns valid JSON | Contains expected fields |
| 8 | list_pending_approvals returns stored requests | Contains request_id |

### 9.10 `e2e/toolbox/approval-system.bats` — Approval Workflow (5 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | REQMOD rewriter module exists | .so file check |
| 2 | RESPMOD scanner module exists | .so file check |
| 3 | REQMOD service active | c-icap-client check |
| 4 | RESPMOD service active | c-icap-client check |
| 5 | Valkey ACL prevents agent self-approval | ACL check |

### 9.11 `e2e/agents/agent-system.bats` — Agent Plugin System (6 tests)

| # | Test | Assertion |
|---|---|---|
| 1 | Workspace container exists | docker ps check |
| 2 | Workspace is running | Status = Up |
| 3 | Can determine active agent from image tag | Image regex match |
| 4 | docker-compose workspace uses latest tag | grep check |
| 5 | docker-compose base healthcheck includes ip route | grep check |
| 6 | Workspace can access HTTP via TPROXY | curl example.com → 200 |

**Tier 3 Total: ~74 tests across 11 files**

---

## 10. Test Runner & CI

### 10.1 Updated `run-tests.sh`

```bash
# Tier selection:
./run-tests.sh unit           # ~30s, no Docker needed
./run-tests.sh integration    # ~3min, needs containers
./run-tests.sh e2e            # ~10min, needs containers + network
./run-tests.sh all            # Everything

# Tag-based filtering:
./run-tests.sh --filter-tags "security"
./run-tests.sh --filter-tags "network"

# CI mode (auto-starts httpbin for E2E, resets test state):
./run-tests.sh --ci unit              # Fast PR check
./run-tests.sh --ci integration       # Full PR check
./run-tests.sh --ci all               # Nightly
```

### 10.2 CI Pipeline

```yaml
# .github/workflows/ci.yml
jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { submodules: recursive }
      - run: ./tests/run-tests.sh unit

  integration-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
        with: { submodules: recursive }
      - run: ./tests/run-tests.sh --ci integration

  e2e-tests:
    runs-on: ubuntu-latest
    needs: integration-tests
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4
        with: { submodules: recursive }
      - run: ./tests/run-tests.sh --ci e2e
```

### 10.3 Test Counts Summary

| Tier | Files | Tests | Dependencies | Speed |
|---|---|---|---|---|
| Unit | 15 | ~117 | None | < 30s |
| Integration | 20 | ~219 | Docker containers | < 3min |
| E2E | 11 | ~74 | Full stack + network | < 10min |
| **Total** | **46** | **~410** | | |

> Note: The current suite has ~638 tests but with massive duplication. The rewrite consolidates to ~410 unique, non-overlapping tests with better coverage. See Appendix C for the full old→new mapping.

---

## 11. Migration Plan

### Phase 1: Infrastructure (Week 1)

0. **Compose hardening prerequisite:** Remove `privileged: true` from gate service, add `cap_drop: [ALL]`, uncomment `seccomp=./services/gate/config/seccomp/gateway.json`, add `no-new-privileges:true` to gate `security_opt`. Gate-init retains `privileged: true` (one-shot network setup).
1. Create `tests/lib/` directory with all helper modules
2. Create `tests/lib/mocks/mock_helper.bash` (native command mock + call tracking)
3. Create `tests/lib/constants.bash` with all container/network constants
4. Create `tests/lib/guards.bash` with skip guards (including TTL-based `relax_security_level`)
5. Create assertion modules in `tests/lib/assertions/`
6. Create fixture files in `tests/lib/fixtures/invalid/` (negative test cases only)

### Phase 2: Unit Tests (Week 1-2)

7. Write all 15 unit test files (116 tests)
8. Verify all pass without Docker: `./run-tests.sh unit`
9. Delete old mislabeled "unit" tests from `services/*/tests/unit/` that require containers

### Phase 3: Integration Tests (Week 2-3)

10. Write all 20 integration test files (196 tests)
11. Verify all pass with running containers: `./run-tests.sh integration`
12. Delete old integration tests from `services/*/tests/integration/` and `tests/integration/`

### Phase 4: E2E Tests (Week 3)

13. Write all 11 E2E test files (74 tests)
14. Verify all pass with full stack: `./run-tests.sh e2e`
15. Delete old E2E tests from `tests/e2e/`

### Phase 4.5: Parallel Validation (Week 3-4)

> **Rollback safety net.** Do not delete old tests until this phase confirms zero coverage gaps.

16. Move all old test files to `tests/_legacy/` (not deleted yet)
17. Run BOTH old (`_legacy/`) and new suites in CI for 1 sprint
18. Compare results: any old test that passes but has no new equivalent is a coverage gap
19. Produce the Appendix C coverage mapping (old test → new test or REMOVED-DUPLICATE)
20. Only proceed to Phase 5 after zero-gap confirmation

### Phase 5: Cleanup (Week 4)

21. Remove `tests/_legacy/` directory
22. Update `run-tests.sh` for new directory structure
23. Update CI pipeline
24. Update documentation
25. Final full-suite validation

### File Deletion List

After migration, remove:
- `tests/unit/polis-script.bats` → replaced by `unit/cli/polis-script.bats`
- `tests/integration/*.bats` (5 files) → consolidated into `integration/`
- `tests/e2e/*.bats` (7 files) → consolidated into `e2e/`
- `tests/helpers/common.bash` → replaced by modular `lib/`
- `tests/helpers/network.bash` → merged into `lib/assertions/network.bash`
- `tests/helpers/valkey.bash` → merged into `lib/assertions/`
- `tests/fixtures/expected-names.bash` → moved to `lib/fixtures/`
- `services/gate/tests/` (6 files) → consolidated into `integration/` and `unit/`
- `services/sentinel/tests/` (6 files) → consolidated
- `services/scanner/tests/` (2 files) → consolidated
- `services/state/tests/` (2 files) → consolidated
- `services/resolver/tests/` (2 files) → consolidated
- `services/workspace/tests/` (5 files) → consolidated
- `services/toolbox/tests/` (2 files) → consolidated

**Total files removed:** 38  
**Total files created:** 46 test files + 10 library files = 56

---

## Appendix A: Concern-to-File Mapping

| Concern | Unit | Integration | E2E |
|---|---|---|---|
| **Gate/Proxy** | g3proxy-config, gate-init, gate-health | gate-processes, tproxy | http-flow, https-flow, edge-cases |
| **Sentinel/ICAP** | cicap-config, squidclamav-config | sentinel-processes | scan-bypass |
| **Scanner/ClamAV** | — | scanner-processes | malware-detection |
| **Resolver/DNS** | corefile-config, blocklist-validation | dns | domain-blocking |
| **State/Valkey** | valkey-config, generate-secrets, generate-certs, state-health | state-processes, valkey-acl | — |
| **Toolbox/MCP** | — | toolbox-processes | mcp-tools, approval-system |
| **Workspace** | — | workspace-processes | agent-system |
| **Security** | seccomp-profiles, dockerfile-hardening | capabilities, privileges, users, mounts, seccomp-runtime | — |
| **Network** | compose-config | topology, isolation, ipv6, tproxy | concurrent |
| **CLI** | polis-script, agent-manifests | — | — |
| **DLP** | dlp-config | — | credential-detection |

## Appendix B: Tag Taxonomy

Every `.bats` file includes `bats file_tags=` with these tags:

| Tag | Meaning |
|---|---|
| `unit` | Tier 1 |
| `integration` | Tier 2 |
| `e2e` | Tier 3 |
| `config` | Configuration validation |
| `security` | Security hardening |
| `network` | Network topology/isolation |
| `scanning` | Malware scanning |
| `dlp` | Data loss prevention |
| `dns` | DNS resolution/blocking |
| `state` | Valkey/state management |
| `toolbox` | MCP tools |
| `traffic` | HTTP/HTTPS traffic flow |
| `cli` | CLI script |
| `agents` | Agent plugin system |

## Appendix C: Coverage Mapping Template

> **Required during Phase 4.5.** Every old test must map to a new test or be explicitly marked as a removed duplicate. This is the migration's safety net — the 37% test reduction (638 → ~392) must be justified line by line.

| Old File | Old Test Name | New File | New Test # | Status |
|---|---|---|---|---|
| `services/gate/tests/unit/gateway.bats` | gateway: container exists | `integration/container/lifecycle.bats` | #1 | MOVED (was mislabeled unit) |
| `services/gate/tests/unit/gateway.bats` | gateway: container is running | `integration/container/lifecycle.bats` | #8 | MOVED |
| `services/sentinel/tests/integration/icap-hardening.bats` | hardening: no abortcontent directives | `e2e/scanning/scan-bypass.bats` | #1 | CONSOLIDATED |
| `tests/e2e/icap-hardening.bats` | e2e-hardening: no abortcontent in running config | `e2e/scanning/scan-bypass.bats` | #1 | REMOVED-DUPLICATE |
| `tests/integration/hardening.bats` | hardening: workspace has CAP_DROP=ALL | `integration/security/capabilities.bats` | #11 | CONSOLIDATED |
| ... | ... | ... | ... | ... |

**Status values:**
- `MOVED` — Test relocated to correct tier, logic preserved
- `CONSOLIDATED` — Multiple old tests merged into one new test
- `REMOVED-DUPLICATE` — Exact duplicate of another test (cite which)
- `REMOVED-OBSOLETE` — Tests feature that no longer exists
- `SPLIT` — One old test became multiple new tests
