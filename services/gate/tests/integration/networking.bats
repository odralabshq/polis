#!/usr/bin/env bats
# Gate Networking Integration Tests (TProxy & Routing)

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$GATEWAY_CONTAINER"
}

# =============================================================================
# TPROXY Rule Tests
# =============================================================================

@test "tproxy: G3TPROXY chain exists in mangle table" {
    # Skip iptables check for non-root containers as they cannot list rules
    skip "Cannot list iptables rules as non-root user"
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -n
    assert_success
}

@test "tproxy: TPROXY redirects to port 18080" {
    # Skip iptables check for non-root containers as they cannot list rules
    skip "Cannot list iptables rules as non-root user"
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -n -v
    assert_success
    assert_output --partial "18080"
}

@test "tproxy: g3proxy listening on port 18080" {
    run docker exec "${GATEWAY_CONTAINER}" ss -tln
    assert_success
    assert_output --partial ":18080"
}

# =============================================================================
# Policy Routing Tests
# =============================================================================

@test "tproxy: ip rule for fwmark 0x1 exists" {
    run docker exec "${GATEWAY_CONTAINER}" ip rule show
    assert_success
    assert_output --partial "fwmark 0x1"
}

@test "tproxy: routing table 100 has local route" {
    run docker exec "${GATEWAY_CONTAINER}" ip route show table 100
    assert_success
    assert_output --partial "local"
}

# =============================================================================
# Connectivity
# =============================================================================

@test "network: gateway has IP forwarding enabled" {
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/ip_forward
    assert_success
    assert_output "1"
}

@test "network: gateway can resolve icap via DNS" {
    run docker exec "${GATEWAY_CONTAINER}" getent hosts sentinel
    assert_success
}
