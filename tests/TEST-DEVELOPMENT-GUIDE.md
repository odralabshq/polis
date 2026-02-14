# Polis BATS Test Development Guide

**For:** SDETs and contributors writing or modifying BATS tests
**Last updated:** 2026-02-14
**Scope:** All `.bats` files under `tests/` and `services/*/tests/`

---

## 1. Architecture Overview

```
tests/
├── setup_suite.bash              # Auto-starts containers, waits for health
├── helpers/
│   ├── common.bash               # Core helpers, container names, assertions
│   ├── valkey.bash               # Valkey CLI helpers (valkey_cli_as, cleanup)
│   └── network.bash              # IPv6/network helpers
├── fixtures/
│   └── expected-names.bash       # Centralized volume/image/user names
├── unit/                         # True unit tests (no containers)
├── integration/                  # Cross-service integration tests
├── e2e/                          # Full-stack end-to-end tests
└── bats/                         # BATS submodules (do not edit)

services/<name>/tests/
├── unit/                         # Per-service tests (may require containers)
└── integration/                  # Per-service integration tests
```

---

## 2. Test Tiers

Every `.bats` file MUST have a `# bats file_tags=` line immediately after the shebang.

| Tier | Tag | Container Required | Runs In CI | Example |
|------|-----|--------------------|------------|---------|
| Unit | `unit` | No | PR checks (~30s) | `polis-script.bats` |
| Integration | `integration` | Yes | Post-merge (~2min) | `gateway.bats` |
| E2E | `e2e` | Yes + network | Nightly (~5min) | `traffic.bats` |

```bash
#!/usr/bin/env bats
# bats file_tags=integration,gate
```

Add a second tag for the component: `gate`, `sentinel`, `resolver`, `scanner`, `state`, `toolbox`, `workspace`, `cli`, `network`, `security`, `agents`.

**Classification rule:** If a test calls `docker exec`, it is NOT a unit test — tag it `integration` regardless of directory location.

### Running by tier

```bash
# Unit only (fast, no containers):
bats --filter-tags unit tests/ services/

# Integration only:
bats --filter-tags integration tests/ services/

# E2E only:
bats --filter-tags e2e tests/e2e/

# Full suite:
bats tests/ services/
```

---

## 3. File Structure Template

Every test file should follow this structure:

```bash
#!/usr/bin/env bats
# bats file_tags=<tier>,<component>
# <Description of what this file tests>

setup_file() {
    load "../helpers/common.bash"
    # File-level setup: relax_security_level, wait_for_port, etc.
}

teardown_file() {
    load "../helpers/common.bash"
    # File-level cleanup: restore_security_level, etc.
}

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER"  # skip if not running
}

teardown() {
    # Per-test cleanup (temp files, Valkey keys, etc.)
}

# =============================================================================
# Section Name
# =============================================================================

@test "component: descriptive test name" {
    # test body
}
```

---

## 4. Mandatory Rules

### 4.1 Every state mutation MUST have a teardown

If your test or `setup_file()` changes state, you MUST restore it.

| Mutation | Restore Location | Function |
|----------|-----------------|----------|
| `relax_security_level` | `teardown_file()` | `restore_security_level` |
| Valkey key creation | `teardown()` | `cleanup_valkey_key "key"` |
| Temp files in container | `teardown()` | `docker exec ... rm -f` |
| Host temp files | Use `$BATS_TEST_TMPDIR` | Auto-cleaned by BATS |

**Never use `mktemp -d` with manual `rm -rf`.** Use `$BATS_TEST_TMPDIR` instead — BATS auto-cleans it after each test.

```bash
# WRONG:
local tmpdir=$(mktemp -d)
# ... use tmpdir ...
rm -rf "$tmpdir"

# RIGHT:
run bash "${SCRIPT}" "${BATS_TEST_TMPDIR}"
# BATS cleans up automatically
```

### 4.2 Never read secrets from host filesystem

Read secrets from inside the container via Docker secrets:

```bash
# WRONG — leaks to CI logs if set -x:
local pass=$(cat "${PROJECT_ROOT}/secrets/valkey_password.txt")

# RIGHT — reads from Docker tmpfs mount:
local pass
pass=$(docker exec "${VALKEY_CONTAINER}" cat /run/secrets/valkey_password 2>/dev/null)
```

### 4.3 Never use bare `[[ ]]` for assertions

Bare `[[ ]]` produces no diagnostic output on failure. Always use `assert`:

```bash
# WRONG — silent failure:
[[ "$output" -ge 1000 ]]

# RIGHT — shows actual vs expected:
assert [ "$output" -ge 1000 ]
```

For regex matching, use `assert_output --partial` or `assert_output --regexp` when possible.

### 4.4 Every test body MUST have at least one assertion

A test with no assertion always passes — it's a false positive.

```bash
# WRONG — always passes:
@test "something works" {
    run docker exec "$CONTAINER" curl -s http://example.com
}

# RIGHT:
@test "something works" {
    run docker exec "$CONTAINER" curl -s -o /dev/null -w "%{http_code}" http://example.com
    assert_success
    assert_output "200"
}
```

### 4.5 No code outside `@test`, `setup`, `teardown`, or helper functions

Bare statements outside these blocks are undefined behavior in BATS. They will silently fail or cause confusing errors.

---

## 5. Container Guards

Always use `require_container` in `setup()` to skip gracefully when containers aren't running:

```bash
setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER"
}
```

This checks both running state and health status. If any container is missing or unhealthy, the test is skipped (not failed).

### Port readiness

Docker health ≠ port readiness. For e2e tests, add `wait_for_port` in `setup_file()`:

```bash
setup_file() {
    load "../helpers/common.bash"
    wait_for_port "$GATEWAY_CONTAINER" 18080 || skip "Gateway port not ready"
    wait_for_port "$ICAP_CONTAINER" 1344 || skip "ICAP port not ready"
}
```

Place port checks in `setup_file()` (not `setup()`) — port readiness is a file-level invariant.

---

## 6. External Network Dependencies

Any test hitting an external service (httpbin.org, github.com, etc.) MUST use the post-check wrapper:

```bash
@test "e2e: HTTP request succeeds" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" \
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://httpbin.org/get
    assert_success
    assert_output "200"
}
```

`run_with_network_skip` replaces `run`. It executes the command, then checks if the failure looks like a network issue (DNS failure, timeout, unreachable). If so, it `skip`s instead of failing.

**Do NOT use pre-check patterns** (checking network before the test) — they have TOCTOU race conditions.

**Do NOT wrap tests that expect failure** (e.g., "blocked port" tests) — the network skip would incorrectly trigger on the expected failure.

---

## 7. Available Helpers

### `tests/helpers/common.bash`

| Function | Purpose |
|----------|---------|
| `require_container "$NAME"` | Skip if container not running/healthy |
| `wait_for_healthy "$NAME" $TIMEOUT` | Block until container is healthy |
| `wait_for_port "$NAME" $PORT $TIMEOUT` | Block until port is listening |
| `relax_security_level` | Set Valkey security_level to relaxed |
| `restore_security_level` | Delete security_level override, restart ICAP |
| `run_with_network_skip "label" cmd...` | Run command, skip on network errors |
| `assert_container_running "$NAME"` | Assert container is Up |
| `assert_container_healthy "$NAME"` | Assert container health is healthy |
| `assert_port_listening "$NAME" $PORT` | Assert port is open |
| `assert_can_reach "$FROM" "$HOST" $PORT` | Assert TCP connectivity |
| `assert_cannot_reach "$FROM" "$HOST" $PORT` | Assert TCP blocked |
| `assert_http_success "$NAME" "$URL"` | Assert HTTP 200 |
| `assert_process_running "$NAME" "$PROC"` | Assert process exists |
| `get_container_ip "$NAME" "$NETWORK"` | Get container IP on network |

### `tests/helpers/valkey.bash`

```bash
load "../helpers/valkey.bash"

# Run valkey-cli as a specific ACL user
valkey_cli_as "mcp-admin" "valkey_mcp_admin_password" GET "polis:config:security_level"

# Clean up a test key (best-effort, never fails)
cleanup_valkey_key "polis:blocked:test-key"
```

### `tests/helpers/network.bash`

```bash
load "../helpers/network.bash"

# Check if ip6tables works in a container
if ! ip6tables_functional "$GATEWAY_CONTAINER"; then
    skip "ip6tables not functional"
fi

# Check if IPv6 is disabled
ipv6_disabled "$GATEWAY_CONTAINER"
```

### `tests/fixtures/expected-names.bash`

```bash
load "../fixtures/expected-names.bash"

assert_output "$EXPECTED_SCANNER_IMAGE"
assert_output --partial "$EXPECTED_STATE_VOLUME"
```

Use these instead of hardcoding names that may drift from `docker-compose.yml`.

---

## 8. Container Names

Defined in `common.bash` — always use the variables, never hardcode:

| Variable | Container |
|----------|-----------|
| `$DNS_CONTAINER` | `polis-resolver` |
| `$GATEWAY_CONTAINER` | `polis-gate` |
| `$ICAP_CONTAINER` | `polis-sentinel` |
| `$WORKSPACE_CONTAINER` | `polis-workspace` |
| `$CLAMAV_CONTAINER` | `polis-scanner` |
| `$VALKEY_CONTAINER` | `polis-state` |
| `$MCP_AGENT_CONTAINER` | `polis-toolbox` |

---

## 9. Naming Conventions

### Test names

Format: `"<component>: <descriptive name>"`

```bash
@test "security: gateway is NOT running privileged" { ... }
@test "e2e-dlp: Anthropic key to google.com is BLOCKED" { ... }
@test "property 1: all 11 dangerous commands return error" { ... }
@test "tproxy: g3proxy listening on port 18080" { ... }
```

Prefixes by tier:
- Integration: component name (`security:`, `tproxy:`, `network:`)
- E2E: `e2e:`, `e2e-dlp:`, `e2e-av:`, `e2e-mcp:`, `e2e-hardening:`
- Unit: `polis-script:`, `property N:`

### Section headers

Use comment blocks to group related tests:

```bash
# =============================================================================
# Section Name
# =============================================================================
```

---

## 10. Mocking Strategy

### Where to mock

- **CLI script tests** (`polis-script.bats`): Shadow `docker`, `yq`, `sha256sum` via function export to test argument parsing without a running stack.
- **Health check scripts**: Use environment variable injection (see `valkey-properties.bats` property 10).

### Where NOT to mock

Integration and e2e tests MUST NOT mock Docker, containers, or network calls. Their value comes from testing the real stack. Use `require_container` + `skip` for graceful degradation.

---

## 11. Parallel Execution Safety

Tests can run with `--jobs N` ONLY if:

1. Every `relax_security_level` has a matching `teardown_file()` with `restore_security_level`
2. Every Valkey key mutation has a `teardown()` cleanup
3. No test depends on output from another test

Safe parallelism: across tiers (`unit ∥ integration ∥ e2e`).
**NOT safe:** within e2e files that share ICAP state.

---

## 12. Checklist Before Submitting

- [ ] File has `# bats file_tags=` on line 2
- [ ] Correct tier tag (`unit` only if zero container deps)
- [ ] `setup()` calls `require_container` for all needed containers
- [ ] Every state mutation has a matching teardown
- [ ] No bare `[[ ]]` — use `assert` equivalents
- [ ] Every `@test` has at least one assertion
- [ ] External network calls use `run_with_network_skip`
- [ ] Secrets read from container (`/run/secrets/`), not host filesystem
- [ ] Temp files use `$BATS_TEST_TMPDIR` (not `mktemp -d`)
- [ ] No code outside `@test`/`setup`/`teardown`/helper functions
- [ ] Container names use variables from `common.bash`
- [ ] Volume/image names use `expected-names.bash` where applicable

---

## 13. Common Patterns

### Testing a config value inside a container

```bash
@test "config: maxsize is 100M" {
    run docker exec "$ICAP_CONTAINER" grep "^maxsize" /etc/squidclamav.conf
    assert_success
    assert_output "maxsize 100M"
}
```

### Testing something is NOT present

```bash
@test "security: no abort directives in config" {
    run docker exec "$ICAP_CONTAINER" grep "^abort " /etc/squidclamav.conf
    assert_failure  # grep returns 1 when no match
}
```

### Testing HTTP through the proxy stack

```bash
@test "e2e: HTTPS request succeeds" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" \
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 https://httpbin.org/get
    assert_success
    assert_output "200"
}
```

### Testing DLP blocking (expects specific headers)

```bash
@test "e2e-dlp: credential to unauthorized destination is BLOCKED" {
    run_with_network_skip "google.com" docker exec "${WORKSPACE_CONTAINER}" \
        curl -s -D - -o /dev/null -X POST -d "key=${SECRET}" \
        --connect-timeout 15 https://www.google.com
    assert_output --partial "x-polis-block: true"
    assert_output --partial "x-polis-reason: credential_detected"
}
```

### Testing Valkey ACL enforcement

```bash
@test "acl: mcp-agent denied DEL on allowed keys" {
    load "../helpers/valkey.bash"
    run valkey_cli_as "mcp-agent" "valkey_mcp_agent_password" DEL "polis:blocked:test"
    assert_output --partial "NOPERM"
}
```

### Property-based testing over input domains

```bash
@test "property: all dangerous commands return error" {
    local commands=("FLUSHALL" "FLUSHDB" "DEBUG" "CONFIG" "SHUTDOWN")
    for cmd in "${commands[@]}"; do
        local result
        result="$(valkey_cli_as "mcp-admin" "valkey_mcp_admin_password" ${cmd} 2>&1 || true)"
        if [[ "${result}" != *"NOPERM"* ]] && [[ "${result}" != *"ERR"* ]]; then
            fail "Command ${cmd} was not blocked. Got: ${result}"
        fi
    done
}
```

---

## 14. Debugging Tips

```bash
# Run a single test with verbose output:
bats --verbose-run tests/e2e/traffic.bats --filter "HTTP request"

# Run with TAP output for CI:
bats --formatter tap tests/

# Run with JUnit XML:
bats --formatter junit tests/ > test-results.xml

# Check which tests would run for a tag:
bats --filter-tags unit --count tests/ services/

# Debug a flaky test — run it 5 times:
for i in {1..5}; do bats tests/e2e/traffic.bats --filter "slow responses"; done
```
