# Polis BATS Test Suite — Comprehensive Refactoring Plan

**Date:** 2026-02-14
**Author:** Lead SDET / Bash Specialist
**Sources:** Security Audit, Architecture Review, QA Audit Report, Architecture Review of QA Audit
**Scope:** 37 `.bats` files across `tests/` and `services/**/tests/`
**Estimated Total Tests:** ~555

---

## 1. Executive Summary

Four independent audits of the Polis BATS test suite converge on the same conclusion: the suite is **~70% of the way to production-grade** but is undermined by systemic failures that make it unreliable as a quality gate. This plan synthesizes all findings, resolves contradictions between audits, and provides a single prioritized execution roadmap.

### Core Problems (Consensus Across All Audits)

| # | Problem | Severity | Affected Tests |
|---|---------|----------|----------------|
| 1 | Zero teardown discipline — not a single `teardown()` or `teardown_file()` in any project file | CRITICAL | All 37 files |
| 2 | Global state mutation (`relax_security_level`) without restore | CRITICAL | 5 files, ~40 tests |
| 3 | Race conditions — Docker health ≠ port readiness | HIGH | ~15 e2e tests |
| 4 | External network dependency with no fallback | HIGH | 59 refs across 5 e2e files |
| 5 | Tier mislabeling — 8 "unit" files require running containers | MEDIUM | 8 files, ~190 tests |
| 6 | Security assertion contradictions (privileged mode, ACL bypass) | CRITICAL | 2 vulnerabilities |
| 7 | Empty test bodies / orphaned code outside `@test` blocks | MEDIUM | 4 tests |
| 8 | Bare `[[ ]]` assertions with no diagnostics | LOW | 27 instances |
| 9 | Grep-only CLI tests — zero execution coverage | MEDIUM | 40 tests |
| 10 | Duplicated helper logic across files | LOW | ~10 tests |

### Audit Disagreements Resolved

| Topic | QA Audit Said | Architecture Review Corrected | This Plan Uses |
|---|---|---|---|
| Port readiness function | Create new `require_port_ready()` using `/proc/net/tcp` | `wait_for_port()` already exists at `common.bash:270` using `ss -tlnp` | **Existing `wait_for_port`** |
| Port check location | Add to `setup()` (per-test) | Add to `setup_file()` (per-file) — port is a file-level invariant | **`setup_file()` only** |
| Network guard pattern | Pre-check with `assert_http_reachable_or_skip()` | Post-check with `run_with_network_skip()` — avoids TOCTOU race | **Post-check pattern** |
| File count | 35 files | 37 files (missing `icap-hardening.bats`, `agents.bats`) | **37 files** (verified) |
| External dependency count | ~20 tests | ~40-50 tests (79 pattern matches) | **59 refs** (verified via grep) |
| Parallel execution safety | "Already safe for `--jobs N`" | NOT safe until teardown is fixed | **Unsafe until Phase 1 complete** |

---

## 2. Current State Inventory

### 2.1 File Classification (Verified)

**True Unit Tests (no container dependency):**

| File | Tests | Notes |
|---|---|---|
| `tests/unit/polis-script.bats` | ~40 | All grep-based; zero execution tests |
| `services/resolver/tests/unit/dns.bats` | ~8 | Static config validation ✓ |
| `services/state/tests/unit/valkey-properties.bats` | ~15 | Mixed — some need containers |

**Misclassified as Unit (actually Integration — require `docker exec`):**

| File | Tests | Container Dependency |
|---|---|---|
| `services/gate/tests/unit/gateway.bats` | ~30 | `GATEWAY_CONTAINER` |
| `services/gate/tests/unit/gateway-ipv6.bats` | ~18 | `GATEWAY_CONTAINER` |
| `services/sentinel/tests/unit/icap.bats` | ~20 | `ICAP_CONTAINER` |
| `services/sentinel/tests/unit/dlp.bats` | ~10 | `ICAP_CONTAINER` |
| `services/toolbox/tests/unit/mcp-agent.bats` | ~12 | `MCP_AGENT_CONTAINER` |
| `services/state/tests/unit/valkey.bats` | ~25 | `VALKEY_CONTAINER` |
| `services/workspace/tests/unit/workspace.bats` | ~40 | `WORKSPACE_CONTAINER` |

**Integration Tests (correctly classified):** 14 files, ~200 tests
**E2E Tests:** 7 files, ~160 tests

### 2.2 Infrastructure Files

| File | Role | Issues |
|---|---|---|
| `tests/setup_suite.bash` | Auto-starts containers, waits for health | Waits for Docker health only, not port readiness |
| `tests/helpers/common.bash` | 30+ helpers, container names, assertions | `wait_for_port` exists (line 270) but unused in setup; no `restore_security_level`; volume/image/user names not centralized |

### 2.3 Verified Metrics

| Metric | Count |
|---|---|
| Project `.bats` files | 37 |
| `teardown()` functions | 0 |
| `teardown_file()` functions | 0 |
| Files calling `relax_security_level` | 5 |
| External network references (e2e) | 59 |
| Bare `[[ ]]` assertions | 27 |
| Permanently skipped tests | 2 |
| Empty test bodies | 2 |
| Orphaned code outside `@test` | 1 file |

---

## 3. Security Vulnerabilities (From Security Audit)

These are not test-quality issues — they are **production security risks** exposed by the audit.

### V-01: Privileged Container Assertion Contradiction [CRITICAL]

`docker-compose.yml` declares `gate: privileged: true`. The test `security: gateway is NOT running privileged` asserts `false`. The test either skips (container not running) or fails (container IS running), creating false confidence. An attacker who compromises g3proxy gains full host kernel access.

**Fix:** Remove `privileged: true` from compose; use `cap_add: [NET_ADMIN, NET_RAW]` + `cap_drop: [ALL]` + seccomp profile. Add test verifying seccomp is active.

### V-02: Agent Self-Approval via Valkey ACL [CRITICAL]

The `mcp-agent` ACL user has `+setex` on `polis:approved:*`, allowing the agent to forge its own approvals and bypass human-in-the-loop review.

**Fix:** Revoke `+setex` on `polis:approved:*` from mcp-agent. Only `mcp-admin` should write approvals.

### V-03: Host Filesystem Secret Exposure [HIGH]

Tests read plaintext passwords from host filesystem (`cat "${PROJECT_ROOT}/secrets/..."`) instead of from container-internal tmpfs mounts.

**Fix:** Read secrets via `docker exec "$CONTAINER" cat /run/secrets/...`.

### V-04: 10 Security Baseline Mandates With Zero Test Coverage

Including: immutable audit logging, per-session memory isolation, tool invocation rate limiting, supply chain kill switch, seccomp profile enforcement on gate.

---

## 4. Refactoring Phases

### Phase 0: Surgical Fixes (30 min, zero structural change, zero risk)

Merge immediately — no dependencies, no side effects.

**0.1 — Delete orphaned code in `services/gate/tests/integration/security.bats`**

Lines 49-53 contain `skip`, `run`, `assert_success`, `assert_output` outside any `@test` block. `skip` is undefined outside test context. The preceding test already validates capabilities via `CapEff` bitmask.

Action: Delete the 5 orphaned lines.

**0.2 — Add assertions to empty test bodies in `tests/e2e/traffic.bats`**

Two tests (~lines 112-135) execute `curl` but have no assertions — they always pass regardless of outcome.

Action: Add proxy-involvement assertion:
```bash
@test "e2e: direct IP access is handled by proxy" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" \
        --connect-timeout 10 http://1.1.1.1 2>/dev/null
    # Must get an HTTP status (not 000) — proves proxy intercepted it
    [[ "$status" -ne 0 ]] || assert [ "$output" != "000" ]
}
```

**0.3 — Delete or fix permanently skipped tests in `services/gate/tests/integration/networking.bats`**

Two tests have unconditional `skip` and will never run. The nftables equivalents already exist in `workspace-isolation.bats`.

Action: Delete the dead tests or move the logic inside the container where capabilities exist.

---

### Phase 1: Teardown Discipline + State Isolation (2 hours)

**This phase is the single highest-impact change.** It eliminates cross-suite contamination and enables future parallel execution.

**1.1 — Implement `restore_security_level` in `tests/helpers/common.bash`**

This function does not exist anywhere in the codebase. It is the inverse of `relax_security_level`.

```bash
restore_security_level() {
    local admin_pass
    admin_pass=$(docker exec "$VALKEY_CONTAINER" \
        cat /run/secrets/valkey_mcp_admin_password 2>/dev/null) || return 0
    docker exec "$VALKEY_CONTAINER" sh -c "valkey-cli --tls \
        --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key \
        --cacert /etc/valkey/tls/ca.crt \
        --user mcp-admin --pass '$admin_pass' --no-auth-warning \
        DEL polis:config:security_level" 2>/dev/null || true
    docker restart "$ICAP_CONTAINER" >/dev/null 2>&1 || true
    wait_for_healthy "$ICAP_CONTAINER" 60 || true
}
```

**1.2 — Add `teardown_file()` to all 5 files calling `relax_security_level`**

Files: `traffic.bats`, `malware-scan.bats`, `dlp.bats`, `edge-cases.bats`, `workspace-isolation.bats`

```bash
teardown_file() {
    load "../helpers/common.bash"
    restore_security_level
}
```

**1.3 — Add `teardown()` to `tests/e2e/mcp-agent.bats` for Valkey key cleanup**

Currently, `cleanup_valkey_key` is inline — skipped on assertion failure.

```bash
teardown() {
    cleanup_valkey_key "polis:blocked:req-e2e${BATS_TEST_NUMBER}" 2>/dev/null || true
    cleanup_valkey_key "polis:approved:req-e2e${BATS_TEST_NUMBER}" 2>/dev/null || true
}
```

**1.4 — Add `teardown()` to `tests/e2e/dlp.bats` for temp file cleanup**

```bash
teardown() {
    docker exec "${WORKSPACE_CONTAINER}" rm -f /tmp/large_payload 2>/dev/null || true
}
```

**1.5 — Replace `mktemp -d` + inline `rm -rf` with `$BATS_TEST_TMPDIR` in `valkey-properties.bats`**

BATS auto-cleans `$BATS_TEST_TMPDIR` after each test — no manual cleanup needed.

**1.6 — Add `wait_for_port` to `setup_file()` in e2e files**

Use the EXISTING `wait_for_port` function (line 270 of `common.bash`). Do NOT create a new function. Place in `setup_file()`, NOT `setup()` — port readiness is a file-level invariant.

```bash
setup_file() {
    load "../helpers/common.bash"
    relax_security_level
    wait_for_port "$GATEWAY_CONTAINER" 18080 || skip "Gateway port 18080 not ready"
    wait_for_port "$ICAP_CONTAINER" 1344 || skip "ICAP port 1344 not ready"
}
```

---

### Phase 2: External Network Resilience (1 hour)

**2.1 — Add retry-on-failure wrapper to `tests/helpers/common.bash`**

Use a POST-CHECK pattern (not pre-check) to avoid TOCTOU race conditions:

```bash
run_with_network_skip() {
    local label="$1"; shift
    run "$@"
    if [[ "$status" -ne 0 ]]; then
        case "$output" in
            *"Could not resolve"*|*"Connection timed out"*|\
            *"Network is unreachable"*|*"Connection refused"*)
                skip "${label} unreachable — network-dependent test"
                ;;
        esac
    fi
}
```

**2.2 — Apply to all e2e tests hitting external services**

Affected files (59 external refs total):
- `tests/e2e/traffic.bats` — httpbin.org, github.com, ftp.gnu.org
- `tests/e2e/malware-scan.bats` — eicar.org, httpbin.org
- `tests/e2e/icap-hardening.bats` — httpbin.org, deb.debian.org, npmjs.org, github.com
- `tests/e2e/dlp.bats` — httpbin.org, google.com, anthropic.com
- `tests/e2e/edge-cases.bats` — httpbin.org

---

### Phase 3: Assertion Hygiene (1 hour)

**3.1 — Replace 27 bare `[[ ]]` with `assert` equivalents**

Mechanical transformation across all project `.bats` files:

```bash
# Before (no diagnostic on failure):
[[ "$output" -ge 1000 ]]

# After (shows actual vs expected):
assert [ "$output" -ge 1000 ]
```

Affected files: `traffic.bats` (5), `edge-cases.bats` (4), `dlp.bats` (4), `dns.bats` (4), `clamav.bats` (3), `icap-hardening.bats` (2), others (~5).

**3.2 — Move host-filesystem secret reads to container-internal reads**

```bash
# Before (leaks to CI logs if set -x):
local mcp_pass=$(cat "${PROJECT_ROOT}/secrets/valkey_mcp_agent_password.txt")

# After (reads from Docker tmpfs mount):
local mcp_pass
mcp_pass=$(docker exec "${VALKEY_CONTAINER}" cat /run/secrets/valkey_mcp_agent_password 2>/dev/null)
```

---

### Phase 4: Tier Correction + CI Integration (1 hour)

**4.1 — Add BATS file tags to all 37 project test files**

Uses BATS 1.5+ `# bats file_tags=` for tier-based CI filtering without moving any files:

```bash
# In services/gate/tests/unit/gateway.bats (misclassified):
# bats file_tags=integration,gate

# In services/resolver/tests/unit/dns.bats (true unit):
# bats file_tags=unit,resolver

# In tests/e2e/traffic.bats:
# bats file_tags=e2e,network
```

**4.2 — Add execution tests to `tests/unit/polis-script.bats`**

All 40 existing tests are `grep -q` against source text. A syntax error passes all tests.

Add at minimum:
```bash
@test "polis-script: passes bash syntax check" {
    run bash -n "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: --help prints usage" {
    run bash "${POLIS_SCRIPT}" --help
    assert_success
    assert_output --partial "Usage:"
}

@test "polis-script: unknown command exits non-zero" {
    run bash "${POLIS_SCRIPT}" nonexistent-command
    assert_failure
}
```

**4.3 — CI pipeline configuration**

```bash
# PR checks (fast, ~30s — no containers needed):
bats --filter-tags unit tests/ services/

# Post-merge (medium, ~2min — containers required):
bats --filter-tags integration tests/ services/

# Nightly (full stack, ~5min):
bats --formatter junit tests/ services/ > test-results.xml
```

Parallel execution (`--jobs 4`) is safe ONLY after Phase 1 is complete, and ONLY across tiers (unit ∥ integration ∥ e2e), not within e2e files that share ICAP state.

---

### Phase 5: Helper Extraction + Centralization (1 hour)

**5.1 — Create `tests/helpers/valkey.bash`**

Extract from `mcp-agent.bats` and `valkey.bats` (Rule of Three satisfied):

```bash
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

**5.2 — Create `tests/helpers/network.bash`**

Extract from `gateway-ipv6.bats` (reusable by `ipv6.bats`, `hardening.bats`):

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

**5.3 — Create `tests/fixtures/expected-names.bash`**

Single source of truth for names that drift between tests and `docker-compose.yml`:

```bash
export EXPECTED_SCANNER_IMAGE="polis-scanner-oss:latest"
export EXPECTED_SCANNER_VOLUME="polis-scanner-db"
export EXPECTED_STATE_VOLUME="polis-state-data"
export EXPECTED_ICAP_USER="sentinel"
export EXPECTED_ICAP_GROUP="sentinel"
```

---

### Phase 6: Security Coverage Gaps (4-8 hours, ongoing)

These address the 10 untested security baseline mandates identified in the security audit.

| # | New Test | Priority | Effort |
|---|----------|----------|--------|
| 6.1 | Gate seccomp profile is applied (currently commented out in compose) | P1 | 30m |
| 6.2 | DNS exfiltration prevention via encoded hostnames | P1 | 2h |
| 6.3 | MCP toolbox→workspace uses authenticated transport | P2 | 1h |
| 6.4 | Valkey TTL enforcement under concurrent access | P3 | 1h |
| 6.5 | Workspace state isolation between sessions | P3 | 1h |
| 6.6 | Audit log integrity (hash chain verification) | P3 | 2h |
| 6.7 | ClamAV config unit tests (currently zero) | P2 | 1h |
| 6.8 | Failure mode tests (ClamAV down, Valkey unreachable, DNS down) | P2 | 2h |

---

## 5. Target Directory Structure

Only 3 new files. No directory restructuring needed.

```
tests/
├── setup_suite.bash              # existing — unchanged
├── helpers/
│   ├── common.bash               # existing — add restore_security_level, run_with_network_skip
│   ├── valkey.bash               # NEW — extracted from mcp-agent.bats + valkey.bats
│   └── network.bash              # NEW — extracted from gateway-ipv6.bats
├── fixtures/
│   └── expected-names.bash       # NEW — centralized volume/image/user names
├── unit/
│   └── polis-script.bats         # existing — add execution tests
├── integration/                  # existing — unchanged
├── e2e/                          # existing — add teardown_file to 5 files
└── bats/                         # submodules — unchanged

services/<name>/tests/
├── unit/                         # existing — add file_tags for correct tier
└── integration/                  # existing — unchanged
```

---

## 6. Mocking Strategy

### Where to Mock

1. **CLI script tests (`polis-script.bats`)** — Shadow `docker`, `yq`, `sha256sum` via function export to test argument parsing without a running stack.
2. **Health check scripts** — Already well-tested via environment injection in `valkey-properties.bats`. Correct pattern.

### Where NOT to Mock

Integration and e2e tests must NOT mock Docker, containers, or network calls. Their value comes from testing the real stack. The `require_container` + `skip` pattern is the correct approach for graceful degradation.

---

## 7. Priority Matrix

| Phase | Effort | Risk | Impact | Prerequisite |
|---|---|---|---|---|
| **0: Surgical fixes** | 30m | None | Removes dead code + false positives | None |
| **1: Teardown discipline** | 2h | Low | Eliminates state leakage; enables `--jobs` | None |
| **2: Network resilience** | 1h | None | Prevents CI failures on external outages | None |
| **3: Assertion hygiene** | 1h | None | Better failure diagnostics; secret protection | None |
| **4: Tier correction + CI** | 1h | None | Enables fast PR checks (`--filter-tags unit`) | None |
| **5: Helper extraction** | 1h | None | DRY; enables new test files | None |
| **6: Security coverage** | 4-8h | Low | Aligns with security baseline mandates | Phases 0-1 |

**Execution order:** Phase 0 → Phase 1 (must complete before enabling parallel) → Phases 2-5 (any order, parallelizable across contributors) → Phase 6 (ongoing).

**Total effort: ~10-14 hours** (6h core refactoring + 4-8h security coverage gaps).

---

## 8. Coverage Gap Matrix

| Component | Unit | Integration | E2E | Critical Gap |
|---|---|---|---|---|
| gate (g3proxy) | ✅ 30 tests* | ✅ 18 tests | ✅ traffic.bats | Privileged mode untested; seccomp commented out |
| sentinel (c-icap) | ✅ 20 tests* | ✅ 69 tests | ✅ malware-scan, dlp | State mutation without teardown |
| resolver (CoreDNS) | ✅ 8 tests | ✅ 12 tests | Indirect only | **No e2e DNS exfiltration test** |
| scanner (ClamAV) | ❌ 0 tests | ✅ 2 tests | ✅ via malware-scan | **No unit tests; no config validation** |
| state (Valkey) | ✅ 25 tests* | ❌ 0 tests | ✅ via mcp-agent | **No integration tests; ACL bypass untested** |
| toolbox (MCP) | ✅ 12 tests* | ✅ 10 tests | ✅ mcp-agent.bats | No mTLS verification |
| workspace | ✅ 40 tests* | ✅ 15 tests | ✅ via all e2e | — |
| polis.sh CLI | ✅ 40 tests | — | — | **All grep-based; zero execution tests** |

*\* = misclassified as unit, actually integration*

---

## 9. Success Criteria

After all phases are complete:

1. `bats --filter-tags unit` runs in <30s with zero container dependencies
2. `bats --filter-tags integration` runs in <2min with containers
3. Full suite runs in <5min with `--jobs 4`
4. Zero `teardown_file` / `teardown` gaps — every state mutation has a restore
5. External network failures produce `skip`, not `fail`
6. All assertions produce diagnostic output on failure
7. CI produces JUnit XML for visibility
8. Security baseline mandates have ≥80% test coverage
9. No test reads secrets from host filesystem

---

## 10. Appendix: Flakiness Root Cause → Fix Mapping

| Root Cause | Affected Tests | Fix | Phase |
|---|---|---|---|
| `require_container` checks health, not port readiness | ~15 e2e | Add `wait_for_port` to `setup_file()` | 1.6 |
| External network dependency | 59 refs, 5 files | Add `run_with_network_skip()` post-check | 2.1-2.2 |
| `relax_security_level()` without `teardown_file()` | 5 files, ~40 tests | Add `teardown_file()` + `restore_security_level()` | 1.1-1.2 |
| Valkey key cleanup inline, not in `teardown()` | ~6 mcp-agent tests | Move to `teardown()` | 1.3 |
| Bare `[[ ]]` without diagnostics | 27 instances | Replace with `assert [ ]` | 3.1 |
| Orphaned code outside `@test` block | 1 file | Delete lines | 0.1 |
| Empty test bodies (no assertions) | 2 tests | Add proxy-involvement assertion | 0.2 |
| Host filesystem secret reads | ~8 tests | Read from container via `docker exec` | 3.2 |
