# Polis Core Test Suite

Comprehensive testing suite for polis-core infrastructure using [BATS](https://github.com/bats-core/bats-core) (Bash Automated Testing System).

## Structure

```
tests/
├── README.md                    # This file
├── run-tests.sh                 # Main test runner
├── setup_suite.bash             # Suite-level setup/teardown
├── helpers/
│   └── common.bash              # Shared helpers and assertions
├── unit/
│   ├── gateway.bats             # Gateway container unit tests
│   ├── icap.bats                # ICAP container unit tests
│   └── workspace.bats           # Workspace container unit tests
├── integration/
│   ├── network.bats             # Network isolation tests
│   ├── tproxy.bats              # TPROXY/iptables tests
│   └── security.bats            # Security hardening tests
└── e2e/
    ├── traffic.bats             # Traffic interception tests
    └── edge-cases.bats          # Edge case and failure mode tests
```

## Prerequisites

- Docker and Docker Compose
- Bash 4.0+
- BATS (installed automatically by `run-tests.sh`)

## Quick Start

```bash
# Run all tests
./tests/run-tests.sh

# Run specific test category
./tests/run-tests.sh unit
./tests/run-tests.sh integration
./tests/run-tests.sh e2e

# Run specific test file
./tests/run-tests.sh unit/gateway.bats

# Run with verbose output
./tests/run-tests.sh -v

# Run with TAP output (for CI)
./tests/run-tests.sh --tap
```

## Test Categories

### Unit Tests (`unit/`)

Container-level tests that verify individual components work correctly in isolation:

- **gateway.bats**: g3proxy/g3fcgen binaries, config validation, init scripts
- **icap.bats**: c-icap service, configuration, user/permissions
- **workspace.bats**: systemd, CA certificates, init scripts

### Integration Tests (`integration/`)

Tests that verify components work together correctly:

- **network.bats**: Docker network isolation, DNS resolution, inter-container connectivity
- **tproxy.bats**: TPROXY iptables rules, policy routing, socket matching
- **security.bats**: Capabilities, seccomp profiles, privilege restrictions

### E2E Tests (`e2e/`)

End-to-end tests that verify the complete system works as expected:

- **traffic.bats**: HTTP/HTTPS interception, TLS MITM, blocked traffic
- **edge-cases.bats**: Failure modes, recovery scenarios, certificate handling

## Writing Tests

Tests use BATS syntax with helper assertions:

```bash
@test "gateway container is running" {
    run docker ps --filter name=polis-gateway --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}
```

### Available Helpers

```bash
# Container assertions
assert_container_running "container-name"
assert_container_healthy "container-name"
assert_container_not_privileged "container-name"

# Network assertions
assert_port_listening "container-name" "port"
assert_can_reach "from-container" "to-host" "port"
assert_cannot_reach "from-container" "to-host" "port"

# Docker assertions
assert_has_capability "container-name" "CAP_NAME"
assert_has_seccomp "container-name"
```

## CI Integration

For CI pipelines, use TAP output:

```bash
./tests/run-tests.sh --tap > test-results.tap
```

Or use JUnit output (requires bats-core 1.5+):

```bash
./tests/run-tests.sh --formatter junit > test-results.xml
```

## Troubleshooting

### Tests hang or timeout

- Ensure containers are running: `../cli/polis.sh status`
- Check container logs: `../cli/polis.sh logs`
- Increase timeout: `BATS_TEST_TIMEOUT=120 ./run-tests.sh`

### BATS not found

- Run `./run-tests.sh` which auto-installs BATS
- Or manually: `git submodule update --init --recursive`

### Permission denied

- Ensure scripts are executable: `chmod +x run-tests.sh`
- Some tests require root/sudo for Docker access
