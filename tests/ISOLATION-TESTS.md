# Workspace Isolation Test Suite

## Purpose
Comprehensive tests to verify zero-trust network isolation and prevent regression to WSL2 cruft.

## Test Coverage (27 tests)

### Architecture Tests (7 tests)
Prevent regression to WSL2 artifacts:
- ✅ Only `inet polis` table exists (no old `ip polis_nat`/`ip polis_mangle`)
- ✅ No masquerade rule (WSL2 cruft removed)
- ✅ Forward chain has `policy drop` (zero-trust)
- ✅ TPROXY rule exists in `prerouting_tproxy` chain
- ✅ DNS DNAT rule exists in `prerouting_dnat` chain
- ✅ IPv6 blocked (defense-in-depth)
- ✅ Docker DNS rules preserved (not flushed by `flush ruleset`)

### Positive Tests (4 tests)
Verify allowed traffic works:
- ✅ HTTP via TPROXY
- ✅ HTTPS via TPROXY
- ✅ DNS resolution (forced through CoreDNS)
- ✅ Internal service name resolution (Docker DNS)

### Negative Tests (5 tests)
Verify blocked traffic fails (zero-trust):
- ✅ Cannot directly access gateway-bridge services (10.30.1.x)
- ✅ Cannot directly access external-bridge (10.20.1.x)
- ⏭️ Cannot ping external IPs (ICMP blocked) — skipped, ping not in workspace
- ⏭️ Cannot use arbitrary UDP ports — skipped, nc not in workspace
- ✅ DNS to external resolvers is DNATed to CoreDNS

### Regression Tests (7 tests)
Prevent known bugs from returning:
- ✅ Health check completes quickly (< 10s, was timing out at 5s)
- ✅ Gate can resolve sentinel via Docker DNS (flush ruleset was breaking this)
- ✅ TPROXY exclusions include all internal subnets
- ✅ Forward chain logs dropped packets (`[polis-drop]` prefix)
- ✅ Forward drop counter is functional
- ✅ Policy routing configured (fwmark 0x2 → table 102 → local)
- ✅ Required sysctls set (ip_forward, ip_nonlocal_bind, rp_filter, route_localnet)

### Security Boundary Tests (4 tests)
Verify network topology:
- ✅ Workspace default route points to gate only (10.10.1.10)
- ✅ Workspace on internal-bridge only (single interface)
- ✅ Gate has three interfaces (internal, gateway, external)
- ✅ Workspace cannot see other Docker networks

## Running Tests

```bash
# Run all isolation tests
./tests/run-tests.sh tests/integration/workspace-isolation.bats

# Run specific test
./tests/run-tests.sh tests/integration/workspace-isolation.bats -f "masquerade"

# Verbose output
./tests/run-tests.sh tests/integration/workspace-isolation.bats -v
```

## What These Tests Catch

### WSL2 Cruft Regression
If someone accidentally re-adds:
- `masquerade` rule → Test 2 fails
- Old `ip polis_nat` table → Test 1 fails
- `flush ruleset` → Test 7 fails (Docker DNS breaks)

### Zero-Trust Violations
If someone weakens isolation:
- Changes forward policy to `accept` → Test 3 fails
- Removes TPROXY rule → Test 4 fails
- Allows direct gateway-bridge access → Test 12 fails

### Configuration Drift
If sysctls or routing changes:
- Missing `ip_nonlocal_bind=1` → Test 22 fails
- Missing policy routing → Test 21 fails
- Wrong default route in workspace → Test 23 fails

## Test Philosophy

**Defense-in-depth**: Multiple layers tested (nftables + sysctls + Docker networks)

**Fail-safe defaults**: Tests verify restrictive policies, not permissive ones

**Observability**: Tests check logging and counters exist for monitoring

**Regression prevention**: Each bug fix gets a test to prevent recurrence
