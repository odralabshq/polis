# Test Development Guide

How to write, run, and maintain tests for Polis.

## Directory Structure

```
tests/
├── unit/                          # Tier 1: No Docker, no network
│   ├── cli/                       # polis.sh and agent manifest tests
│   ├── config/                    # Config file validation (g3proxy, valkey, etc.)
│   ├── dlp/                       # DLP config validation
│   ├── scripts/                   # Shell script syntax and logic
│   └── security/                  # Seccomp profiles, blocklists, ACLs, Dockerfiles
├── integration/                   # Tier 2: Running containers required
│   ├── config/                    # Runtime config verification
│   ├── container/                 # Lifecycle, images, resources, restart policies
│   ├── network/                   # Topology, DNS, isolation, TPROXY, IPv6
│   ├── security/                  # Capabilities, privileges, mounts, users
│   ├── service/                   # Per-service process and state checks
│   └── state/                     # Valkey ACL, TLS, persistence
├── e2e/                           # Tier 3: Full stack + external network
│   ├── agents/                    # Agent system tests
│   ├── dlp/                       # Credential detection through proxy chain
│   ├── dns/                       # Domain blocking from workspace
│   ├── scanning/                  # Malware detection, scan bypass prevention
│   ├── toolbox/                   # MCP tools, approval system
│   └── traffic/                   # HTTP/HTTPS flow, edge cases, concurrency
├── lib/                           # Shared test infrastructure
│   ├── assertions/                # Custom assertion functions
│   ├── fixtures/                  # Test data (expected values, invalid configs)
│   ├── mocks/                     # Mock helpers for unit tests
│   ├── constants.bash             # Container names, network names, paths
│   ├── guards.bash                # Skip guards (require_container, etc.)
│   └── test_helper.bash           # Main loader — loads bats libs + constants + guards
├── bats/                          # BATS framework (git submodules, do not edit)
├── native/                        # Native C tests (out of scope for BATS suite)
└── run-tests.sh                   # Test runner
```

## Tier Rules

| Tier | Tag | Dependencies | Speed Target |
|------|-----|-------------|-------------|
| Unit | `unit` | None. No Docker, no network. | < 30s total |
| Integration | `integration` | Running Docker containers | < 3min |
| E2E | `e2e` | Full stack + external network | < 10min |

**Tier boundaries are strict.** Unit tests must never call `docker`. Integration tests must never hit external networks. E2E tests run commands inside the workspace container.

## Running Tests

```bash
# Run a single tier
./tests/run-tests.sh unit
./tests/run-tests.sh integration
./tests/run-tests.sh e2e

# Run all tiers
./tests/run-tests.sh all

# CI mode (resets state, starts httpbin for e2e)
./tests/run-tests.sh --ci all

# Filter by tag
./tests/run-tests.sh --filter-tags security integration

# Run a single file
./tests/bats/bats-core/bin/bats tests/unit/config/g3proxy-config.bats
```

## Writing a New Test

### 1. Choose the tier

- Testing a config file, script syntax, or static property? → `unit`
- Testing container state, docker inspect, inter-service connectivity? → `integration`
- Testing traffic through the full proxy chain from workspace? → `e2e`

### 2. Create the file

```bash
#!/usr/bin/env bats
# bats file_tags=<tier>,<concern>
# Brief description of what this file tests
```

Valid concern tags: `config`, `security`, `network`, `scanning`, `dlp`, `dns`, `state`, `toolbox`, `traffic`, `cli`, `agents`, `container`, `service`

### 3. Load helpers

```bash
# Unit tests
setup() {
    load "../../lib/test_helper.bash"
}

# Integration/E2E tests
setup() {
    load "../../lib/test_helper.bash"
    require_container "$GATEWAY_CONTAINER"
}
```

### 4. Write tests

Each `@test` asserts ONE thing. Always use `run` before the command under test.

```bash
@test "g3proxy config: has resolver section" {
    run grep -q "^resolver:" "$PROJECT_ROOT/services/gate/config/g3proxy.yaml"
    assert_success
}
```

### 5. Verify expected values against source

Read the actual config/source file before writing the expected value. Never copy values from other tests.

## Test Naming Convention

```
"<scope>: <what is tested>"
```

- Unit: `"g3proxy config: has resolver section"`
- Integration: `"gate: g3proxy process running"`
- E2E: `"e2e: HTTP GET returns 200"`

## Key Helpers

| Function | Purpose |
|----------|---------|
| `require_container "$NAME"` | Skip test if container not running |
| `assert_container_running "$NAME"` | Assert container is in running state |
| `assert_container_healthy "$NAME"` | Assert container health check passes |
| `assert_port_listening "$CONTAINER" $PORT` | Assert port is open inside container |
| `assert_process_running "$CONTAINER" "$PROC"` | Assert process exists in container |
| `run_with_network_skip "$HOST" cmd...` | Skip gracefully if external host unreachable |
| `relax_security_level` / `restore_security_level` | Temporarily relax DLP for testing |
| `wait_for_healthy "$CONTAINER"` | Block until container is healthy |
| `get_container_ip "$CONTAINER" "$NETWORK"` | Get container IP on a network |

## Anti-Patterns

- **No `docker` in unit tests.** Read from `$PROJECT_ROOT` paths instead.
- **No `assert_output` without `run`.** Always capture with `run` first.
- **No hardcoded values.** Verify against source files.
- **No state leaks.** Pair `relax_security_level` with `restore_security_level` in `teardown_file()`.
- **No network calls in integration tests.** Use `run_with_network_skip` in E2E only.
- **No duplicate assertions across files.** Each property is tested in exactly one file.

## CI Pipeline

The CI runs in three stages:

1. `unit-tests` — runs on every PR, no Docker needed
2. `integration-tests` — runs on every PR, needs containers
3. `e2e-tests` — runs on push to `main` only, needs full stack + network
