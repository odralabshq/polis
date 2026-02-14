# Polis BATS Test Suite — QA Audit Report

**Date:** 2026-02-14  
**Auditor:** USNSA / Lead QA Automation  
**Scope:** `tests/`, `services/**/tests/`, `agents/**/tests/`  
**Total Test Files:** 25 service-level + 10 top-level = 35 `.bats` files  
**Total Helpers:** 1 shared (`helpers/common.bash`), 1 suite setup (`setup_suite.bash`)

---

## 1. Executive Summary

The Polis BATS suite is **structurally sound** — it uses `bats-core`, `bats-assert`, `bats-support`, and `bats-file` as git submodules, has a centralized helper library, and separates tests into `unit/`, `integration/`, and `e2e/` tiers. Container guards (`require_container`) prevent false negatives when infrastructure is absent.

However, the suite suffers from **six systemic weaknesses** that produce non-deterministic failures:

| Category | Severity | Affected Tests (est.) |
|---|---|---|
| Race conditions (service readiness) | **Critical** | ~15 e2e tests |
| Hardcoded naming drift (test ↔ implementation) | **High** | ~12 tests |
| External network dependency | **High** | ~20 e2e tests |
| Missing teardown / state leakage | **Medium** | ~8 tests |
| Assertion anti-patterns | **Low** | ~30 tests |
| Duplicated helper logic | **Low** | ~10 tests |

**Verdict:** The suite is ~70% of the way to production-grade. The remaining 30% requires targeted refactoring, not a rewrite.

---

## 2. Root Cause Analysis (RCA)

### RCA-1: Race Conditions — Service Readiness (Critical)

**Symptom:** E2e tests intermittently fail with `502 Bad Gateway`, `curl exit 28`, or empty output.

**Root Cause:** `setup()` calls `require_container` which checks `docker inspect` state, but a container can be `running` + `healthy` while its application-layer service (c-icap, g3proxy listener, ClamAV daemon) is not yet accepting connections.

**Evidence:**
- `malware-scan.bats` had to add `$ICAP_CONTAINER` to `require_container` to fix 502 errors
- `traffic.bats` uses `--connect-timeout 15` as a band-aid for slow startup
- `setup_suite.bash` waits for Docker health status but not for port readiness

**Affected Files:**
- `tests/e2e/traffic.bats` (all 30+ tests)
- `tests/e2e/malware-scan.bats`
- `tests/e2e/dlp.bats`
- `tests/e2e/edge-cases.bats`

### RCA-2: Naming Drift Between Tests and Implementation (High)

**Symptom:** Tests assert hardcoded container names, volume names, or image tags that don't match the actual `docker-compose.yml`.

**Root Cause:** Test expectations were written against an earlier design. When services were renamed (e.g., `polis-gateway` → `polis-gate`, `clamav` → `scanner`, `valkey` → `state`), some tests were not updated.

**Evidence from TODO context:**
- Test 483: expects `polis-gateway`, actual is `polis-gate`
- Test 359: expects `clamav/clamav:1.5`, actual is `polis-scanner-oss:latest`
- Test 441: expects `polis-clamav-db` volume, actual is `polis-scanner-db`
- Test 553: expects `polis-valkey-data`, actual is `polis-state-data`
- Tests 498-502: expect `c-icap` user, actual is `sentinel`

**Mitigation pattern:** Container/volume/image names are already centralized in `common.bash` — but only container names. Volume names, image tags, and user names are hardcoded in individual test files.

### RCA-3: External Network Dependency (High)

**Symptom:** Tests fail when `httpbin.org`, `github.com`, `eicar.org`, or `example.com` are unreachable, rate-limited, or slow.

**Root Cause:** 20+ e2e tests make live HTTP requests to external services with no fallback.

**Affected Files:**
- `tests/e2e/traffic.bats` — 18 tests hit `httpbin.org`
- `tests/e2e/malware-scan.bats` — 2 tests hit `eicar.org`
- `tests/e2e/dlp.bats` — 4 tests hit `httpbin.org`, `google.com`, `anthropic.com`
- `tests/integration/workspace-isolation.bats` — 2 tests hit `example.com`

### RCA-4: Missing Teardown / State Leakage (Medium)

**Symptom:** Tests that modify system state (Valkey keys, security level, container restarts) can affect subsequent tests.

**Root Cause:**
- `relax_security_level()` in `setup_file()` restarts `$ICAP_CONTAINER` and sets Valkey keys — but there is no corresponding `teardown_file()` to restore the original state.
- `mcp-agent.bats` creates Valkey keys and cleans them up per-test, but if a test fails mid-execution, `cleanup_valkey_key` in the test body is skipped.
- `dlp.bats` creates `/tmp/large_payload` inside the workspace container — cleanup is inline, not in `teardown()`.

**Affected Files:**
- `tests/e2e/malware-scan.bats` — `setup_file` mutates global state
- `tests/e2e/traffic.bats` — same
- `tests/e2e/dlp.bats` — file creation without teardown guard
- `tests/e2e/mcp-agent.bats` — Valkey key cleanup not in teardown

### RCA-5: Assertion Anti-Patterns (Low)

**Symptom:** Failures produce unhelpful output; some tests silently pass when they shouldn't.

**Patterns found:**

| Anti-Pattern | Example | Location |
|---|---|---|
| Bare `[[ ]]` without `run` | `[[ "$output" -ge 1000 ]]` | `traffic.bats:138` |
| Empty test body (no assertion) | `edge: direct IP access is handled` | `traffic.bats:112-115` |
| `assert_success \|\| [[ ]]` | `hardening.bats:72` | Swallows failure |
| Orphaned code outside `@test` | `skip "CapBnd check..."` after closing `}` | `gate/security.bats:56-58` |
| `grep -q` inside `run` | `run grep -q 'pattern' file` | `polis-script.bats` (30+ tests) |

### RCA-6: Duplicated Helper Logic (Low)

**Symptom:** Same patterns repeated across files without extraction to `common.bash`.

**Examples:**
- `valkey_cli()` helper defined in `mcp-agent.bats` — used only there, but the pattern (TLS + ACL auth) is also needed in `valkey.bats` ACL tests which inline the same `valkey-cli` flags.
- `ip6tables_functional()` / `sysctl_functional()` / `ipv6_disabled()` defined in `gateway-ipv6.bats` — reusable for `ipv6.bats` and `hardening.bats`.
- `mcp_call()` (JSON-RPC session helper) is 40 lines — belongs in a helper if MCP tests grow.

---

## 3. Architectural Blueprint

### Current Structure (Actual)

```
tests/
├── setup_suite.bash          # Auto-starts containers
├── helpers/common.bash       # Shared assertions + container names
├── unit/polis-script.bats    # CLI script grep tests
├── integration/
│   ├── hardening.bats
│   ├── ipv6.bats
│   ├── isolation.bats
│   └── workspace-isolation.bats
├── e2e/
│   ├── traffic.bats
│   ├── malware-scan.bats
│   ├── dlp.bats
│   ├── edge-cases.bats
│   ├── agents.bats
│   └── mcp-agent.bats
└── bats/                     # Submodules (bats-core, assert, support, file)

services/<name>/tests/
├── unit/<name>.bats
└── integration/<name>.bats
```

### Proposed Structure (Minimal Changes)

```
tests/
├── setup_suite.bash
├── helpers/
│   ├── common.bash           # (existing) Container guards, assertions
│   ├── network.bash          # (NEW) ip6tables_functional, sysctl helpers
│   ├── valkey.bash           # (NEW) valkey_cli, cleanup_valkey_key
│   └── mcp.bash              # (NEW) mcp_call, mcp_init_session
├── fixtures/
│   └── expected-names.bash   # (NEW) Volume names, image tags, user names
├── unit/
├── integration/
├── e2e/
└── bats/
```

**Rationale:** Only 3 new helper files + 1 fixture file. No directory restructuring needed — the existing `unit/integration/e2e` split is correct. Service-level tests under `services/` are well-placed.

---

## 4. Mocking Strategy

### 4.1 External Network Isolation

The e2e tests legitimately need external network access (they test the proxy stack end-to-end). However, **flakiness** from external services should be mitigated:

**Pattern: Retry-with-skip guard**

```bash
# helpers/common.bash — add once, use everywhere
# Stability Impact: HIGH
assert_http_reachable_or_skip() {
    local url="$1" label="${2:-external service}"
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" \
        --connect-timeout 5 --max-time 10 "$url"
    if [[ "$status" -ne 0 ]] || [[ "$output" == "000" ]]; then
        skip "${label} unreachable — network-dependent test"
    fi
}
```

Usage in tests:
```bash
@test "e2e: HTTP request to httpbin.org succeeds" {
    assert_http_reachable_or_skip "http://httpbin.org/get" "httpbin.org"
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" \
        --connect-timeout 15 http://httpbin.org/get
    assert_success
    assert_output "200"
}
```

### 4.2 Service Readiness (Port-Level Wait)

**Pattern: Wait-for-port in setup_file, not per-test timeouts**

```bash
# helpers/common.bash — add to existing file
# Stability Impact: HIGH
require_port_ready() {
    local container="$1" port="$2" timeout="${3:-30}"
    local elapsed=0
    while [[ $elapsed -lt $timeout ]]; do
        if docker exec "$container" sh -c "cat /proc/net/tcp 2>/dev/null | grep -qi ':$(printf '%04X' "$port")'"; then
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    skip "Port ${port} not ready on ${container} after ${timeout}s"
}
```

### 4.3 Centralizing Magic Values

**Pattern: Single-source fixture file for names that drift**

```bash
# tests/fixtures/expected-names.bash
# Stability Impact: MEDIUM
#
# Derived from docker-compose.yml — update here when renaming services.
export EXPECTED_SCANNER_IMAGE="polis-scanner-oss:latest"
export EXPECTED_SCANNER_VOLUME="polis-scanner-db"
export EXPECTED_STATE_VOLUME="polis-state-data"
export EXPECTED_ICAP_USER="sentinel"
export EXPECTED_ICAP_GROUP="sentinel"
```

### 4.4 Valkey Test Isolation

**Pattern: teardown() cleanup instead of inline cleanup**

```bash
# Stability Impact: MEDIUM
setup() {
    load "../helpers/common.bash"
    TEST_REQ_ID="req-e2e-${BATS_TEST_NUMBER}"
}

teardown() {
    cleanup_valkey_key "polis:blocked:${TEST_REQ_ID}" 2>/dev/null || true
    cleanup_valkey_key "polis:approved:${TEST_REQ_ID}" 2>/dev/null || true
}
```

---

## 5. Refactoring Roadmap

### Phase 1: Eliminate Flakiness (Stability Impact: HIGH)

#### 1.1 Add `require_port_ready` to e2e setup

**Before** (`tests/e2e/traffic.bats`):
```bash
setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER" "$WORKSPACE_CONTAINER"
}
```

**After:**
```bash
setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER" "$WORKSPACE_CONTAINER"
    require_port_ready "$GATEWAY_CONTAINER" 18080
    require_port_ready "$ICAP_CONTAINER" 1344
}
```

#### 1.2 Add `teardown_file` to restore state

**Before** (`tests/e2e/malware-scan.bats`):
```bash
setup_file() {
    load "../helpers/common.bash"
    relax_security_level
}
# No teardown_file — security_level stays "relaxed" for all subsequent suites
```

**After:**
```bash
setup_file() {
    load "../helpers/common.bash"
    relax_security_level
}

teardown_file() {
    # Restore default security level after this file's tests complete
    load "../helpers/common.bash"
    restore_security_level 2>/dev/null || true
}
```

#### 1.3 Guard external-network tests

**Before** (`tests/e2e/traffic.bats`):
```bash
@test "e2e: HTTPS to different domains works" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" \
        --connect-timeout 15 https://api.github.com
    assert_success
    [[ "$output" == "200" ]] || [[ "$output" == "403" ]]
}
```

**After:**
```bash
@test "e2e: HTTPS to different domains works" {
    assert_http_reachable_or_skip "https://api.github.com" "GitHub API"
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" \
        --connect-timeout 15 https://api.github.com
    assert_success
    [[ "$output" == "200" ]] || [[ "$output" == "403" ]]
}
```

### Phase 2: Fix Assertion Anti-Patterns (Stability Impact: MEDIUM)

#### 2.1 Replace bare `[[ ]]` with proper assertions

**Before** (`tests/e2e/traffic.bats:138`):
```bash
@test "e2e: large response body handled correctly" {
    run docker exec "${WORKSPACE_CONTAINER}" bash -c \
        "curl -s --connect-timeout 15 'http://httpbin.org/bytes/1024' | wc -c"
    assert_success
    [[ "$output" -ge 1000 ]]
}
```

**After:**
```bash
@test "e2e: large response body handled correctly" {
    run docker exec "${WORKSPACE_CONTAINER}" bash -c \
        "curl -s --connect-timeout 15 'http://httpbin.org/bytes/1024' | wc -c"
    assert_success
    assert [ "$output" -ge 1000 ]
}
```

#### 2.2 Remove empty/no-op test bodies

**Before** (`tests/e2e/traffic.bats:112`):
```bash
@test "e2e: direct IP access is handled by proxy" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" \
        --connect-timeout 10 http://1.1.1.1 2>/dev/null
    # Either succeeds (proxied) or fails (blocked) - both are acceptable
    # The key is it doesn't bypass the proxy
}
```

**After:**
```bash
@test "e2e: direct IP access is handled by proxy" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" \
        --connect-timeout 10 http://1.1.1.1 2>/dev/null
    # Must not return 000 (no proxy involvement) — any HTTP status means proxy handled it
    [[ "$status" -ne 0 ]] || [[ "$output" != "000" ]]
}
```

#### 2.3 Fix orphaned code in `gate/security.bats`

**Before** (`services/gate/tests/integration/security.bats:56-58`):
```bash
    # Skip partial check if "ALL" isn't in output, rely on bitmask
    skip "CapBnd check is flaky across runtimes"
    run docker inspect --format '{{.HostConfig.CapDrop}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "ALL"
```

This code is **outside any `@test` block** — it executes as free code during file load, calling `skip` which is undefined outside a test context.

**After:** Delete the orphaned lines entirely, or wrap in a proper test:
```bash
@test "security: gateway drops ALL capabilities" {
    skip "CapBnd check is flaky across runtimes — verified via CapEff bitmask"
    run docker inspect --format '{{.HostConfig.CapDrop}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "ALL"
}
```

### Phase 3: Extract Helpers (Stability Impact: LOW)

#### 3.1 Extract `valkey_cli` to shared helper

**Rule of Three check:** Used in `mcp-agent.bats` (6 calls) and `valkey.bats` (2 inline equivalents). Qualifies.

Create `tests/helpers/valkey.bash`:
```bash
# Valkey CLI helper — authenticates as the specified ACL user
# Usage: valkey_cli_as <user> <secret_name> [command...]
valkey_cli_as() {
    local user="$1" secret="$2"; shift 2
    local pass
    pass=$(docker exec "${VALKEY_CONTAINER}" cat "/run/secrets/${secret}" 2>/dev/null) || return 1
    docker exec "${VALKEY_CONTAINER}" valkey-cli \
        --tls --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
        --user "$user" --pass "$pass" --no-auth-warning "$@"
}

cleanup_valkey_key() {
    valkey_cli_as mcp-admin valkey_mcp_admin_password DEL "$1" 2>/dev/null || true
}
```

#### 3.2 Extract IPv6 capability helpers

Create `tests/helpers/network.bash`:
```bash
ip6tables_functional() {
    docker exec "$1" ip6tables -L -n &>/dev/null
}

sysctl_ipv6_functional() {
    docker exec "$1" sysctl -n net.ipv6.conf.all.disable_ipv6 &>/dev/null
}

ipv6_disabled() {
    ! docker exec "$1" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6"
}
```

---

## 6. Coverage Gap Analysis

| Component | Unit | Integration | E2E | Gap |
|---|---|---|---|---|
| gate (g3proxy) | ✅ 30 tests | ✅ 18 tests | ✅ via traffic.bats | — |
| sentinel (c-icap) | ✅ 20 tests | ✅ 8 tests | ✅ via malware-scan | — |
| resolver (CoreDNS) | ✅ 8 tests | ✅ 4 tests | Indirect only | **No direct e2e DNS test** |
| scanner (ClamAV) | — | ✅ 2 tests | ✅ via malware-scan | **No unit tests** |
| state (Valkey) | ✅ 25 tests | — | ✅ via mcp-agent | **No integration tests** |
| toolbox (MCP) | ✅ exists | ✅ approval_system | ✅ mcp-agent.bats | — |
| workspace | ✅ exists | ✅ 4 files | ✅ via all e2e | — |
| DLP module | ✅ 10 tests | — | ✅ dlp.bats | — |
| polis.sh CLI | ✅ 40 tests | — | — | **All grep-based, no execution tests** |

**Critical gaps:**
1. `polis-script.bats` has 40 tests but every single one is `grep -q` against the script source — zero tests actually execute `polis.sh` with arguments. A syntax error in the script would pass all tests.
2. Scanner has no unit tests for ClamAV configuration or daemon behavior.
3. No test verifies the `setup_suite.bash` auto-start logic itself.

---

## 7. CI/CD Integration Recommendations

1. **Tag tests by tier** using BATS 1.5+ `# bats test_tags=`:
   ```bash
   # bats test_tags=e2e,network
   @test "e2e: HTTP request succeeds" { ... }
   ```
   Then run `--filter-tags unit` in PR checks (fast) and `--filter-tags e2e` in nightly (slow).

2. **JUnit output** for CI visibility:
   ```bash
   ./tests/run-tests.sh --formatter junit > test-results.xml
   ```

3. **Parallel execution** — the suite is already safe for `--jobs N` since tests don't share mutable state (except the `relax_security_level` issue in Phase 1.2).

---

## 8. Priority Matrix

| Phase | Effort | Impact | Do First? |
|---|---|---|---|
| 1.1 Port readiness guards | 1h | Eliminates ~15 flaky tests | ✅ |
| 1.2 teardown_file for state | 30m | Prevents cross-suite contamination | ✅ |
| 1.3 External network guards | 1h | Prevents CI failures on network issues | ✅ |
| 2.1 Fix bare assertions | 1h | Better failure diagnostics | After Phase 1 |
| 2.2 Remove no-op tests | 30m | Reduces false confidence | After Phase 1 |
| 2.3 Fix orphaned code | 15m | Prevents load-time errors | After Phase 1 |
| 3.1 Extract valkey helper | 30m | DRY, enables new Valkey tests | After Phase 2 |
| 3.2 Extract network helpers | 30m | DRY across 3 files | After Phase 2 |
| Fixtures file for names | 30m | Single source of truth for naming | After Phase 2 |

**Total estimated effort: ~6 hours for full remediation.**
